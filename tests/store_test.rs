use agents_connector::broker::store::Store;
use tempfile::TempDir;

#[test]
fn opens_creates_schema_and_registers_agent() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite");

    let store = Store::open(&db_path).unwrap();
    let token = store.register_agent("alice", "claude", None).unwrap();
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

    store.register_agent("alice", "claude", None).unwrap();
    let err = store.register_agent("alice", "codex", None).unwrap_err();
    assert!(format!("{:#}", err).to_lowercase().contains("agent already exists"));
}

#[test]
fn list_agents_returns_all() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite");
    let store = Store::open(&db_path).unwrap();

    store.register_agent("alice", "claude", None).unwrap();
    store.register_agent("bob", "claude", None).unwrap();
    let agents = store.list_agents().unwrap();
    let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"alice"));
    assert!(names.contains(&"bob"));
}

#[test]
fn tells_and_reads_messages() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    let _alice = store.register_agent("alice", "claude", None).unwrap();
    let _bob = store.register_agent("bob", "claude", None).unwrap();

    let msg_id = store.tell("alice", Some("bob"), "hello bob").unwrap();
    assert!(msg_id > 0);

    let msgs = store.read_messages_for("bob", 0).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text, "hello bob");
    assert_eq!(msgs[0].from_name, "alice");
    assert_eq!(msgs[0].to_name, Some("bob".to_string()));

    // After reading, second call with the new high-water-mark returns empty.
    let high = msgs[0].id;
    let msgs2 = store.read_messages_for("bob", high).unwrap();
    assert!(msgs2.is_empty());
}

#[test]
fn broadcast_tell_visible_to_everyone_but_sender() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    store.register_agent("alice", "claude", None).unwrap();
    store.register_agent("bob", "claude", None).unwrap();
    store.register_agent("carol", "claude", None).unwrap();

    store.tell("alice", None, "hello everyone").unwrap();

    assert_eq!(store.read_messages_for("bob", 0).unwrap().len(), 1);
    assert_eq!(store.read_messages_for("carol", 0).unwrap().len(), 1);
    assert_eq!(store.read_messages_for("alice", 0).unwrap().len(), 0);
}

#[test]
fn ask_and_reply_links_correctly() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    store.register_agent("alice", "claude", None).unwrap();
    store.register_agent("bob", "claude", None).unwrap();

    let ask = store.ask("alice", "bob", "are you there?").unwrap();
    assert!(ask.ask_id > 0);
    assert!(ask.message_id > 0);

    // Bob sees it via read_messages
    let msgs = store.read_messages_for("bob", 0).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].ask_id, Some(ask.ask_id));
    assert_eq!(msgs[0].id, ask.message_id);

    // Bob replies
    let reply = store.post_reply("bob", ask.ask_id, "yes I am").unwrap();
    assert!(reply.reply_id > 0);
    assert_eq!(reply.original_asker, "alice");

    // Alice checks for replies on her ask
    let replies = store.replies_for_ask(ask.ask_id).unwrap();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].text, "yes I am");
    assert_eq!(replies[0].from_name, "bob");
}

#[test]
fn agent_by_token_excludes_soft_deleted() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite");
    let store = Store::open(&db_path).unwrap();

    let token = store.register_agent("alice", "claude", None).unwrap();
    assert!(store.agent_by_token(&token).unwrap().is_some());

    // Manually soft-delete via raw SQL (we don't have a remove() method yet — Plan 2).
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("UPDATE agents SET removed_at = ?1 WHERE token = ?2",
        rusqlite::params!["2026-05-09T10:00:00Z", &token]).unwrap();

    assert!(store.agent_by_token(&token).unwrap().is_none());
}

#[test]
fn workdir_round_trips_through_register_and_lookup() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    let token = store.register_agent("alice", "claude", Some("/tmp/workdir")).unwrap();
    let by_token = store.agent_by_token(&token).unwrap().unwrap();
    assert_eq!(by_token.workdir.as_deref(), Some("/tmp/workdir"));

    let token2 = store.register_agent("bob", "claude", None).unwrap();
    let by_token2 = store.agent_by_token(&token2).unwrap().unwrap();
    assert_eq!(by_token2.workdir, None);
}

#[test]
fn remove_agent_soft_deletes_and_returns_token() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    let token = store.register_agent("alice", "claude", None).unwrap();
    let removed_token = store.remove_agent("alice").unwrap();
    assert_eq!(removed_token, token);

    // Lookup by name returns None now.
    assert!(store.agent_by_name("alice").unwrap().is_none());
    // Lookup by token also returns None (token is for an inactive agent).
    assert!(store.agent_by_token(&token).unwrap().is_none());
    // list_agents excludes the removed agent.
    assert!(store.list_agents().unwrap().is_empty());
}

#[test]
fn remove_agent_errors_if_not_found() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    let err = store.remove_agent("ghost").unwrap_err();
    assert!(format!("{:#}", err).to_lowercase().contains("not found"));
}

#[test]
fn remove_agent_errors_if_already_removed() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    store.register_agent("alice", "claude", None).unwrap();
    store.remove_agent("alice").unwrap();
    let err = store.remove_agent("alice").unwrap_err();
    assert!(format!("{:#}", err).to_lowercase().contains("not found or already removed"));
}

#[test]
fn set_agent_state_round_trips() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    let token = store.register_agent("alice", "claude", None).unwrap();

    // Initial state should be idle (schema default).
    let agent = store.agent_by_name("alice").unwrap().unwrap();
    assert_eq!(agent.state, "idle");

    // Set to busy by name.
    store.set_agent_state_by_name("alice", "busy").unwrap();
    let agent = store.agent_by_name("alice").unwrap().unwrap();
    assert_eq!(agent.state, "busy");

    // Set back to idle by token.
    store.set_agent_state_by_token(&token, "idle").unwrap();
    let agent = store.agent_by_token(&token).unwrap().unwrap();
    assert_eq!(agent.state, "idle");

    // Verify list_agents also returns updated state.
    let agents = store.list_agents().unwrap();
    assert_eq!(agents[0].state, "idle");

    // set_agent_state_by_token errors on unknown token.
    let err = store.set_agent_state_by_token("no-such-token", "busy").unwrap_err();
    assert!(format!("{:#}", err).contains("not found"));
}
