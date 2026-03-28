# Spec — M01-S08: Agent Integration

## Problem

After S07, the graph stays fresh via watch daemon and incremental indexing, but there's no automated integration with AI agents. Users must manually run `code-graph index` or start the daemon. Claude Code (the primary target agent) has a hooks system that can trigger indexing automatically on session start and after file edits — but there's no `setup` command to install/verify/remove these hooks. S08 bridges this gap with a one-command setup experience.

## Approach

### Command Structure

```
code-graph setup claude [--global]     # install hooks (default: project-local)
code-graph setup --check [--global]    # verify installation
code-graph setup --remove [--global] [--clean] [--purge]  # uninstall
```

**CLI Args:**

```rust
#[derive(Args)]
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

**Dispatch logic:**
- `--check` → verify mode
- `--remove` → uninstall mode
- `--clean` or `--purge` without `--remove` → Clap validation error (enforced by `requires = "remove"`)
- Otherwise → install mode (requires `platform` argument)
- `platform` must be `"claude"` — error on unknown platform

### Hook Definitions

Two Claude Code hooks installed into `settings.json`. Complete target structure:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup",
        "hooks": [{
          "type": "command",
          "command": "code-graph index --incremental 2>/dev/null || true",
          "timeout": 120
        }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [{
          "type": "command",
          "command": "code-graph index --incremental --files \"$(cat | jq -r '.tool_input.file_path // empty')\" 2>/dev/null || true",
          "timeout": 15
        }]
      }
    ]
  }
}
```

**Hook 1 — SessionStart** (event: `SessionStart`, matcher: `startup`):
Runs incremental index on session startup. Timeout is 120s to accommodate first-time indexing on large repos. The `|| true` prevents non-zero exit from blocking session start. Silently exits 0 if no project detected. Users with very large repos should run `code-graph index` manually first for initial indexing.

**Hook 2 — PostToolUse** (event: `PostToolUse`, matcher: `Edit|Write`):
Extracts `file_path` from stdin JSON via `jq`. Re-indexes only the changed file. Timeout is 15s (single-file re-index is fast). If `jq` is missing or extraction yields an empty string, `code-graph index --incremental --files ""` is a no-op (empty file list = nothing to index, exit 0).

**R7 deviation — no PreCommit hook.** R7 specifies "Claude Code hooks: SessionStart, PostToolUse, PreCommit." Claude Code's hook system has no `PreCommit` event. SessionStart + PostToolUse already keep the graph fresh — every file edit triggers re-indexing, so the graph is current before any commit. The pre-commit indexing intent from R7 is deferred to lefthook in S09 (CI/CD slice) where lefthook is already planned per R9, providing the same guarantee for both AI agents and human developers.

### Settings JSON Management

**Target file resolution:**
- `--global` → `~/.claude/settings.json`
- Default → `<project_root>/.claude/settings.json`

**Read/write strategy:**
1. Read existing settings.json as `serde_json::Value` (or start with `{}` if absent)
2. Navigate to `hooks.<event>` arrays
3. Identify code-graph hooks by checking if `command` field contains `"code-graph"`
4. On install: update existing hook in place or append new entry
5. On remove: filter out entries whose command contains `"code-graph"`
6. Write back with `serde_json::to_string_pretty` — only the `hooks` subtree is modified, all other settings (env, permissions, model, plugins, etc.) are preserved

**Idempotency:** Before inserting, scan existing hook entries. If a hook with a command containing `code-graph` already exists in that event's array, update it in place rather than duplicating.

### .gitignore Management

**On install (both default and `--global`):**
1. Read `<project_root>/.gitignore` (or create if absent)
2. Check if `.code-graph/` line already exists (exact line match)
3. If missing, append `\n# Code Graph data\n.code-graph/\n`
4. Idempotent — running twice doesn't duplicate
5. Note: `.gitignore` is always modified at project root, even with `--global`. The hooks target differs but the data directory is always per-project.

**On remove with `--clean`:**
1. Remove `.code-graph/` line and its `# Code Graph data` comment header from `.gitignore`

**On remove with `--purge`:**
1. Everything `--clean` does, plus `rm -rf <project_root>/.code-graph/`

### Check Command

**Verification steps:**
1. **Binary on PATH** — verify `code-graph` binary is findable
2. **Settings file exists** — read target settings.json
3. **Per-hook status** — for each expected hook (SessionStart, PostToolUse): `installed`, `outdated` (found but command differs), or `missing`
4. **jq dependency** — verify `jq` is on PATH (required for PostToolUse hook)

**Output (compact default):**
```
code-graph binary: /usr/local/bin/code-graph
jq: /usr/bin/jq
settings: .claude/settings.json
SessionStart hook: installed
PostToolUse hook: installed
Status: all hooks installed
```

**Exit code:** 0 if all hooks present, 1 if any missing/outdated.

## Acceptance Criteria

### Install
- **AC1**: `code-graph setup claude` creates `.claude/settings.json` in project root with SessionStart and PostToolUse hook entries. Commands contain `code-graph index --incremental` (SessionStart) and file-specific incremental index (PostToolUse).
- **AC2**: `code-graph setup claude --global` writes hooks to `~/.claude/settings.json` instead.
- **AC3**: Install is idempotent — running twice does not duplicate hook entries. Existing hooks are updated in place if command string changed.
- **AC4**: Existing settings (env, permissions, model, plugins, etc.) are preserved — only the `hooks` subtree is modified.
- **AC5**: `.code-graph/` is appended to project `.gitignore` if not already present.

### Check
- **AC6**: `code-graph setup --check` reports status of each hook (installed/outdated/missing), binary presence, jq presence, and settings file location.
- **AC7**: Exit code 0 if all hooks installed, 1 if any missing or outdated.

### Remove
- **AC8**: `code-graph setup --remove` removes all hook entries whose command contains `code-graph` from the target settings.json. Other hooks and settings are untouched.
- **AC9**: `--clean` also removes `.code-graph/` from `.gitignore`.
- **AC10**: `--purge` also deletes the `.code-graph/` directory entirely.
- **AC11**: Remove is safe to run when hooks are already absent (no-op, exit 0).

### Error Handling
- **AC12**: Unknown platform (not `"claude"`) prints error with supported platforms list and exits 1.
- **AC13**: If settings.json has invalid JSON, print clear error and exit 1 (don't corrupt the file).
- **AC14**: Install warns (does not error) if `jq` is not found on PATH: "Warning: jq not found — PostToolUse hook will not extract file paths. Install jq for per-file incremental indexing."
- **AC15**: `--global` outside a git project skips `.gitignore` management with info message, does not error.

### Quality
- **AC16**: `cargo test --workspace` passes.
- **AC17**: `cargo clippy --workspace -- -Dwarnings` passes.

## Non-Goals

- **No MCP server tools** — pure CLI hooks only for v0.1. MCP integration deferred.
- **No PreCommit hook** — Claude Code has no PreCommit event. Graph is kept fresh by SessionStart + PostToolUse. Pre-commit indexing deferred to lefthook in S09.
- **No multi-platform support** — only `claude` platform. `cursor`, `windsurf`, etc. deferred.
- **No hook auto-update** — if the hook command format changes, user must re-run `setup claude`. No version negotiation.
- **No Windows support** — path handling uses Unix conventions. Windows deferred.
- **No `eval` command** — deferred to S09.

## Design Notes

- **Hook identification by command prefix.** Claude Code hooks have no metadata or source tag field. We identify code-graph hooks by checking if the `command` string contains `"code-graph"`. This is simple and reliable — the binary name is unique enough.
- **`jq` dependency for PostToolUse.** The PostToolUse hook needs to extract `file_path` from stdin JSON. `jq` is the standard tool for this. The `--check` command verifies its presence. If `jq` is missing, SessionStart hook still works — only per-file incremental is degraded.
- **`|| true` in hook commands.** Hooks that exit non-zero can block Claude Code operations. `|| true` ensures graceful degradation — a failed index doesn't prevent the agent from editing files.
- **Project-local default.** Hooks in `.claude/settings.json` (project root) only affect that project. This avoids polluting global settings when users work on multiple projects, some without code-graph. `--global` is available for users who want universal coverage.
- **`--files ""` is a no-op.** If `jq` fails or extracts an empty string, `code-graph index --incremental --files ""` receives an empty file list and exits 0 without re-indexing anything. The index command must handle `--files ""` gracefully: if the file list contains only empty strings, treat as empty and return zero stats. This is the intentional degradation path — the `|| true` is belt-and-suspenders on top.
- **SessionStart timeout is 120s.** Initial incremental index on a large repo (first session after `code-graph index`) needs to check all files via git status + hash comparison. 120s accommodates repos with thousands of files. For truly large codebases, users should run `code-graph index` manually first.
- **Hooks format is based on Claude Code docs (March 2025).** The JSON structure (event key → array of `{matcher, hooks}` objects) matches the documented Claude Code hooks schema. If the format changes, `--check` will report hooks as missing/outdated. No version negotiation — user re-runs `setup claude`.
- **`serde_json` `preserve_order` feature required.** Without this, keys in settings.json are alphabetically sorted on write-back, creating noisy diffs. Enable `preserve_order` in `cli/Cargo.toml` to maintain insertion order.
- **Enum variant upgrade.** The existing `Commands::Setup` unit variant in `commands/mod.rs` must be changed to `Setup(SetupArgs)`. The existing `all_subcommands_parse` test must be updated.
- **`--global` without a project root.** If `--global` is used outside a git project, skip `.gitignore` management (no project root to modify). Print info message: "Not inside a git project — skipping .gitignore."
- **`--purge` safety.** No confirmation prompt needed — the data is always reconstructible via `code-graph index`. The `--purge` flag already requires `--remove`, which is sufficient intent signal.

## Testing Strategy

### Unit Tests
- Settings JSON read/write: empty file, existing hooks, existing code-graph hooks (idempotency), corrupted JSON, missing file creation
- Hook identification: command prefix matching, distinguishing code-graph hooks from other hooks
- .gitignore management: append when missing, skip when present, clean removal, empty file edge case
- Check output: all installed, some missing, all missing, outdated hooks
- CLI arg parsing: `setup claude`, `setup --check`, `setup --remove --clean --purge`, `setup unknown-platform`

### Integration Tests
- Full install → check → remove cycle in temp directory
- Install on existing settings.json with other hooks (verify preservation)
- `--global` flag targets home directory settings
- `--purge` deletes `.code-graph/` directory
- Idempotent install: run twice, verify no duplicates
