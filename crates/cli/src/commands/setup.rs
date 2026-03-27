use std::path::{Path, PathBuf};
use std::fs;
use serde_json::{json, Value};
use domain::error::{CodeGraphError, Result};

use super::SetupArgs;
use super::setup_helpers::{
    resolve_settings_path, ensure_gitignore_entry, remove_gitignore_entry, find_on_path,
};

// ── Settings JSON management (T02) ──────────────────────────────────────────

pub(super) fn read_settings(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| {
        CodeGraphError::FileSystem { path: path.to_path_buf(), source: e }
    })?;
    serde_json::from_str(&content).map_err(|e| {
        CodeGraphError::Other(format!("Invalid JSON in {}: {}", path.display(), e))
    })
}

pub(super) fn write_settings(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CodeGraphError::FileSystem { path: parent.to_path_buf(), source: e }
        })?;
    }
    let mut content = serde_json::to_string_pretty(value).map_err(|e| {
        CodeGraphError::Other(format!("Failed to serialize settings: {}", e))
    })?;
    content.push('\n');
    fs::write(path, content).map_err(|e| {
        CodeGraphError::FileSystem { path: path.to_path_buf(), source: e }
    })
}

pub(super) fn is_code_graph_hook(entry: &Value) -> bool {
    if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
        hooks.iter().any(|hook| {
            hook.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.contains("code-graph"))
                .unwrap_or(false)
        })
    } else {
        false
    }
}

pub(super) fn session_start_hook() -> Value {
    json!({
        "matcher": "startup",
        "hooks": [
            {
                "type": "command",
                "command": "code-graph index --incremental 2>/dev/null || true",
                "timeout": 120
            }
        ]
    })
}

pub(super) fn post_tool_use_hook() -> Value {
    json!({
        "matcher": "Edit|Write",
        "hooks": [
            {
                "type": "command",
                "command": "code-graph index --incremental --files \"$(cat | jq -r '.tool_input.file_path // empty')\" 2>/dev/null || true",
                "timeout": 15
            }
        ]
    })
}

// ── Hook definitions mapping ────────────────────────────────────────────────

fn hook_definitions() -> Vec<(&'static str, Value)> {
    vec![
        ("SessionStart", session_start_hook()),
        ("PostToolUse", post_tool_use_hook()),
    ]
}

// ── Check mode (T05) ────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum HookStatus {
    Installed,
    Outdated,
    Missing,
}

impl std::fmt::Display for HookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookStatus::Installed => write!(f, "installed"),
            HookStatus::Outdated => write!(f, "outdated"),
            HookStatus::Missing => write!(f, "missing"),
        }
    }
}

fn expected_command(hook_def: &Value) -> Option<&str> {
    hook_def.get("hooks")
        .and_then(|h| h.as_array())
        .and_then(|a| a.first())
        .and_then(|h| h.get("command"))
        .and_then(|c| c.as_str())
}

fn check_hook_status(settings: &Value, event: &str, expected: &Value) -> HookStatus {
    let entries = match settings.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(|e| e.as_array())
    {
        Some(arr) => arr,
        None => return HookStatus::Missing,
    };

    for entry in entries {
        if is_code_graph_hook(entry) {
            // Found a code-graph hook — check if command matches
            let found_cmd = entry.get("hooks")
                .and_then(|h| h.as_array())
                .and_then(|a| a.first())
                .and_then(|h| h.get("command"))
                .and_then(|c| c.as_str());
            let expected_cmd = expected_command(expected);
            if found_cmd == expected_cmd {
                return HookStatus::Installed;
            } else {
                return HookStatus::Outdated;
            }
        }
    }

    HookStatus::Missing
}

fn run_check(args: &SetupArgs, project_root: Option<&Path>) -> Result<()> {
    let cg_binary = find_on_path("code-graph");
    let jq_binary = find_on_path("jq");
    let settings_path = resolve_settings_path(project_root, args.global)?;

    println!("code-graph binary: {}", match &cg_binary {
        Some(p) => p.display().to_string(),
        None => "not found".to_string(),
    });
    println!("jq: {}", match &jq_binary {
        Some(p) => p.display().to_string(),
        None => "not found".to_string(),
    });

    let rel_path = if args.global {
        format!("~/.claude/settings.json")
    } else {
        ".claude/settings.json".to_string()
    };
    println!("settings: {}", rel_path);

    let settings = read_settings(&settings_path)?;
    let defs = hook_definitions();
    let mut all_installed = true;

    for (event, expected) in &defs {
        let status = check_hook_status(&settings, event, expected);
        println!("{} hook: {}", event, status);
        if status != HookStatus::Installed {
            all_installed = false;
        }
    }

    if all_installed {
        println!("Status: all hooks installed");
        Ok(())
    } else {
        Err(CodeGraphError::Other("Some hooks are missing or outdated".into()))
    }
}

// ── Install mode (T04) ──────────────────────────────────────────────────────

fn run_install(args: &SetupArgs, project_root: Option<&Path>) -> Result<()> {
    let settings_path = resolve_settings_path(project_root, args.global)?;
    let mut settings = read_settings(&settings_path)?;

    // Ensure hooks object exists
    if settings.get("hooks").is_none() {
        settings.as_object_mut().unwrap().insert("hooks".to_string(), json!({}));
    }

    let defs = hook_definitions();
    for (event, hook_def) in &defs {
        let hooks_obj = settings.get_mut("hooks").unwrap().as_object_mut().unwrap();

        // Ensure event array exists
        if !hooks_obj.contains_key(*event) {
            hooks_obj.insert(event.to_string(), json!([]));
        }

        let event_arr = hooks_obj.get_mut(*event).unwrap().as_array_mut().unwrap();

        // Find existing code-graph entry
        let existing_idx = event_arr.iter().position(|e| is_code_graph_hook(e));

        match existing_idx {
            Some(idx) => {
                // Update in place (idempotent)
                event_arr[idx] = hook_def.clone();
            }
            None => {
                // Append new entry
                event_arr.push(hook_def.clone());
            }
        }
    }

    write_settings(&settings_path, &settings)?;

    // Manage .gitignore
    if let Some(root) = project_root {
        ensure_gitignore_entry(root)?;
    } else if args.global {
        println!("Not inside a git project — skipping .gitignore.");
    }

    // Check jq availability
    if find_on_path("jq").is_none() {
        println!("Warning: jq not found — PostToolUse hook will not extract file paths. Install jq for per-file incremental indexing.");
    }

    println!("Installed 2 hooks to {}", settings_path.display());
    Ok(())
}

// ── Remove mode (T06) ───────────────────────────────────────────────────────

fn run_remove(args: &SetupArgs, project_root: Option<&Path>) -> Result<()> {
    let settings_path = resolve_settings_path(project_root, args.global)?;
    let mut settings = read_settings(&settings_path)?;

    if let Some(hooks_obj) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let event_keys: Vec<String> = hooks_obj.keys().cloned().collect();
        for event_key in event_keys {
            if let Some(arr) = hooks_obj.get_mut(&event_key).and_then(|v| v.as_array_mut()) {
                arr.retain(|entry| !is_code_graph_hook(entry));
                if arr.is_empty() {
                    hooks_obj.remove(&event_key);
                }
            }
        }
        if hooks_obj.is_empty() {
            settings.as_object_mut().unwrap().remove("hooks");
        }
    }

    write_settings(&settings_path, &settings)?;

    // --clean or --purge → remove .gitignore entry
    if (args.clean || args.purge) {
        if let Some(root) = project_root {
            remove_gitignore_entry(root)?;
        }
    }

    // --purge → delete .code-graph/ directory
    if args.purge {
        if let Some(root) = project_root {
            let data_dir = root.join(".code-graph");
            if data_dir.is_dir() {
                fs::remove_dir_all(&data_dir).map_err(|e| {
                    CodeGraphError::FileSystem { path: data_dir, source: e }
                })?;
            }
        }
    }

    println!("Removed code-graph hooks from {}", settings_path.display());
    Ok(())
}

// ── Dispatcher (T07 stub — wired in T07) ────────────────────────────────────

pub fn run_setup(args: &SetupArgs) -> Result<()> {
    Err(CodeGraphError::Other("setup: not yet implemented".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    // ── T02 tests: settings JSON management ─────────────────────────────────

    #[test]
    fn read_settings_returns_empty_for_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let result = read_settings(&path).unwrap();
        assert!(result.is_object());
        assert_eq!(result.as_object().unwrap().len(), 0);
    }

    #[test]
    fn read_settings_parses_existing_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let content = r#"{"hooks": {"SessionStart": []}, "theme": "dark"}"#;
        fs::write(&path, content).unwrap();

        let result = read_settings(&path).unwrap();
        assert!(result.is_object());
        assert!(result.get("hooks").is_some());
        assert!(result.get("theme").is_some());
        assert_eq!(result["theme"], Value::String("dark".into()));
    }

    #[test]
    fn read_settings_errors_on_invalid_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.json");
        fs::write(&path, "{ not valid json !!").unwrap();

        let err = read_settings(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid JSON"));
        assert!(msg.contains("broken.json"));
    }

    #[test]
    fn write_settings_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("deep").join("nested").join("settings.json");
        let value = json!({"key": "value"});

        write_settings(&path, &value).unwrap();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["key"], Value::String("value".into()));
    }

    #[test]
    fn write_settings_preserves_key_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let value = json!({"alpha": 1, "beta": 2, "gamma": 3});

        write_settings(&path, &value).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        let keys: Vec<&str> = parsed.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn is_code_graph_hook_identifies_our_hooks() {
        let entry = json!({
            "matcher": "Edit|Write",
            "hooks": [{"type": "command", "command": "code-graph index --incremental"}]
        });
        assert!(is_code_graph_hook(&entry));
    }

    #[test]
    fn is_code_graph_hook_ignores_other_hooks() {
        let entry = json!({
            "matcher": "Edit|Write",
            "hooks": [{"type": "command", "command": "echo hello"}]
        });
        assert!(!is_code_graph_hook(&entry));
    }

    #[test]
    fn hook_definitions_have_correct_structure() {
        let ss = session_start_hook();
        assert_eq!(ss["matcher"], "startup");
        let ss_hooks = ss["hooks"].as_array().unwrap();
        assert_eq!(ss_hooks.len(), 1);
        assert_eq!(ss_hooks[0]["type"], "command");
        assert!(ss_hooks[0]["command"].as_str().unwrap().contains("code-graph index --incremental"));
        assert_eq!(ss_hooks[0]["timeout"], 120);

        let ptu = post_tool_use_hook();
        assert_eq!(ptu["matcher"], "Edit|Write");
        let ptu_hooks = ptu["hooks"].as_array().unwrap();
        assert_eq!(ptu_hooks.len(), 1);
        assert!(ptu_hooks[0]["command"].as_str().unwrap().contains("code-graph index --incremental"));
        assert_eq!(ptu_hooks[0]["timeout"], 15);
    }

    // ── T04 tests: install mode ─────────────────────────────────────────────

    fn make_setup_args(platform: Option<&str>, global: bool, check: bool, remove: bool, clean: bool, purge: bool) -> SetupArgs {
        SetupArgs {
            platform: platform.map(String::from),
            global,
            check,
            remove,
            clean,
            purge,
        }
    }

    #[test]
    fn install_creates_hooks_in_empty_settings() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join(".git")).unwrap();
        let args = make_setup_args(Some("claude"), false, false, false, false, false);

        run_install(&args, Some(root)).unwrap();

        let settings_path = root.join(".claude").join("settings.json");
        let settings: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(settings["hooks"]["SessionStart"].as_array().unwrap().len() == 1);
        assert!(settings["hooks"]["PostToolUse"].as_array().unwrap().len() == 1);
        assert!(is_code_graph_hook(&settings["hooks"]["SessionStart"][0]));
        assert!(is_code_graph_hook(&settings["hooks"]["PostToolUse"][0]));
    }

    #[test]
    fn install_preserves_existing_settings() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, r#"{"env": {"DEBUG": "1"}, "permissions": {"allow": ["Read"]}}"#).unwrap();

        let args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&args, Some(root)).unwrap();

        let settings: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(settings["env"]["DEBUG"], "1");
        assert!(settings["permissions"]["allow"].as_array().unwrap().contains(&Value::String("Read".into())));
        assert!(settings.get("hooks").is_some());
    }

    #[test]
    fn install_preserves_existing_non_codegraph_hooks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let existing = json!({
            "hooks": {
                "SessionStart": [
                    {"matcher": "startup", "hooks": [{"type": "command", "command": "echo hello"}]}
                ]
            }
        });
        fs::write(&settings_path, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&args, Some(root)).unwrap();

        let settings: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let ss_arr = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss_arr.len(), 2, "should have both original and code-graph hook");
    }

    #[test]
    fn install_idempotent_no_duplicates() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let args = make_setup_args(Some("claude"), false, false, false, false, false);

        run_install(&args, Some(root)).unwrap();
        run_install(&args, Some(root)).unwrap();

        let settings_path = root.join(".claude").join("settings.json");
        let settings: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(settings["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_updates_outdated_hooks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let old = json!({
            "hooks": {
                "SessionStart": [
                    {"matcher": "startup", "hooks": [{"type": "command", "command": "code-graph old-command", "timeout": 60}]}
                ]
            }
        });
        fs::write(&settings_path, serde_json::to_string_pretty(&old).unwrap()).unwrap();

        let args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&args, Some(root)).unwrap();

        let settings: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let ss_arr = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss_arr.len(), 1, "should replace in place, not duplicate");
        let cmd = ss_arr[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("--incremental"), "should have updated command");
    }

    #[test]
    fn install_adds_gitignore_entry() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let args = make_setup_args(Some("claude"), false, false, false, false, false);

        run_install(&args, Some(root)).unwrap();

        let gitignore = fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(gitignore.contains(".code-graph/"));
    }

    // ── T05 tests: check mode ───────────────────────────────────────────────

    #[test]
    fn check_all_installed_reports_ok() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Install first
        let install_args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&install_args, Some(root)).unwrap();

        // Then check
        let check_args = make_setup_args(None, false, true, false, false, false);
        let result = run_check(&check_args, Some(root));
        assert!(result.is_ok());
    }

    #[test]
    fn check_missing_hooks_reports_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Create empty settings
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, "{}").unwrap();

        let check_args = make_setup_args(None, false, true, false, false, false);
        let result = run_check(&check_args, Some(root));
        assert!(result.is_err());
    }

    #[test]
    fn check_outdated_hook_reports_outdated() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let old = json!({
            "hooks": {
                "SessionStart": [
                    {"matcher": "startup", "hooks": [{"type": "command", "command": "code-graph old-cmd", "timeout": 60}]}
                ],
                "PostToolUse": [post_tool_use_hook()]
            }
        });
        fs::write(&settings_path, serde_json::to_string_pretty(&old).unwrap()).unwrap();

        let check_args = make_setup_args(None, false, true, false, false, false);
        let result = run_check(&check_args, Some(root));
        assert!(result.is_err(), "outdated hook should report error");
    }

    #[test]
    fn check_hook_status_installed() {
        let settings = json!({
            "hooks": {
                "SessionStart": [session_start_hook()]
            }
        });
        let status = check_hook_status(&settings, "SessionStart", &session_start_hook());
        assert_eq!(status, HookStatus::Installed);
    }

    #[test]
    fn check_hook_status_missing() {
        let settings = json!({});
        let status = check_hook_status(&settings, "SessionStart", &session_start_hook());
        assert_eq!(status, HookStatus::Missing);
    }

    #[test]
    fn check_hook_status_outdated() {
        let settings = json!({
            "hooks": {
                "SessionStart": [
                    {"matcher": "startup", "hooks": [{"type": "command", "command": "code-graph old-cmd"}]}
                ]
            }
        });
        let status = check_hook_status(&settings, "SessionStart", &session_start_hook());
        assert_eq!(status, HookStatus::Outdated);
    }

    // ── T06 tests: remove mode ──────────────────────────────────────────────

    #[test]
    fn remove_filters_code_graph_hooks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let settings = json!({
            "hooks": {
                "SessionStart": [
                    {"matcher": "startup", "hooks": [{"type": "command", "command": "echo hello"}]},
                    session_start_hook()
                ]
            }
        });
        fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();

        let args = make_setup_args(None, false, false, true, false, false);
        run_remove(&args, Some(root)).unwrap();

        let result: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let arr = result["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "should only have non-code-graph hook left");
        assert!(!is_code_graph_hook(&arr[0]));
    }

    #[test]
    fn remove_cleans_empty_event_arrays() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let settings = json!({
            "hooks": {
                "SessionStart": [session_start_hook()],
                "PostToolUse": [
                    {"matcher": "Edit", "hooks": [{"type": "command", "command": "echo other"}]}
                ]
            }
        });
        fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();

        let args = make_setup_args(None, false, false, true, false, false);
        run_remove(&args, Some(root)).unwrap();

        let result: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(result["hooks"].get("SessionStart").is_none(), "empty event should be removed");
        assert!(result["hooks"]["PostToolUse"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn remove_cleans_empty_hooks_object() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Install then remove
        let install_args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&install_args, Some(root)).unwrap();

        let remove_args = make_setup_args(None, false, false, true, false, false);
        run_remove(&remove_args, Some(root)).unwrap();

        let settings_path = root.join(".claude").join("settings.json");
        let result: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(result.get("hooks").is_none(), "empty hooks object should be removed");
    }

    #[test]
    fn remove_noop_when_no_hooks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, "{}").unwrap();

        let args = make_setup_args(None, false, false, true, false, false);
        let result = run_remove(&args, Some(root));
        assert!(result.is_ok());
    }

    #[test]
    fn remove_with_clean_removes_gitignore() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Set up: install hooks + gitignore entry
        let install_args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&install_args, Some(root)).unwrap();
        assert!(fs::read_to_string(root.join(".gitignore")).unwrap().contains(".code-graph/"));

        // Remove with --clean
        let remove_args = make_setup_args(None, false, false, true, true, false);
        run_remove(&remove_args, Some(root)).unwrap();

        let gitignore = fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(!gitignore.contains(".code-graph/"));
    }

    #[test]
    fn remove_with_purge_deletes_data_dir() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create .code-graph/ directory
        let data_dir = root.join(".code-graph");
        fs::create_dir(&data_dir).unwrap();
        fs::write(data_dir.join("graph.db"), "test").unwrap();
        assert!(data_dir.is_dir());

        // Install then purge
        let install_args = make_setup_args(Some("claude"), false, false, false, false, false);
        run_install(&install_args, Some(root)).unwrap();

        let remove_args = make_setup_args(None, false, false, true, true, true);
        run_remove(&remove_args, Some(root)).unwrap();

        assert!(!data_dir.exists(), ".code-graph/ should be deleted");
    }

    #[test]
    fn remove_preserves_other_settings() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let settings_path = root.join(".claude").join("settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let settings = json!({
            "env": {"DEBUG": "1"},
            "hooks": {
                "SessionStart": [session_start_hook()]
            }
        });
        fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();

        let args = make_setup_args(None, false, false, true, false, false);
        run_remove(&args, Some(root)).unwrap();

        let result: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(result["env"]["DEBUG"], "1");
    }
}
