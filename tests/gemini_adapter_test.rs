use agents_connector::adapters::gemini;
use std::path::PathBuf;

#[test]
fn writes_settings_with_required_keys() {
    let tmp = tempfile::TempDir::new().unwrap();
    let agent_dir = tmp.path().join("alice");
    let bin = PathBuf::from("/usr/local/bin/agents-connector");
    let sock = PathBuf::from("/tmp/sock");

    let result = gemini::generate(&agent_dir, &bin, &sock, "TOK").unwrap();
    assert!(agent_dir.join(".gemini/settings.json").exists());
    assert_eq!(result.launch_cwd, agent_dir);

    let body = std::fs::read_to_string(agent_dir.join(".gemini/settings.json")).unwrap();
    assert!(body.contains("\"agents_connector\""));
    assert!(body.contains("\"BeforeAgent\""));
    assert!(body.contains("\"AfterTool\""));
}
