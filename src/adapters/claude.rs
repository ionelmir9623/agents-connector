//! Claude Code adapter.
//!
//! Generates:
//!   - An MCP config file (mcp.json) that points Claude Code at our shim.
//!   - A `settings.json` with a Stop hook that calls our `hook` subcommand.

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
///   <agent_dir>/settings.json  — Claude --settings target (with Stop hook)
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

    // Stop hook: invoked by Claude Code at the end of every turn.
    let settings = json!({
        "hooks": {
            "Stop": [{
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": format!(
                        "{} hook --socket {} --agent-token {} --event stop",
                        shell_quote(&binary_path.to_string_lossy()),
                        shell_quote(&socket_path.to_string_lossy()),
                        shell_quote(agent_token)
                    )
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
        let cmd = settings.pointer("/hooks/Stop/0/hooks/0/command").unwrap().as_str().unwrap();
        assert!(cmd.contains("hook"));
        assert!(cmd.contains("TOKEN-123"));
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
        let cmd = settings.pointer("/hooks/Stop/0/hooks/0/command").unwrap().as_str().unwrap();
        // Path with spaces must appear quoted in the command string.
        assert!(cmd.contains("'/tmp/with space/sock'"), "got: {}", cmd);
    }
}
