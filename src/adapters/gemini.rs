//! Gemini CLI adapter.
//!
//! Gemini reads MCP servers and hooks from `<cwd>/.gemini/settings.json` (merged with
//! `~/.gemini/settings.json`). It has no per-invocation `-c` override and no
//! `--settings <file>` flag, so per-agent isolation requires running gemini in a
//! per-agent cwd. We write the agent's settings to `<agent_dir>/.gemini/settings.json`
//! and the launcher cd's into `<agent_dir>` before exec-ing gemini.
//!
//! Schema reference: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

pub struct Generated {
    /// The agent_dir itself — gemini should be launched with this as cwd
    /// so its `.gemini/settings.json` is discovered as project-local config.
    pub launch_cwd: PathBuf,
}

/// Writes `<agent_dir>/.gemini/settings.json` with our MCP server entry and
/// the `BeforeAgent` + `AfterTool` hooks pointing at our hook subcommand.
pub fn generate(
    agent_dir: &Path,
    binary_path: &Path,
    socket_path: &Path,
    agent_token: &str,
) -> Result<Generated> {
    let dot_gemini = agent_dir.join(".gemini");
    std::fs::create_dir_all(&dot_gemini)?;

    let bin = binary_path.to_string_lossy().to_string();
    let sock = socket_path.to_string_lossy().to_string();

    let hook_before_agent = format!(
        "{} hook --socket {} --agent-token {} --event before_agent --cli-kind gemini",
        bin, sock, agent_token
    );
    let hook_after_tool = format!(
        "{} hook --socket {} --agent-token {} --event after_tool --cli-kind gemini",
        bin, sock, agent_token
    );
    let hook_after_agent = format!(
        "{} hook --socket {} --agent-token {} --event after_agent --cli-kind gemini",
        bin, sock, agent_token
    );

    let settings = json!({
        "mcpServers": {
            "agents_connector": {
                "command": bin,
                "args": [
                    "mcp-shim",
                    "--socket", sock,
                    "--agent-token", agent_token,
                ],
                "env": {}
            }
        },
        "hooks": {
            "BeforeAgent": [{
                "matchers": ["*"],
                "command": hook_before_agent,
            }],
            "AfterTool": [{
                "matchers": ["*"],
                "command": hook_after_tool,
            }],
            "AfterAgent": [{
                "matchers": ["*"],
                "command": hook_after_agent,
            }],
        }
    });

    let settings_path = dot_gemini.join("settings.json");
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;

    Ok(Generated { launch_cwd: agent_dir.to_path_buf() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_settings_with_mcp_and_hooks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_dir = tmp.path().join("alice");
        let bin = PathBuf::from("/usr/local/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let result = generate(&agent_dir, &bin, &sock, "TOK-G").unwrap();

        // Returned cwd is the agent_dir.
        assert_eq!(result.launch_cwd, agent_dir);

        // settings.json was written under .gemini/
        let settings_path = agent_dir.join(".gemini").join("settings.json");
        assert!(settings_path.exists());

        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();

        // MCP server present.
        let mcp = &parsed["mcpServers"]["agents_connector"];
        assert_eq!(mcp["command"], "/usr/local/bin/agents-connector");
        assert_eq!(mcp["args"][0], "mcp-shim");
        assert_eq!(mcp["args"][2], "/tmp/sock");
        assert_eq!(mcp["args"][4], "TOK-G");

        // All three hooks present and point at our hook subcommand with correct event + cli-kind.
        let before = &parsed["hooks"]["BeforeAgent"][0]["command"];
        let after_tool = &parsed["hooks"]["AfterTool"][0]["command"];
        let after_agent = &parsed["hooks"]["AfterAgent"][0]["command"];
        assert!(before.as_str().unwrap().contains("--event before_agent"));
        assert!(before.as_str().unwrap().contains("--cli-kind gemini"));
        assert!(after_tool.as_str().unwrap().contains("--event after_tool"));
        assert!(after_tool.as_str().unwrap().contains("--cli-kind gemini"));
        assert!(after_agent.as_str().unwrap().contains("--event after_agent"));
        assert!(after_agent.as_str().unwrap().contains("--cli-kind gemini"));
    }
}
