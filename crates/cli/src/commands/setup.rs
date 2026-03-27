use std::path::Path;
use std::fs;
use serde_json::{json, Value};
use domain::error::{CodeGraphError, Result};

use super::SetupArgs;

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

pub fn run_setup(args: &SetupArgs) -> Result<()> {
    Err(CodeGraphError::Other("setup: not yet implemented".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    #[test]
    fn read_settings_returns_empty_for_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let result = read_settings(&path).unwrap();
        assert!(result.is_object(), "expected object, got {:?}", result);
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
        assert!(result.get("hooks").is_some(), "missing 'hooks' key");
        assert!(result.get("theme").is_some(), "missing 'theme' key");
        assert_eq!(result["theme"], Value::String("dark".into()));
    }

    #[test]
    fn read_settings_errors_on_invalid_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.json");
        fs::write(&path, "{ not valid json !!").unwrap();

        let err = read_settings(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("Invalid JSON"),
            "error should mention 'Invalid JSON', got: {}",
            msg
        );
        assert!(
            msg.contains("broken.json"),
            "error should contain file name, got: {}",
            msg
        );
    }

    #[test]
    fn write_settings_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("deep").join("nested").join("settings.json");
        let value = json!({"key": "value"});

        write_settings(&path, &value).unwrap();

        assert!(path.exists(), "settings file should have been created");
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
            "hooks": [
                {"type": "command", "command": "code-graph index --incremental"}
            ]
        });
        assert!(is_code_graph_hook(&entry));
    }

    #[test]
    fn is_code_graph_hook_ignores_other_hooks() {
        let entry = json!({
            "matcher": "Edit|Write",
            "hooks": [
                {"type": "command", "command": "echo hello"}
            ]
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
        assert_eq!(ptu_hooks[0]["type"], "command");
        assert!(ptu_hooks[0]["command"].as_str().unwrap().contains("code-graph index --incremental"));
        assert_eq!(ptu_hooks[0]["timeout"], 15);
    }
}
