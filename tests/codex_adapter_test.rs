use agents_connector::adapters::codex;
use std::path::PathBuf;

#[test]
fn produces_feature_flag_mcp_and_hook_overrides() {
    let bin = PathBuf::from("/usr/local/bin/agents-connector");
    let sock = PathBuf::from("/tmp/sock");
    let overrides = codex::config_overrides(&bin, &sock, "TOK-1");
    assert_eq!(overrides.len(), 5);

    let joined = overrides.join(" ");
    assert!(joined.contains("features.hooks=true"));
    assert!(joined.contains("mcp_servers.agents_connector"));
    assert!(joined.contains("hooks.PostToolUse"));
    assert!(joined.contains("hooks.UserPromptSubmit"));
    assert!(joined.contains("type = \"command\""));
    assert!(joined.contains("--cli-kind codex"));
    assert!(joined.contains("--event post_tool_use"));
    assert!(joined.contains("--event user_prompt_submit"));
}
