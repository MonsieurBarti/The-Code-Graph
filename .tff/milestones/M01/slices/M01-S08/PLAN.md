# Plan — M01-S08: Agent Integration

> For agentic workers: execute task-by-task with TDD.

**Goal:** Implement the `code-graph setup` command that installs, verifies, and removes Claude Code hooks for automatic graph indexing. One-command setup experience: `code-graph setup claude` installs SessionStart + PostToolUse hooks into Claude Code's settings.json, adds `.code-graph/` to `.gitignore`, and verifies dependencies.

**Architecture:** Single new module `crates/cli/src/commands/setup.rs` (~350 lines). No new crate dependencies — only `serde_json` `preserve_order` feature added. Settings JSON manipulated via `serde_json::Value` (preserves unknown keys). Hook identification by `"code-graph"` substring in command field.

**Key decisions (from RESEARCH):**
- Single file `setup.rs` — follows existing command pattern (watch.rs, index.rs)
- `serde_json` `preserve_order` feature — prevents key reordering on write-back
- `$HOME` for global path — no new dependency (already used in project.rs)
- `which` via `std::process::Command` — no `which` crate needed
- Hook identification by command substring — simple, reliable

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/cli/Cargo.toml` | Modify | Add `preserve_order` feature to serde_json |
| `crates/cli/src/commands/mod.rs` | Modify | Add `SetupArgs`, change `Setup` variant, update tests |
| `crates/cli/src/commands/setup.rs` | Create | Install/check/remove logic, settings JSON management, .gitignore management |
| `crates/cli/src/lib.rs` | Modify | Wire `Commands::Setup(args)` dispatch |

---

## Acceptance Criteria

> Numbering follows SPEC.md.

### Install
- AC1: `code-graph setup claude` creates `.claude/settings.json` with SessionStart and PostToolUse hooks.
- AC2: `code-graph setup claude --global` writes hooks to `~/.claude/settings.json`.
- AC3: Install is idempotent — no duplicate hooks.
- AC4: Other settings preserved — only `hooks` subtree modified.
- AC5: `.code-graph/` appended to `.gitignore`.

### Check
- AC6: `code-graph setup --check` reports per-hook status, binary/jq presence, settings location.
- AC7: Exit code 0 if all installed, 1 if any missing/outdated.

### Remove
- AC8: `--remove` removes code-graph hooks, leaves others untouched.
- AC9: `--clean` also removes `.code-graph/` from `.gitignore`.
- AC10: `--purge` also deletes `.code-graph/` directory.
- AC11: Remove is safe when hooks already absent.

### Error Handling
- AC12: Unknown platform prints error with supported platforms list.
- AC13: Invalid JSON in settings.json prints clear error (no corruption).
- AC14: Missing `jq` warns but doesn't error on install.
- AC15: `--global` outside git project skips .gitignore with info message.

### Quality
- AC16: `cargo test --workspace` passes.
- AC17: `cargo clippy --workspace -- -Dwarnings` passes.

---

## Wave 0 — CLI Wiring + Dependency

### T01: SetupArgs + Commands enum + dispatch wiring
**AC coverage:** (structural prerequisite, partially AC12)
**Files:** `crates/cli/Cargo.toml`, `crates/cli/src/commands/mod.rs`, `crates/cli/src/commands/setup.rs`, `crates/cli/src/lib.rs`

1. Enable `preserve_order` feature in `cli/Cargo.toml`:
   ```toml
   serde_json = { version = "1", features = ["preserve_order"] }
   ```
2. Add `pub mod setup;` to `commands/mod.rs` module declarations
3. Add `SetupArgs` struct in `commands/mod.rs`:
   ```rust
   #[derive(clap::Args)]
   pub struct SetupArgs {
       /// Target platform (currently: "claude")
       pub platform: Option<String>,
       /// Install to ~/.claude/settings.json instead of .claude/settings.json
       #[arg(long)]
       pub global: bool,
       /// Check hook installation status
       #[arg(long)]
       pub check: bool,
       /// Remove all code-graph hooks
       #[arg(long)]
       pub remove: bool,
       /// Also remove .code-graph/ from .gitignore (requires --remove)
       #[arg(long, requires = "remove")]
       pub clean: bool,
       /// Also delete .code-graph/ directory entirely (requires --remove)
       #[arg(long, requires = "remove")]
       pub purge: bool,
   }
   ```
4. Change `Commands::Setup,` (unit variant) to `Setup(SetupArgs),`
5. Update `all_subcommands_parse` test:
   - Change `vec!["code-graph", "setup"]` to `vec!["code-graph", "setup", "claude"]`
   - Add: `vec!["code-graph", "setup", "--check"]`
   - Add: `vec!["code-graph", "setup", "--remove"]`
   - Add: `vec!["code-graph", "setup", "--remove", "--clean"]`
   - Add: `vec!["code-graph", "setup", "--remove", "--purge"]`
6. Create `setup.rs` with stub `run_setup`:
   ```rust
   use domain::error::{CodeGraphError, Result};
   use super::SetupArgs;

   pub fn run_setup(args: &SetupArgs) -> Result<()> {
       Err(CodeGraphError::Other("setup: not yet implemented".into()))
   }
   ```
7. Update dispatch in `lib.rs`:
   ```rust
   Commands::Setup(args) => commands::setup::run_setup(args),
   ```
8. `cargo test -p cli` passes (all arg parsing tests)

---

## Wave 1 — Core Utilities

### T02: Settings JSON management + hook definitions
**AC coverage:** AC4, AC13 (partially AC1, AC3, AC8)
**Files:** `crates/cli/src/commands/setup.rs`
**Depends on:** T01

1. Write unit tests first:
   - `read_settings_returns_empty_for_missing_file`: missing path → `Value::Object({})`
   - `read_settings_parses_existing_json`: valid JSON with existing keys → preserves all
   - `read_settings_errors_on_invalid_json`: broken JSON → clear error (AC13)
   - `write_settings_creates_parent_dirs`: write to non-existent parent → creates it
   - `write_settings_preserves_key_order`: write object with known order → same order on read-back
   - `is_code_graph_hook_identifies_our_hooks`: entry with `"code-graph"` in command → true
   - `is_code_graph_hook_ignores_other_hooks`: entry without `"code-graph"` → false
   - `hook_definitions_have_correct_structure`: SessionStart and PostToolUse templates match spec
2. Implement `read_settings(path: &Path) -> Result<serde_json::Value>`:
   - File doesn't exist → `Ok(json!({}))`
   - File exists, valid JSON → parse into Value
   - File exists, invalid JSON → `Err(CodeGraphError::Other("..."))`  (AC13)
3. Implement `write_settings(path: &Path, value: &serde_json::Value) -> Result<()>`:
   - `fs::create_dir_all` on parent directory
   - `serde_json::to_string_pretty` + trailing newline
4. Implement `is_code_graph_hook(entry: &Value) -> bool`:
   - Check if any hook in entry's `hooks` array has `command` containing `"code-graph"`
5. Implement `session_start_hook() -> Value` and `post_tool_use_hook() -> Value`:
   - Return exact JSON structures from spec:
     - SessionStart: matcher `"startup"`, command `"code-graph index --incremental 2>/dev/null || true"`, timeout 120
     - PostToolUse: matcher `"Edit|Write"`, command with `jq` extraction, timeout 15
6. `cargo test -p cli -- setup` passes

### T03: Path resolution + .gitignore management + binary detection
**AC coverage:** AC2, AC5, AC9, AC15
**Files:** `crates/cli/src/commands/setup.rs`
**Depends on:** T01

1. Write unit tests first:
   - `resolve_settings_path_local`: no --global → `<root>/.claude/settings.json`
   - `resolve_settings_path_global`: --global → `$HOME/.claude/settings.json`
   - `ensure_gitignore_creates_file`: no .gitignore → creates with `.code-graph/` entry
   - `ensure_gitignore_appends`: existing .gitignore without entry → appends
   - `ensure_gitignore_idempotent`: already has `.code-graph/` → no duplicate
   - `ensure_gitignore_handles_no_trailing_newline`: file without newline → adds newline before entry
   - `remove_gitignore_entry_removes_line_and_comment`: both `.code-graph/` and `# Code Graph data` removed
   - `remove_gitignore_entry_noop_when_absent`: entry not present → file unchanged
   - `find_on_path_returns_some_for_existing_binary`: `which ls` → Some
   - `find_on_path_returns_none_for_nonexistent`: `which nonexistent_xyz` → None
2. Implement `resolve_settings_path(project_root: Option<&Path>, global: bool) -> Result<PathBuf>`:
   - `--global` → `$HOME/.claude/settings.json`
   - default → `project_root/.claude/settings.json` (error if no project root)
3. Implement `ensure_gitignore_entry(project_root: &Path) -> Result<bool>`:
   - Read `.gitignore` (or default to empty string if missing)
   - Check if `.code-graph/` line exists → return false (already present)
   - Append `\n# Code Graph data\n.code-graph/\n` (ensure newline before if needed)
   - Return true (added)
4. Implement `remove_gitignore_entry(project_root: &Path) -> Result<bool>`:
   - Read `.gitignore`
   - Filter out lines matching `.code-graph/` or `# Code Graph data`
   - Write back, return true if any lines removed
5. Implement `find_on_path(binary: &str) -> Option<PathBuf>`:
   - `Command::new("which").arg(binary).output()` → parse stdout
6. `cargo test -p cli -- setup` passes

---

## Wave 2 — Command Modes

### T04: Install mode
**AC coverage:** AC1, AC2, AC3, AC5, AC12, AC14, AC15
**Files:** `crates/cli/src/commands/setup.rs`
**Depends on:** T02, T03

1. Write unit tests first:
   - `install_creates_hooks_in_empty_settings`: empty settings → both hooks created with correct structure (AC1)
   - `install_preserves_existing_settings`: settings with env/permissions → hooks added, rest untouched (AC4)
   - `install_preserves_existing_non_codegraph_hooks`: settings with other hooks → code-graph hooks added alongside
   - `install_idempotent_no_duplicates`: install twice → exactly one code-graph entry per event (AC3)
   - `install_updates_outdated_hooks`: existing code-graph hook with old command → updated in place (AC3)
   - `install_adds_gitignore_entry`: .gitignore managed after hook install (AC5)
   - `install_unknown_platform_errors`: platform "cursor" → error with supported list (AC12)
2. Implement `run_install(args: &SetupArgs, project_root: Option<&Path>) -> Result<()>`:
   - Validate platform is `"claude"` (AC12)
   - Resolve settings path
   - Read existing settings
   - For each hook definition (SessionStart, PostToolUse):
     - Navigate to `hooks.<event>` array (create intermediate objects if absent)
     - Scan for existing code-graph entry via `is_code_graph_hook`
     - If found → replace entry in place (idempotent update)
     - If not found → append entry
   - Write settings back
   - Manage .gitignore: skip if `--global` and no project root (AC15), otherwise `ensure_gitignore_entry`
   - Check `jq` availability → print warning if missing (AC14)
   - Print summary: "Installed 2 hooks to <path>"
3. `cargo test -p cli -- setup` passes

### T05: Check mode
**AC coverage:** AC6, AC7
**Files:** `crates/cli/src/commands/setup.rs`
**Depends on:** T02, T03

1. Write unit tests first:
   - `check_all_installed_reports_ok`: both hooks present with correct commands → all "installed", returns Ok (AC7 exit 0)
   - `check_missing_hooks_reports_missing`: no hooks → "missing" for each, returns Err (AC7 exit 1)
   - `check_outdated_hook_reports_outdated`: hook present but command differs → "outdated", returns Err
   - `check_partial_install`: only SessionStart present → mixed status, returns Err
2. Implement `HookStatus` enum: `Installed`, `Outdated`, `Missing`
3. Implement `check_hook_status(settings: &Value, event: &str, expected: &Value) -> HookStatus`:
   - Find code-graph entry in `hooks.<event>` array
   - Compare command string against expected → Installed, Outdated, or Missing
4. Implement `run_check(args: &SetupArgs, project_root: Option<&Path>) -> Result<()>`:
   - Find `code-graph` binary via `find_on_path`
   - Find `jq` binary via `find_on_path`
   - Resolve and read settings file
   - Check each hook status
   - Print report (matching spec format):
     ```
     code-graph binary: /usr/local/bin/code-graph
     jq: /usr/bin/jq
     settings: .claude/settings.json
     SessionStart hook: installed
     PostToolUse hook: installed
     Status: all hooks installed
     ```
   - Return `Ok(())` if all installed, `Err(...)` if any missing/outdated (AC7)
5. `cargo test -p cli -- setup` passes

### T06: Remove mode
**AC coverage:** AC8, AC9, AC10, AC11
**Files:** `crates/cli/src/commands/setup.rs`
**Depends on:** T02, T03

1. Write unit tests first:
   - `remove_filters_code_graph_hooks`: settings with code-graph + other hooks → only code-graph removed (AC8)
   - `remove_cleans_empty_event_arrays`: after removal, empty event array → event key removed
   - `remove_cleans_empty_hooks_object`: all events empty → `hooks` key removed
   - `remove_noop_when_no_hooks`: no code-graph hooks → no error (AC11)
   - `remove_with_clean_removes_gitignore`: --clean → .gitignore entry removed (AC9)
   - `remove_with_purge_deletes_data_dir`: --purge → `.code-graph/` directory deleted (AC10)
   - `remove_preserves_other_settings`: env/permissions/model untouched
2. Implement `run_remove(args: &SetupArgs, project_root: Option<&Path>) -> Result<()>`:
   - Resolve and read settings
   - For each event key in hooks object:
     - Filter out entries where `is_code_graph_hook` is true
     - If event array becomes empty → remove event key
   - If hooks object becomes empty → remove `hooks` key
   - Write settings back
   - If `--clean` or `--purge` → `remove_gitignore_entry` (AC9)
   - If `--purge` → `fs::remove_dir_all` on `.code-graph/` directory (AC10)
   - Print summary
3. `cargo test -p cli -- setup` passes

---

## Wave 3 — Dispatch + Integration

### T07: Wire run_setup dispatcher + integration tests + clippy
**AC coverage:** AC16, AC17
**Files:** `crates/cli/src/commands/setup.rs`
**Depends on:** T04, T05, T06

1. Replace stub `run_setup` with full dispatcher:
   ```rust
   pub fn run_setup(args: &SetupArgs) -> Result<()> {
       let project_root = find_project_root_optional();
       if args.check { return run_check(args, project_root.as_deref()); }
       if args.remove { return run_remove(args, project_root.as_deref()); }
       // Install mode — platform required
       let platform = args.platform.as_deref()
           .ok_or_else(|| CodeGraphError::Other(
               "platform required: code-graph setup claude".into()
           ))?;
       if platform != "claude" {
           return Err(CodeGraphError::Other(
               format!("Unsupported platform '{platform}'. Supported: claude")
           ));
       }
       run_install(args, project_root.as_deref())
   }
   ```
   Note: `find_project_root_optional` wraps `find_project_root` to return `Option<PathBuf>` instead of erroring — `--global` outside a git project should work for check/remove/install.
2. Write integration tests:
   - `full_install_check_remove_cycle`: temp dir with `.git` → install → verify settings JSON → check reports installed → remove → check reports missing
   - `install_on_existing_settings_preserves_other_hooks`: pre-populate settings with non-code-graph hook → install → both present
   - `idempotent_install_no_duplicates`: install twice → exactly one code-graph entry per event
   - `purge_deletes_data_directory`: create `.code-graph/` dir → --remove --purge → directory gone
3. `cargo test --workspace` passes (AC16)
4. `cargo clippy --workspace -- -Dwarnings` passes (AC17)

---

## Task Dependency Graph

```
T01 (CLI wiring) ────────────────┬──► T02 (settings JSON management)
                                 │
                                 └──► T03 (path/gitignore/binary helpers)

T02 + T03 ───────────────────────┬──► T04 (install mode)
                                 │
                                 ├──► T05 (check mode)
                                 │
                                 └──► T06 (remove mode)

T04 + T05 + T06 ────────────────────► T07 (dispatcher + integration tests)
```

## Wave Summary

| Wave | Tasks | Parallelism |
|------|-------|-------------|
| **0** | T01 | Single task (structural prerequisite) |
| **1** | T02, T03 | Parallel (JSON management vs filesystem helpers) |
| **2** | T04, T05, T06 | Parallel (independent command modes) |
| **3** | T07 | Sequential (composition + integration) |

## Complexity Estimate

| Task | Size | Notes |
|------|------|-------|
| T01 | S | Cargo.toml + SetupArgs + enum change + test update + stub, ~50 lines |
| T02 | M | Read/write settings + hook identification + definitions + tests, ~120 lines |
| T03 | M | Path resolution + gitignore mgmt + binary detection + tests, ~100 lines |
| T04 | M-L | Install flow + idempotency + all edge cases + tests, ~120 lines |
| T05 | M | Check flow + status reporting + tests, ~80 lines |
| T06 | M | Remove flow + clean/purge + tests, ~80 lines |
| T07 | M | Dispatcher + integration tests + clippy, ~100 lines |

**Total estimated:** ~650 lines of new/modified code + tests

## AC Traceability Matrix

| AC | Task | Verified By |
|----|------|-------------|
| AC1 | T04 | Test: install creates both hooks with correct commands |
| AC2 | T03, T04 | Test: --global resolves to ~/.claude/settings.json |
| AC3 | T04 | Test: idempotent install, update in place |
| AC4 | T02, T04 | Test: existing settings preserved after install |
| AC5 | T03, T04 | Test: .gitignore entry added |
| AC6 | T05 | Test: check reports per-hook status, binary, jq |
| AC7 | T05 | Test: exit code based on hook status |
| AC8 | T06 | Test: remove filters only code-graph hooks |
| AC9 | T03, T06 | Test: --clean removes .gitignore entry |
| AC10 | T06 | Test: --purge deletes .code-graph/ directory |
| AC11 | T06 | Test: remove is no-op when already absent |
| AC12 | T04, T07 | Test: unknown platform → error |
| AC13 | T02 | Test: invalid JSON → clear error |
| AC14 | T04 | Test: missing jq → warning |
| AC15 | T03, T04 | Test: --global outside git project → skip .gitignore |
| AC16 | T07 | `cargo test --workspace` |
| AC17 | T07 | `cargo clippy --workspace -- -Dwarnings` |
