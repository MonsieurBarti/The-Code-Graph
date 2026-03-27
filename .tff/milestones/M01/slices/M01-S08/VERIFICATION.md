# Verification -- M01-S08: Agent Integration

## Summary
- Total: 17/17 PASS
- Status: PASS

## Criteria

### AC1: `code-graph setup claude` creates `.claude/settings.json` with SessionStart and PostToolUse hooks
**PASS**
Evidence: `setup.rs` `run_install` (lines 188-238) creates both hooks via `hook_definitions()` which returns SessionStart and PostToolUse entries. `session_start_hook()` (line 54) produces matcher `"startup"` with command `"code-graph index --incremental 2>/dev/null || true"` and timeout 120. `post_tool_use_hook()` (line 67) produces matcher `"Edit|Write"` with the jq-based file extraction command and timeout 15. Test `install_creates_hooks_in_empty_settings` (line 434) verifies both hooks are created with correct structure. Test `hook_definitions_have_correct_structure` (line 403) validates the exact commands and timeouts.

### AC2: `code-graph setup claude --global` writes hooks to `~/.claude/settings.json`
**PASS**
Evidence: `setup_helpers.rs` `resolve_settings_path` (lines 12-25) returns `$HOME/.claude/settings.json` when `global == true`. Test `resolve_settings_path_global` (line 143) verifies this path resolution. The `run_install` function at line 189 calls `resolve_settings_path(project_root, args.global)` which routes through this logic.

### AC3: Install is idempotent -- no duplicate hooks
**PASS**
Evidence: `run_install` (lines 206-220) scans for existing code-graph entries via `is_code_graph_hook` and replaces in place if found, appends only if not found. Test `install_idempotent_no_duplicates` (line 491) installs twice and verifies exactly 1 entry per event. Test `install_updates_outdated_hooks` (line 506) verifies an old command is replaced in place without duplication. Integration test `idempotent_install_no_duplicates_integration` (line 847) installs three times and verifies exactly 1 entry per event plus exactly 1 gitignore entry.

### AC4: Other settings preserved -- only hooks subtree modified
**PASS**
Evidence: Settings are manipulated as `serde_json::Value` objects. `run_install` reads existing settings, only modifies the `hooks` key, and writes back. `serde_json` with `preserve_order` feature (Cargo.toml line 15) maintains key order. Test `install_preserves_existing_settings` (line 451) pre-populates env and permissions, installs hooks, and verifies env/permissions remain intact. Test `install_on_existing_settings_preserves_other_hooks` (line 819) verifies non-code-graph hooks in other event keys (PreToolUse) are preserved.

### AC5: `.code-graph/` appended to `.gitignore`
**PASS**
Evidence: `setup_helpers.rs` `ensure_gitignore_entry` (lines 30-63) checks for existing `.code-graph/` line, appends `# Code Graph data\n.code-graph/\n` if absent. Called from `run_install` at line 227. Test `install_adds_gitignore_entry` (line 531) verifies. Tests `ensure_gitignore_creates_file`, `ensure_gitignore_appends`, `ensure_gitignore_idempotent`, and `ensure_gitignore_handles_no_trailing_newline` (lines 161-213) cover edge cases.

### AC6: `code-graph setup --check` reports per-hook status, binary/jq presence, settings location
**PASS**
Evidence: `run_check` (lines 145-184) outputs: binary path via `find_on_path("code-graph")`, jq path via `find_on_path("jq")`, settings file location (line 164), and per-hook status via `check_hook_status` which returns `Installed`, `Outdated`, or `Missing` (lines 116-143). Output format matches spec (lines 150-173). Tests `check_all_installed_reports_ok`, `check_missing_hooks_reports_missing`, `check_outdated_hook_reports_outdated`, `check_hook_status_installed`, `check_hook_status_missing`, `check_hook_status_outdated` (lines 545-622) verify all states.

### AC7: Exit code 0 if all installed, 1 if any missing/outdated
**PASS**
Evidence: `run_check` returns `Ok(())` when `all_installed` is true (line 180) and `Err(CodeGraphError::Other("Some hooks are missing or outdated"))` otherwise (line 182). The CLI framework propagates errors as non-zero exit. Tests `check_all_installed_reports_ok` (line 545) asserts `result.is_ok()` and `check_missing_hooks_reports_missing` (line 559) asserts `result.is_err()`.

### AC8: `--remove` removes code-graph hooks, leaves others untouched
**PASS**
Evidence: `run_remove` (lines 243-285) iterates event arrays and calls `arr.retain(|entry| !is_code_graph_hook(entry))` (line 251), keeping only non-code-graph hooks. Test `remove_filters_code_graph_hooks` (line 627) sets up settings with both a code-graph and a non-code-graph hook, removes, and verifies only the non-code-graph hook remains. Test `remove_preserves_other_settings` (line 767) verifies env settings are untouched.

### AC9: `--clean` also removes `.code-graph/` from `.gitignore`
**PASS**
Evidence: `run_remove` at lines 265-269 calls `remove_gitignore_entry(root)` when `args.clean || args.purge`. `remove_gitignore_entry` in `setup_helpers.rs` (lines 71-108) filters out both `.code-graph/` and `# Code Graph data` lines. Test `remove_with_clean_removes_gitignore` (line 705) verifies the gitignore entry is removed after `--clean`. Test `remove_gitignore_entry_removes_line_and_comment` (line 218) verifies both lines are removed.

### AC10: `--purge` also deletes `.code-graph/` directory
**PASS**
Evidence: `run_remove` at lines 272-280 calls `fs::remove_dir_all` on `.code-graph/` when `args.purge` and the directory exists. Test `remove_with_purge_deletes_data_dir` (line 723) creates a `.code-graph/` with a file, purges, and asserts `!data_dir.exists()`. Integration test `purge_deletes_data_directory_integration` (line 868) does the same with multiple files and also verifies gitignore cleanup.

### AC11: Remove is safe when hooks already absent
**PASS**
Evidence: `run_remove` only enters the hook-removal logic if `settings.get_mut("hooks")` yields `Some` (line 247). If no hooks object exists, it skips to writing back the unchanged settings and returns `Ok(())`. Test `remove_noop_when_no_hooks` (line 692) writes empty `{}` settings and verifies `run_remove` returns `Ok`.

### AC12: Unknown platform prints error with supported platforms list
**PASS**
Evidence: `run_setup` at lines 307-310 checks `if platform != "claude"` and returns `Err(CodeGraphError::Other(format!("Unsupported platform '{}'. Supported: claude", platform)))`. Test `install_unknown_platform_errors` (line 744) passes `"cursor"`, asserts error contains `"Unsupported platform"` and `"claude"`.

### AC13: Invalid JSON in settings.json prints clear error (no corruption)
**PASS**
Evidence: `read_settings` at lines 21-23 returns `Err(CodeGraphError::Other(format!("Invalid JSON in {}: {}", path.display(), e)))` when JSON parsing fails. No write is attempted. Test `read_settings_errors_on_invalid_json` (line 347) writes broken JSON, calls `read_settings`, and verifies the error contains `"Invalid JSON"` and the filename.

### AC14: Missing `jq` warns but doesn't error on install
**PASS**
Evidence: `run_install` at lines 233-235 checks `find_on_path("jq").is_none()` and prints `"Warning: jq not found -- PostToolUse hook will not extract file paths. Install jq for per-file incremental indexing."` without returning an error. The install completes successfully regardless of jq availability.

### AC15: `--global` outside git project skips .gitignore with info message
**PASS**
Evidence: `run_install` at lines 226-229: when `project_root` is `None` (outside a git project) and `args.global` is true, it prints `"Not inside a git project -- skipping .gitignore."` instead of erroring. The `find_project_root_optional` function (line 289) returns `None` when no project root is found, and `resolve_settings_path` with `global == true` does not require a project root (setup_helpers.rs line 13).

### AC16: `cargo test --workspace` passes
**PASS**
Evidence: `cargo test --workspace` completed with 460 tests passed across 11 suites, 0 failures.

### AC17: `cargo clippy --workspace -- -Dwarnings` passes
**PASS**
Evidence: `cargo clippy --workspace -- -Dwarnings` completed with no issues found.
