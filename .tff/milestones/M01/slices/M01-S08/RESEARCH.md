# Research — M01-S08: Agent Integration

## 1. Dependency Research

### 1.1 `serde_json` (Settings JSON Management)

**Current state:** `serde_json = "1"` already in `cli/Cargo.toml` (line 15).

**`preserve_order` feature required.** Without it, `serde_json::to_string_pretty` alphabetically sorts keys on write-back, creating noisy diffs in users' settings files. The `preserve_order` feature enables `serde_json::Map` to use `IndexMap` internally, maintaining insertion order.

**Change needed:**
```toml
# cli/Cargo.toml
serde_json = { version = "1", features = ["preserve_order"] }
```

**API for settings manipulation:**
- `serde_json::from_str::<Value>(&content)` — parse existing settings
- `value["hooks"]["SessionStart"]` — navigate with indexing (returns `Value::Null` for missing keys)
- `value.as_object_mut()` — get mutable `Map` for insertion/removal
- `serde_json::to_string_pretty(&value)` — write back with formatting

**Key pattern for safe navigation + mutation:**
```rust
let hooks = root.as_object_mut()
    .unwrap()
    .entry("hooks")
    .or_insert_with(|| json!({}));
let event_array = hooks.as_object_mut()
    .unwrap()
    .entry("SessionStart")
    .or_insert_with(|| json!([]));
```

This creates intermediate objects on demand without clobbering existing data.

### 1.2 Home Directory Resolution

**No new dependency needed.** `std::env::var("HOME")` is already used in the codebase (`project.rs:26`). For `--global`, we need `~/.claude/settings.json`:

```rust
fn home_dir() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| CodeGraphError::Other("HOME not set".into()))
}
```

The `dirs` crate would be more portable, but the spec explicitly states "No Windows support", so `$HOME` is sufficient and avoids a new dependency.

### 1.3 Binary / Tool Detection on PATH

**For `--check` command:** Need to verify `code-graph` and `jq` are on PATH.

**Approach: `std::process::Command` with `which`.**
```rust
fn find_on_path(binary: &str) -> Option<PathBuf> {
    std::process::Command::new("which")
        .arg(binary)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
}
```

No `which` crate needed — shelling out to `which` is simpler and avoids a dependency. This is macOS/Linux only, which aligns with the spec's "No Windows support" non-goal.

### 1.4 Dependency Summary

**No new crate dependencies.** Only changes to existing:
```toml
# cli/Cargo.toml — change from:
serde_json = "1"
# to:
serde_json = { version = "1", features = ["preserve_order"] }
```

All other functionality uses `std` and existing deps (`serde_json`, `clap`).

---

## 2. Integration Point Analysis

### 2.1 What Already Exists (No Changes Needed)

| Component | Location | Why It's Ready |
|-----------|----------|---------------|
| `find_project_root()` | `project.rs:6-21` | Walks up to find `.git` dir — used for settings file path |
| `ensure_data_dir()` | `project.rs:29-34` | Creates `.code-graph/` dir — not needed for setup but available |
| `serde_json` dependency | `cli/Cargo.toml:15` | Already in CLI crate (needs `preserve_order` feature) |
| `serde` with `derive` | `cli/Cargo.toml:14` | Already available for any Serialize/Deserialize structs |
| `clap` with `derive` | `cli/Cargo.toml:11` | Already available for `SetupArgs` struct |
| `tempfile` dev-dep | `cli/Cargo.toml:22` | Already available for test isolation |
| `CodeGraphError::Other` | `domain/src/error.rs` | Catch-all error variant for setup-specific errors |
| `stubs::not_implemented()` | `commands/stubs.rs:3-7` | Currently handles `Setup` — will be replaced |
| `$HOME` usage pattern | `project.rs:26` | Precedent for home dir resolution in codebase |

### 2.2 What Needs to Change

**1. CLI args (`crates/cli/src/commands/mod.rs`):**
- Line 62: Change `Setup,` (unit variant) to `Setup(SetupArgs)`
- Add `SetupArgs` struct as specified in SPEC (platform, global, check, remove, clean, purge)
- Line 213: Update `all_subcommands_parse` test — `vec!["code-graph", "setup"]` must become `vec!["code-graph", "setup", "claude"]` or test `--check`/`--remove` variants

**2. CLI dispatch (`crates/cli/src/lib.rs`):**
- Line 26: Change `Commands::Setup => commands::stubs::not_implemented("setup")` to `Commands::Setup(args) => commands::setup::run_setup(args)`
- No output_format needed — setup uses its own output

**3. Module registration (`crates/cli/src/commands/mod.rs`):**
- Add `pub mod setup;` to module declarations (line 1-12 area)

**4. `serde_json` feature (`crates/cli/Cargo.toml`):**
- Line 15: Change `serde_json = "1"` to `serde_json = { version = "1", features = ["preserve_order"] }`

### 2.3 New Files to Create

| File | Purpose |
|------|---------|
| `crates/cli/src/commands/setup.rs` | CLI command handler — dispatch to install/check/remove |
| `crates/cli/src/commands/setup/mod.rs` | Alternative: module dir if file gets large |

**Decision: Single file `setup.rs`.** The logic (install, check, remove, gitignore) totals ~300-400 lines. A single file keeps it simple and matches the pattern of other commands (e.g., `watch.rs`, `index.rs`). If it grows beyond 500 lines, extract into a module directory.

### 2.4 Confirmed Format: Claude Code Settings JSON

**Verified against live `~/.claude/settings.json`.** The hooks structure exactly matches the SPEC:

```json
{
  "hooks": {
    "<EventName>": [
      {
        "matcher": "<pattern>",
        "hooks": [{
          "type": "command",
          "command": "<shell command>",
          "timeout": <seconds>
        }]
      }
    ]
  }
}
```

The live file has a `PreToolUse` hook with this exact structure. The SPEC's `SessionStart` and `PostToolUse` hooks follow the same pattern. Format is confirmed correct.

**Other top-level keys in live settings:** `env`, `permissions`, `model`, `hooks`, `statusLine`, `enabledPlugins`, etc. The setup command must preserve ALL of these — only modify the `hooks` subtree.

---

## 3. Architecture Decisions

### 3.1 Settings JSON Read/Write Strategy

**Safe mutation algorithm:**

```
read_settings(path) → serde_json::Value
  │
  ├─ File doesn't exist → start with Value::Object({})
  ├─ File exists, valid JSON → parse into Value
  └─ File exists, invalid JSON → error out (AC13)
  │
  mutate hooks subtree
  │
  write_settings(path, value)
  ├─ Ensure parent directory exists (mkdir_all)
  └─ serde_json::to_string_pretty + write_all
```

**Key safety properties:**
1. Parse entire file as `Value` (not a typed struct) — preserves unknown keys
2. Only navigate into `hooks.<event>` — never touch other top-level keys
3. Write back with `to_string_pretty` — human-readable, key order preserved (via `preserve_order`)
4. Atomic-ish write: write to file directly (no temp file + rename needed for a settings file)

### 3.2 Hook Identification Strategy

**Identify code-graph hooks by command substring match:**
```rust
fn is_code_graph_hook(entry: &Value) -> bool {
    entry["hooks"]
        .as_array()
        .map(|hooks| hooks.iter().any(|h| {
            h["command"].as_str().map_or(false, |c| c.contains("code-graph"))
        }))
        .unwrap_or(false)
}
```

This correctly identifies code-graph hooks regardless of the exact command format (handles future command changes).

### 3.3 Install Flow

```
code-graph setup claude [--global]
  │
  ├─ Resolve settings path:
  │   ├─ --global → ~/.claude/settings.json
  │   └─ default → <project_root>/.claude/settings.json
  │
  ├─ Read existing settings (or start with {})
  │
  ├─ For each hook (SessionStart, PostToolUse):
  │   ├─ Navigate to hooks.<event> array
  │   ├─ Scan for existing code-graph entry
  │   ├─ If found → update command in place (idempotency)
  │   └─ If not found → append new entry
  │
  ├─ Write settings back
  │
  ├─ Manage .gitignore (unless --global outside git project):
  │   ├─ Read <project_root>/.gitignore
  │   ├─ Check if ".code-graph/" line exists
  │   └─ If missing → append "# Code Graph data\n.code-graph/\n"
  │
  ├─ Check jq availability → warn if missing (AC14)
  │
  └─ Print summary:
      "Installed 2 hooks to .claude/settings.json"
      "Added .code-graph/ to .gitignore"
```

### 3.4 Remove Flow

```
code-graph setup --remove [--clean] [--purge] [--global]
  │
  ├─ Resolve settings path (same as install)
  │
  ├─ Read existing settings
  │
  ├─ For each event (SessionStart, PostToolUse):
  │   ├─ Filter out entries where any hook command contains "code-graph"
  │   └─ If event array becomes empty → remove the event key
  │
  ├─ If hooks object becomes empty → remove "hooks" key entirely
  │
  ├─ Write settings back
  │
  ├─ If --clean or --purge:
  │   └─ Remove ".code-graph/" + comment from .gitignore
  │
  ├─ If --purge:
  │   └─ rm_rf <project_root>/.code-graph/
  │
  └─ Print summary
```

### 3.5 Check Flow

```
code-graph setup --check [--global]
  │
  ├─ Find code-graph binary → which code-graph
  ├─ Find jq binary → which jq
  ├─ Read settings file
  │
  ├─ For each expected hook (SessionStart, PostToolUse):
  │   ├─ Find code-graph entry in event array
  │   ├─ Compare command against expected command
  │   └─ Status: installed | outdated | missing
  │
  └─ Print report + exit code (0 = all installed, 1 = any missing/outdated)
```

### 3.6 .gitignore Management

**Append pattern:**
```rust
fn ensure_gitignore_entry(project_root: &Path) -> Result<()> {
    let gitignore = project_root.join(".gitignore");
    let content = fs::read_to_string(&gitignore).unwrap_or_default();
    if content.lines().any(|l| l.trim() == ".code-graph/") {
        return Ok(());  // already present
    }
    let mut file = fs::OpenOptions::new()
        .create(true).append(true).open(&gitignore)?;
    // Ensure newline before our entry if file doesn't end with one
    if !content.is_empty() && !content.ends_with('\n') {
        write!(file, "\n")?;
    }
    writeln!(file, "\n# Code Graph data\n.code-graph/")?;
    Ok(())
}
```

**Remove pattern (for `--clean`):**
```rust
fn remove_gitignore_entry(project_root: &Path) -> Result<()> {
    let gitignore = project_root.join(".gitignore");
    let content = fs::read_to_string(&gitignore)?;
    let filtered: Vec<&str> = content.lines()
        .filter(|l| l.trim() != ".code-graph/" && l.trim() != "# Code Graph data")
        .collect();
    fs::write(&gitignore, filtered.join("\n") + "\n")?;
    Ok(())
}
```

### 3.7 Platform Argument Handling

```
code-graph setup claude       → install (platform = "claude")
code-graph setup --check      → check (platform = None, ok)
code-graph setup --remove     → remove (platform = None, ok)
code-graph setup unknown      → error: "Unsupported platform 'unknown'. Supported: claude"
code-graph setup              → clap error (platform required when not --check/--remove)
```

**Dispatch in `run_setup()`:**
```rust
if args.check { return run_check(args); }
if args.remove { return run_remove(args); }
// Install mode — platform is required
let platform = args.platform.as_deref()
    .ok_or_else(|| CodeGraphError::Other("platform required: code-graph setup claude".into()))?;
if platform != "claude" {
    return Err(CodeGraphError::Other(
        format!("Unsupported platform '{platform}'. Supported: claude")
    ));
}
run_install(args)
```

---

## 4. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `preserve_order` changes existing serde_json behavior in tests | Low | Low | Only affects Map iteration order, not correctness. Run full test suite. |
| User has non-JSON content in settings file (comments, trailing commas) | Medium | Medium | AC13 covers this — print clear error. Claude Code itself generates valid JSON. |
| Settings file permissions prevent write | Low | Medium | Standard io::Error propagation. Clear error message. |
| `~/.claude/` directory doesn't exist for `--global` | Medium | Low | `fs::create_dir_all` on parent dir before writing. |
| `which` command not available | Very Low | Low | `which` is a POSIX standard. If missing, `--check` degrades gracefully. |
| Hook command format changes in future Claude Code versions | Low | Low | `--check` reports "outdated". User re-runs `setup claude`. No auto-update per spec. |
| `.gitignore` has unusual encoding (BOM, non-UTF8) | Very Low | Low | `read_to_string` handles UTF-8. BOM edge case is rare and out of scope. |
| Concurrent writes to settings.json (user editing while setup runs) | Very Low | Low | Not atomic — acceptable for a CLI tool run manually. |

---

## 5. Open Questions Resolved

| Question | Resolution |
|----------|-----------|
| New crate or module in CLI? | **Module in CLI crate** — `commands/setup.rs`. No new crate needed (pure file I/O, no domain logic). |
| Home directory resolution | **`std::env::var("HOME")`** — already used in codebase, no new dependency. |
| Binary detection for `--check` | **Shell out to `which`** — no `which` crate dependency needed. |
| `preserve_order` feature needed? | **Yes** — confirmed that without it, serde_json reorders keys alphabetically, creating noisy diffs. |
| Claude Code hooks format correct? | **Confirmed** — live `~/.claude/settings.json` uses exact same structure as SPEC. |
| `jq` available on dev machine? | **Yes** — at `/usr/bin/jq`. |
| Test strategy for settings JSON manipulation? | **Unit tests with tempdir** — create temp settings files, run install/check/remove, verify JSON output. Follow existing pattern (e.g., `config.rs` tests). |
| What about the empty hooks array cleanup? | **Remove empty event arrays and empty hooks object** — prevents leftover `"hooks": {"SessionStart": []}` after removal. |
