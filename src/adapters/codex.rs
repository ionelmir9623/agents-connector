//! Codex CLI adapter.
//!
//! Codex (>= 0.120.0) reads its config from `~/.codex/config.toml` (or
//! `<CODEX_HOME>/config.toml`). The `-c <key=value>` flag overrides config
//! values per invocation; the value is parsed as TOML.
//!
//! We use `-c` overrides to inject:
//!   1. An `agents_connector` MCP server entry under `[mcp_servers]`.
//!   2. `[hooks]` for `PostToolUse` and `UserPromptSubmit` so new messages
//!      can be injected as `hookSpecificOutput.additionalContext`.
//!
//! We do NOT use `Stop` for context injection because Codex's Stop hook is
//! fire-and-forget and cannot return additionalContext.
//!
//! No files are written; everything goes on the launch command line.

use std::path::Path;

/// Returns the list of `-c key=value` pairs to pass to `codex` for this agent.
/// Each element is a *single argv token*; build the command by interleaving
/// "-c" with each returned string.
pub fn config_overrides(
    binary_path: &Path,
    socket_path: &Path,
    agent_token: &str,
) -> Vec<String> {
    let bin = toml_string(&binary_path.to_string_lossy());
    let sock = toml_string(&socket_path.to_string_lossy());
    let token = toml_string(agent_token);

    // MCP server — agents_connector points at our shim subprocess.
    let mcp_command = format!("mcp_servers.agents_connector.command={}", bin);
    let mcp_args = format!(
        "mcp_servers.agents_connector.args=[{}, {}, {}, {}, {}]",
        toml_string("mcp-shim"),
        toml_string("--socket"),
        sock,
        toml_string("--agent-token"),
        token,
    );

    // Hook commands — invoke our hook subcommand with --cli-kind codex.
    let hook_post = format!(
        "{} hook --socket {} --agent-token {} --event post_tool_use --cli-kind codex",
        binary_path.to_string_lossy(),
        socket_path.to_string_lossy(),
        agent_token,
    );
    let hook_userprompt = format!(
        "{} hook --socket {} --agent-token {} --event user_prompt_submit --cli-kind codex",
        binary_path.to_string_lossy(),
        socket_path.to_string_lossy(),
        agent_token,
    );

    let post_override = format!(
        "hooks.PostToolUse=[{{ command = {} }}]",
        toml_string(&hook_post)
    );
    let userprompt_override = format!(
        "hooks.UserPromptSubmit=[{{ command = {} }}]",
        toml_string(&hook_userprompt)
    );

    vec![mcp_command, mcp_args, post_override, userprompt_override]
}

/// TOML-quote a string value with double-quotes, escaping internal quotes and backslashes.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn config_overrides_includes_mcp_and_hook_keys() {
        let bin = PathBuf::from("/usr/local/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let overrides = config_overrides(&bin, &sock, "TOK-99");

        assert_eq!(overrides.len(), 4);
        assert!(overrides[0].starts_with("mcp_servers.agents_connector.command="));
        assert!(overrides[1].starts_with("mcp_servers.agents_connector.args="));
        assert!(overrides[2].starts_with("hooks.PostToolUse="));
        assert!(overrides[3].starts_with("hooks.UserPromptSubmit="));

        assert!(overrides[2].contains("TOK-99"));
        assert!(overrides[3].contains("TOK-99"));
        assert!(overrides[1].contains("/tmp/sock"));
        assert!(overrides[2].contains("/tmp/sock"));
    }

    #[test]
    fn handles_paths_with_special_chars() {
        let bin = PathBuf::from("/Users/frog with space/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let overrides = config_overrides(&bin, &sock, "TOK-1");

        assert!(overrides[0].contains("/Users/frog with space/bin/agents-connector"));
    }
}
