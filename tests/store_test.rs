use agents_connector::broker::store::Store;
use tempfile::TempDir;

#[test]
fn opens_creates_schema_and_registers_agent() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite");

    let store = Store::open(&db_path).unwrap();
    let token = store.register_agent("alice", "claude").unwrap();
    assert!(!token.is_empty());

    let by_token = store.agent_by_token(&token).unwrap().unwrap();
    assert_eq!(by_token.name, "alice");
    assert_eq!(by_token.cli_kind, "claude");

    let by_name = store.agent_by_name("alice").unwrap().unwrap();
    assert_eq!(by_name.id, by_token.id);
}

#[test]
fn rejects_duplicate_agent_name() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite");
    let store = Store::open(&db_path).unwrap();

    store.register_agent("alice", "claude").unwrap();
    let err = store.register_agent("alice", "codex").unwrap_err();
    assert!(format!("{:#}", err).to_lowercase().contains("agent already exists"));
}

#[test]
fn list_agents_returns_all() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite");
    let store = Store::open(&db_path).unwrap();

    store.register_agent("alice", "claude").unwrap();
    store.register_agent("bob", "claude").unwrap();
    let agents = store.list_agents().unwrap();
    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"alice"));
    assert!(names.contains(&"bob"));
}
