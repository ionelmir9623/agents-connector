//! Claude Code adapter.
//!
//! Generates:
//!   - An MCP config file (mcp.json) that points Claude Code at our shim.
//!   - A `settings.json` with hooks that call our `hook` subcommand.
//!
//! Hook events used: `UserPromptSubmit`, `PostToolUse`, `SessionStart`, `Stop`.
//! The first three support `hookSpecificOutput.additionalContext` per
//! https://code.claude.com/docs/en/hooks.
//!
//! Note: Claude's `Stop` hook does NOT inject context — Claude's docs say Stop
//! only supports a top-level `decision: "block"` and ignores `additionalContext`.
//! We wire `Stop` anyway so the hook subcommand can write `state = idle` to SQLite.

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

pub struct Generated {
    pub mcp_config_path: PathBuf,
    pub settings_path: PathBuf,
}

/// Writes the per-agent MCP config and settings files.
///
/// Layout:
///   <agent_dir>/mcp.json       — Claude --mcp-config target
///   <agent_dir>/settings.json  — Claude --settings target (with hooks)
pub fn generate(
    agent_dir: &Path,
    binary_path: &Path,
    socket_path: &Path,
    agent_token: &str,
) -> Result<Generated> {
    std::fs::create_dir_all(agent_dir)?;

    let mcp_config = json!({
        "mcpServers": {
            "agents_connector": {
                "command": binary_path.to_string_lossy(),
                "args": [
                    "mcp-shim",
                    "--socket", socket_path.to_string_lossy(),
                    "--agent-token", agent_token,
                ],
                "env": {}
            }
        }
    });
    let mcp_config_path = agent_dir.join("mcp.json");
    std::fs::write(&mcp_config_path, serde_json::to_string_pretty(&mcp_config)?)?;

    let bin_q = shell_quote(&binary_path.to_string_lossy());
    let sock_q = shell_quote(&socket_path.to_string_lossy());
    let token_q = shell_quote(agent_token);
    let hook_cmd = |event: &str| {
        format!(
            "{} hook --socket {} --agent-token {} --event {} --cli-kind claude",
            bin_q, sock_q, token_q, event,
        )
    };

    let settings = json!({
        "hooks": {
            "UserPromptSubmit": [{
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": hook_cmd("user_prompt_submit"),
                }]
            }],
            "PostToolUse": [{
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": hook_cmd("post_tool_use"),
                }]
            }],
            "SessionStart": [{
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": hook_cmd("session_start"),
                }]
            }],
            "Stop": [{
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": hook_cmd("stop"),
                }]
            }]
        }
    });
    let settings_path = agent_dir.join("settings.json");
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;

    Ok(Generated { mcp_config_path, settings_path })
}

fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./=:".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn generates_both_files_with_expected_keys() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_dir = tmp.path().join("alice");
        let binary = PathBuf::from("/usr/local/bin/agents-connector");
        let socket = PathBuf::from("/tmp/sock");
        let result = generate(&agent_dir, &binary, &socket, "TOKEN-123").unwrap();

        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&result.mcp_config_path).unwrap()).unwrap();
        assert!(mcp.get("mcpServers").and_then(|v| v.get("agents_connector")).is_some());

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&result.settings_path).unwrap()).unwrap();

        // Three context-injectable hooks must be present (UserPromptSubmit, PostToolUse, SessionStart).
        // Stop is checked separately below.
        for (event, snake) in [
            ("UserPromptSubmit", "user_prompt_submit"),
            ("PostToolUse", "post_tool_use"),
            ("SessionStart", "session_start"),
        ] {
            let path = format!("/hooks/{}/0/hooks/0/command", event);
            let cmd = settings
                .pointer(&path)
                .unwrap_or_else(|| panic!("missing hook for {}", event))
                .as_str()
                .unwrap();
            assert!(cmd.contains("hook"));
            assert!(cmd.contains("TOKEN-123"));
            assert!(cmd.contains(&format!("--event {}", snake)), "got: {}", cmd);
            assert!(cmd.contains("--cli-kind claude"), "got: {}", cmd);
        }

        // Stop hook must be present — it updates agent state to idle.
        // (It doesn't inject additionalContext, but state-writing fires before injection check.)
        let stop_cmd = settings
            .pointer("/hooks/Stop/0/hooks/0/command")
            .expect("Stop hook should be configured for state tracking")
            .as_str()
            .unwrap();
        assert!(stop_cmd.contains("--event stop"), "got: {}", stop_cmd);
        assert!(stop_cmd.contains("--cli-kind claude"), "got: {}", stop_cmd);
    }

    #[test]
    fn handles_paths_with_spaces() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_dir = tmp.path().join("alice");
        let binary = std::path::PathBuf::from("/usr/local/bin/agents-connector");
        let socket = std::path::PathBuf::from("/tmp/with space/sock");
        let result = generate(&agent_dir, &binary, &socket, "TOKEN-123").unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&result.settings_path).unwrap()).unwrap();
        // Path with spaces must appear quoted in at least one of the three hooks.
        let cmd = settings
            .pointer("/hooks/UserPromptSubmit/0/hooks/0/command")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(cmd.contains("'/tmp/with space/sock'"), "got: {}", cmd);
    }
}
