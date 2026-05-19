//! Codex CLI adapter.
//!
//! Codex (>= 0.120.0) reads its config from `~/.codex/config.toml` (or
//! `<CODEX_HOME>/config.toml`). The `-c <key=value>` flag overrides config
//! values per invocation; the value is parsed as TOML.
//!
//! We use `-c` overrides to inject:
//!   1. The `hooks` feature flag (gated off by default; `codex_hooks` is the deprecated old name).
//!   2. An `agents_connector` MCP server entry under `[mcp_servers]`.
//!   3. `[[hooks.PostToolUse]]` and `[[hooks.UserPromptSubmit]]` arrays
//!      with nested `[[hooks.<event>.hooks]]` arrays of `{ type = "command", command = "..." }`
//!      so new messages can be injected as `hookSpecificOutput.additionalContext`.
//!
//! Codex requires the user to explicitly approve each hook the first time it runs
//! (security gate). Open `/hooks` in the codex TUI and approve our two hooks once
//! per agent — they're persisted as approved after that.
//!
//! Schema reference: https://developers.openai.com/codex/hooks
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

    // Enable hooks (gated off by default in current codex versions).
    // The flag name is `hooks` in current codex; older versions used `codex_hooks` (deprecated).
    let feature_flag = "features.hooks=true".to_string();

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
    let hook_post_cmd = format!(
        "{} hook --socket {} --agent-token {} --event post_tool_use --cli-kind codex",
        binary_path.to_string_lossy(),
        socket_path.to_string_lossy(),
        agent_token,
    );
    let hook_userprompt_cmd = format!(
        "{} hook --socket {} --agent-token {} --event user_prompt_submit --cli-kind codex",
        binary_path.to_string_lossy(),
        socket_path.to_string_lossy(),
        agent_token,
    );
    let hook_stop_cmd = format!(
        "{} hook --socket {} --agent-token {} --event stop --cli-kind codex",
        binary_path.to_string_lossy(),
        socket_path.to_string_lossy(),
        agent_token,
    );

    // Schema (per https://developers.openai.com/codex/hooks):
    //   [[hooks.PostToolUse]]
    //   [[hooks.PostToolUse.hooks]]
    //   type = "command"
    //   command = "..."
    //
    // Inline TOML form for `-c`:
    //   hooks.PostToolUse = [{ hooks = [{ type = "command", command = "..." }] }]
    let post_override = format!(
        "hooks.PostToolUse=[{{ hooks = [{{ type = \"command\", command = {} }}] }}]",
        toml_string(&hook_post_cmd)
    );
    let userprompt_override = format!(
        "hooks.UserPromptSubmit=[{{ hooks = [{{ type = \"command\", command = {} }}] }}]",
        toml_string(&hook_userprompt_cmd)
    );
    let stop_override = format!(
        "hooks.Stop=[{{ hooks = [{{ type = \"command\", command = {} }}] }}]",
        toml_string(&hook_stop_cmd)
    );

    vec![feature_flag, mcp_command, mcp_args, post_override, userprompt_override, stop_override]
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
    fn config_overrides_includes_feature_flag_mcp_and_hook_keys() {
        let bin = PathBuf::from("/usr/local/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let overrides = config_overrides(&bin, &sock, "TOK-99");

        assert_eq!(overrides.len(), 6);
        assert_eq!(overrides[0], "features.hooks=true");
        assert!(overrides[1].starts_with("mcp_servers.agents_connector.command="));
        assert!(overrides[2].starts_with("mcp_servers.agents_connector.args="));
        assert!(overrides[3].starts_with("hooks.PostToolUse="));
        assert!(overrides[4].starts_with("hooks.UserPromptSubmit="));
        assert!(overrides[5].starts_with("hooks.Stop="));

        // Hook overrides must include the nested `type = "command"` form.
        assert!(overrides[3].contains("type = \"command\""));
        assert!(overrides[4].contains("type = \"command\""));
        assert!(overrides[5].contains("type = \"command\""));
        assert!(overrides[3].contains("TOK-99"));
        assert!(overrides[4].contains("TOK-99"));
        assert!(overrides[5].contains("TOK-99"));
        assert!(overrides[2].contains("/tmp/sock"));
        assert!(overrides[3].contains("/tmp/sock"));
        assert!(overrides[5].contains("--event stop"));
    }

    #[test]
    fn handles_paths_with_special_chars() {
        let bin = PathBuf::from("/opt/with space/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let overrides = config_overrides(&bin, &sock, "TOK-1");

        // mcp_servers.agents_connector.command override carries the binary path.
        assert!(overrides[1].contains("/opt/with space/bin/agents-connector"));
    }
}
