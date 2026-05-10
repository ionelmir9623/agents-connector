# `agents-connector` v1 (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the minimum end-to-end multi-agent chat substrate: a single binary that lets two Claude Code instances, running in separate tmux panes of one session, exchange messages via an MCP-exposed `tell`/`ask`/`reply` protocol with a durable per-session SQLite store.

**Architecture:** A single Rust binary with multiple subcommands. The launcher subcommands (`start`, `add`, `list`, `stop`, `attach`, `tail`) drive a tmux session and spawn a long-running broker daemon (also a subcommand of the same binary). The broker owns a SQLite store and accepts internal IPC over a Unix socket. Each agent runs an MCP server over stdio (also a subcommand of the same binary, "mcp-shim") that translates MCP tool calls into broker IPC. Claude Code is wired in via an MCP config file and a Stop hook script.

**Tech Stack:** Rust 2021, `tokio` (async runtime), `clap` (CLI), `rusqlite` with `bundled` feature (SQLite, no system dep), `rmcp` (MCP server SDK — verify currency at task 0), `serde`/`serde_json` (serialization), `tracing` (logs), `anyhow`/`thiserror` (errors), `directories` (XDG paths), `chrono` (timestamps), `nix` (signals/Unix sockets), `assert_cmd`/`tempfile`/`predicates` (tests). External: `tmux` (system).

**Final state at end of plan:** Running `agents-connector start demo`, then `agents-connector add claude --name alice` twice (alice + bob) gives two Claude windows in one tmux session. Telling alice "send bob hello via the agents_connector tools" results in bob seeing the message at end of bob's next turn (via Stop hook). Restarting the broker and reconnecting preserves the chat history. Cargo tests pass.

---

## File structure

```
/Users/frog/code/terminal_tool/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── src/
│   ├── main.rs                    # entrypoint; clap dispatch
│   ├── cli.rs                     # top-level Cli struct + Subcommand enum
│   ├── paths.rs                   # XDG path helpers (sessions dir, socket paths)
│   ├── tmux.rs                    # tmux command wrappers (new-session, new-window, send-keys, etc.)
│   ├── ipc.rs                     # length-prefixed JSON protocol over Unix socket (broker <-> shim, broker <-> launcher)
│   ├── broker/
│   │   ├── mod.rs                 # broker daemon entrypoint (run by `broker --serve`)
│   │   ├── store.rs               # SQLite schema + CRUD
│   │   ├── server.rs              # accept connections, dispatch IPC requests
│   │   └── handlers.rs            # request handlers: register, tell, ask, read_messages, etc.
│   ├── shim/
│   │   └── mod.rs                 # mcp-shim subcommand: stdio MCP server <-> broker IPC client
│   ├── adapters/
│   │   ├── mod.rs                 # CliKind enum + dispatch
│   │   └── claude.rs              # Claude Code adapter: MCP config gen + Stop hook script gen
│   ├── hook/
│   │   └── mod.rs                 # `hook` subcommand: invoked by Claude Code's Stop hook
│   ├── subcommands/
│   │   ├── mod.rs
│   │   ├── start.rs
│   │   ├── add.rs
│   │   ├── list.rs
│   │   ├── stop.rs
│   │   ├── attach.rs
│   │   └── tail.rs
│   └── lib.rs                     # re-exports for integration tests
├── tests/
│   ├── store_test.rs              # SQLite store integration
│   ├── broker_ipc_test.rs         # broker IPC protocol end-to-end
│   ├── cli_test.rs                # subcommand smoke tests via assert_cmd
│   └── e2e_test.rs                # spawn launcher + broker + shim, exercise full flow (no real Claude — synthetic MCP client)
└── docs/
    └── superpowers/
        ├── specs/                 # (brainstorm doc lives elsewhere; nothing here in v1)
        └── plans/
            └── 2026-05-09-agents-connector-v1.md  # this file
```

**Key boundaries:**
- `broker` knows nothing about MCP. It speaks an internal IPC protocol.
- `shim` knows MCP and IPC. It translates one to the other.
- `adapters` know how to configure each CLI. They generate config files for shim/hook.
- `hook` knows how to call the broker as a one-shot at end-of-turn.
- `subcommands` know about tmux + adapters; they don't talk MCP.

---

## Sequencing

Tasks build on each other. Each task ends with passing tests + a commit.

```
0. Verify prerequisites          (cargo, tmux, rmcp crate version)
1. Cargo project + clap skeleton
2. paths module
3. SQLite store: schema + agents
4. SQLite store: messages, asks, replies
5. ipc module: framing + types
6. broker daemon skeleton (listens, accepts, no-op handlers)
7. broker handlers: register, list_agents
8. broker handlers: tell, read_messages
9. broker handlers: ask, check_replies, wait_for_reply, post_reply
10. mcp-shim subcommand: stdio MCP server bridging to broker
11. tmux module: command wrappers
12. subcommand: start (broker spawn + tmux session)
13. subcommand: list
14. subcommand: stop (graceful broker shutdown)
15. subcommand: attach
16. subcommand: tail (live transcript over IPC subscribe)
17. claude adapter: MCP config generator
18. hook subcommand: end-of-turn message check
19. claude adapter: Stop hook script generator
20. subcommand: add (Claude-only)
21. End-to-end synthetic test (no real Claude)
22. README + manual smoke test
```

---

## Task 0: Verify prerequisites

**Files:** none

- [ ] **Step 1: Confirm `cargo` and `rustc` are installed**

Run: `cargo --version && rustc --version`
Expected: both print versions; `rustc` ≥ 1.75.

- [ ] **Step 2: Confirm `tmux` is installed**

Run: `tmux -V`
Expected: prints e.g. `tmux 3.4`.

If missing, install: `brew install tmux` (macOS) or `apt install tmux` (Debian/Ubuntu).

- [ ] **Step 3: Verify the `rmcp` crate's current version and API surface**

Run: `cargo search rmcp | head -3`
Then visit https://docs.rs/rmcp/latest/rmcp/ and confirm:
- It exposes a server/transport API for stdio.
- It has a "tools" macro or trait (typical pattern) to define MCP tools.
- It supports rmcp version ≥ 0.1 (or whatever current is at execution time).

If `rmcp` has been renamed, deprecated, or its API has materially shifted from what this plan assumes, **stop and adjust the affected tasks (10, 18) before proceeding**. The rest of the plan is not affected.

- [ ] **Step 4: No commit yet — nothing changed.**

---

## Task 1: Cargo project + clap skeleton

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/cli.rs`
- Create: `src/lib.rs`
- Create: `.gitignore`

- [ ] **Step 1: Initialize the cargo project**

Run from `/Users/frog/code/terminal_tool`:
```bash
cargo init --name agents-connector
```
Expected: creates `Cargo.toml`, `src/main.rs`, `.gitignore`, initializes git.

- [ ] **Step 2: Replace `Cargo.toml` with full dependency list**

Write `Cargo.toml`:
```toml
[package]
name = "agents-connector"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Multi-agent CLI communication substrate"
repository = "https://github.com/REPLACE-ME/agents-connector"

[dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.31", features = ["bundled", "chrono"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
thiserror = "1"
directories = "5"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
nix = { version = "0.27", features = ["signal"] }
# rmcp version: confirm at Task 0
rmcp = "0.1"

[dev-dependencies]
assert_cmd = "2"
tempfile = "3"
predicates = "3"
serial_test = "3"

[lib]
name = "agents_connector"
path = "src/lib.rs"

[[bin]]
name = "agents-connector"
path = "src/main.rs"
```

- [ ] **Step 3: Write a minimal `src/cli.rs` with the full subcommand surface (most are stubs)**

Write `src/cli.rs`:
```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agents-connector", version, about = "Multi-agent CLI communication substrate")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start a new session (creates broker + tmux).
    Start { session: String },
    /// Add an agent to the current session.
    Add {
        cli_kind: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        workdir: Option<std::path::PathBuf>,
    },
    /// List all sessions.
    List,
    /// Stop a running session.
    Stop {
        session: String,
        #[arg(long)]
        kill_tmux: bool,
    },
    /// Attach to a running session's tmux.
    Attach { session: String },
    /// Tail the chat transcript of a session.
    Tail {
        session: Option<String>,
    },
    /// Internal: run the broker daemon. Users should not invoke directly.
    #[command(hide = true)]
    Broker {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        db: std::path::PathBuf,
    },
    /// Internal: run the MCP shim. Users should not invoke directly.
    #[command(hide = true)]
    McpShim {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        agent_token: String,
    },
    /// Internal: invoked by adapter hooks (e.g., Claude Code Stop hook).
    #[command(hide = true)]
    Hook {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        agent_token: String,
        #[arg(long)]
        event: String,
    },
}
```

- [ ] **Step 4: Write `src/main.rs`**

```rust
use clap::Parser;
use agents_connector::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Start { session } => {
            anyhow::bail!("not yet implemented: start {}", session);
        }
        Command::Add { cli_kind, name, .. } => {
            anyhow::bail!("not yet implemented: add {} {}", cli_kind, name);
        }
        Command::List => anyhow::bail!("not yet implemented: list"),
        Command::Stop { session, .. } => anyhow::bail!("not yet implemented: stop {}", session),
        Command::Attach { session } => anyhow::bail!("not yet implemented: attach {}", session),
        Command::Tail { .. } => anyhow::bail!("not yet implemented: tail"),
        Command::Broker { .. } => anyhow::bail!("not yet implemented: broker"),
        Command::McpShim { .. } => anyhow::bail!("not yet implemented: mcp-shim"),
        Command::Hook { .. } => anyhow::bail!("not yet implemented: hook"),
    }
}
```

- [ ] **Step 5: Write `src/lib.rs`**

```rust
pub mod cli;
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: build succeeds with no errors. Warnings about unused imports/fields are OK.

- [ ] **Step 7: Verify the help text works**

Run: `cargo run -- --help`
Expected: prints help including `start`, `add`, `list`, `stop`, `attach`, `tail`. Internal subcommands (`broker`, `mcp-shim`, `hook`) are hidden.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/ .gitignore
git commit -m "feat: cargo skeleton + clap subcommand surface"
```

---

## Task 2: paths module

**Files:**
- Create: `src/paths.rs`
- Modify: `src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/paths.rs`

This module owns where files live on disk: session dirs, socket paths, db paths.

- [ ] **Step 1: Write the failing test**

Add to `src/paths.rs`:
```rust
//! Filesystem layout helpers.

use std::path::PathBuf;

/// Root directory: `~/.agents-connector/` (or `$XDG_DATA_HOME/agents-connector/`).
pub fn root() -> anyhow::Result<PathBuf> {
    if let Ok(override_path) = std::env::var("AGENTS_CONNECTOR_HOME") {
        return Ok(PathBuf::from(override_path));
    }
    let dirs = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    Ok(dirs.home_dir().join(".agents-connector"))
}

pub fn sessions_dir() -> anyhow::Result<PathBuf> {
    Ok(root()?.join("sessions"))
}

pub fn session_dir(session: &str) -> anyhow::Result<PathBuf> {
    Ok(sessions_dir()?.join(session))
}

pub fn session_db(session: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(session)?.join("db.sqlite"))
}

pub fn session_socket(session: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(session)?.join("broker.sock"))
}

pub fn session_pid_file(session: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(session)?.join("broker.pid"))
}

pub fn session_log(session: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(session)?.join("broker.log"))
}

pub fn session_agent_dir(session: &str, agent: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(session)?.join("agents").join(agent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_respects_override_env_var() {
        std::env::set_var("AGENTS_CONNECTOR_HOME", "/tmp/test-ac");
        assert_eq!(root().unwrap(), PathBuf::from("/tmp/test-ac"));
        std::env::remove_var("AGENTS_CONNECTOR_HOME");
    }

    #[test]
    fn session_paths_compose_correctly() {
        std::env::set_var("AGENTS_CONNECTOR_HOME", "/tmp/test-ac");
        assert_eq!(session_dir("demo").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo"));
        assert_eq!(session_db("demo").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo/db.sqlite"));
        assert_eq!(session_socket("demo").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo/broker.sock"));
        assert_eq!(session_agent_dir("demo", "alice").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo/agents/alice"));
        std::env::remove_var("AGENTS_CONNECTOR_HOME");
    }
}
```

- [ ] **Step 2: Wire the module into the library**

Add to `src/lib.rs`:
```rust
pub mod cli;
pub mod paths;
```

- [ ] **Step 3: Run the tests; expect them to pass**

Run: `cargo test --lib paths`
Expected: 2 tests pass.

Note: tests use a shared env var. Either the `serial_test` crate or the fact that they set+unset within a single test makes this safe enough for now. If you see flakiness, add `#[serial_test::serial]` attribute.

- [ ] **Step 4: Commit**

```bash
git add src/paths.rs src/lib.rs
git commit -m "feat: paths module for session filesystem layout"
```

---

## Task 3: SQLite store — schema + agents table

**Files:**
- Create: `src/broker/mod.rs` (just `pub mod store;` for now)
- Create: `src/broker/store.rs`
- Modify: `src/lib.rs`
- Test: `tests/store_test.rs`

The store is the broker's persistence layer. We build it incrementally: agents now, messages/asks/replies in Task 4.

- [ ] **Step 1: Write the failing integration test**

Create `tests/store_test.rs`:
```rust
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
```

- [ ] **Step 2: Run; verify it fails to compile (Store doesn't exist)**

Run: `cargo test --test store_test`
Expected: build fails with "no module `broker`" or "use of undeclared crate or module".

- [ ] **Step 3: Create `src/broker/mod.rs`**

```rust
//! Broker daemon: SQLite store + IPC server.

pub mod store;
```

- [ ] **Step 4: Add `pub mod broker;` to `src/lib.rs`**

After this step, `src/lib.rs` should be:
```rust
pub mod broker;
pub mod cli;
pub mod paths;
```

- [ ] **Step 5: Implement `src/broker/store.rs` minimally to satisfy the agents tests**

```rust
//! SQLite-backed message store for a session.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: i64,
    pub name: String,
    pub cli_kind: String,
    pub token: String,
    pub registered_at: DateTime<Utc>,
    pub removed_at: Option<DateTime<Utc>>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("creating store parent dir {}", parent.display())
            })?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite at {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                cli_kind TEXT NOT NULL,
                token TEXT NOT NULL UNIQUE,
                registered_at TEXT NOT NULL,
                removed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS agents_token_idx ON agents(token);
        "#)?;
        Ok(())
    }

    pub fn register_agent(&self, name: &str, cli_kind: &str) -> Result<String> {
        let token = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agents (name, cli_kind, token, registered_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, cli_kind, token, now],
        ).map_err(|e| match e {
            rusqlite::Error::SqliteFailure(_, Some(s)) if s.contains("agents.name") => {
                anyhow!("agent already exists: {}", name)
            }
            other => anyhow::Error::from(other),
        })?;
        Ok(token)
    }

    pub fn agent_by_token(&self, token: &str) -> Result<Option<Agent>> {
        let conn = self.conn.lock().unwrap();
        let agent = conn.query_row(
            "SELECT id, name, cli_kind, token, registered_at, removed_at FROM agents WHERE token = ?1",
            params![token],
            |row| Ok(Agent {
                id: row.get(0)?,
                name: row.get(1)?,
                cli_kind: row.get(2)?,
                token: row.get(3)?,
                registered_at: row.get::<_, String>(4)?.parse::<DateTime<Utc>>().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                removed_at: row.get::<_, Option<String>>(5)?.map(|s| s.parse::<DateTime<Utc>>()).transpose().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            })
        ).optional()?;
        Ok(agent)
    }

    pub fn agent_by_name(&self, name: &str) -> Result<Option<Agent>> {
        let conn = self.conn.lock().unwrap();
        let agent = conn.query_row(
            "SELECT id, name, cli_kind, token, registered_at, removed_at FROM agents WHERE name = ?1",
            params![name],
            |row| Ok(Agent {
                id: row.get(0)?,
                name: row.get(1)?,
                cli_kind: row.get(2)?,
                token: row.get(3)?,
                registered_at: row.get::<_, String>(4)?.parse::<DateTime<Utc>>().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                removed_at: row.get::<_, Option<String>>(5)?.map(|s| s.parse::<DateTime<Utc>>()).transpose().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            })
        ).optional()?;
        Ok(agent)
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, cli_kind, token, registered_at, removed_at FROM agents WHERE removed_at IS NULL ORDER BY registered_at"
        )?;
        let rows = stmt.query_map([], |row| Ok(Agent {
            id: row.get(0)?,
            name: row.get(1)?,
            cli_kind: row.get(2)?,
            token: row.get(3)?,
            registered_at: row.get::<_, String>(4)?.parse::<DateTime<Utc>>().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            removed_at: row.get::<_, Option<String>>(5)?.map(|s| s.parse::<DateTime<Utc>>()).transpose().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
```

- [ ] **Step 6: Run tests; verify they pass**

Run: `cargo test --test store_test`
Expected: 3 tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/broker/ src/lib.rs tests/store_test.rs
git commit -m "feat: store agents table with register/lookup/list"
```

---

## Task 4: SQLite store — messages, asks, replies

**Files:**
- Modify: `src/broker/store.rs`
- Modify: `tests/store_test.rs`

- [ ] **Step 1: Add the failing tests for messages, asks, and replies**

Append to `tests/store_test.rs`:
```rust
#[test]
fn tells_and_reads_messages() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let store = Store::open(&db).unwrap();

    let _alice = store.register_agent("alice", "claude").unwrap();
    let _bob = store.register_agent("bob", "claude").unwrap();

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

    store.register_agent("alice", "claude").unwrap();
    store.register_agent("bob", "claude").unwrap();
    store.register_agent("carol", "claude").unwrap();

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

    store.register_agent("alice", "claude").unwrap();
    store.register_agent("bob", "claude").unwrap();

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
```

- [ ] **Step 2: Run; expect compilation failure (methods don't exist)**

Run: `cargo test --test store_test`
Expected: errors about missing methods `tell`, `read_messages_for`, `ask`, `post_reply`, `replies_for_ask`, and the `Message`/`Reply` struct fields.

- [ ] **Step 3: Extend the migration to create messages/asks/replies tables**

Replace the `migrate` function body in `src/broker/store.rs` with:
```rust
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS agents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            cli_kind TEXT NOT NULL,
            token TEXT NOT NULL UNIQUE,
            registered_at TEXT NOT NULL,
            removed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS agents_token_idx ON agents(token);

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_agent TEXT NOT NULL,
            to_agent TEXT,                 -- NULL = broadcast
            text TEXT NOT NULL,
            ask_id INTEGER,                -- non-null if this message originates from an ask
            in_reply_to INTEGER,           -- non-null if this message is a reply to an ask
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS messages_to_idx ON messages(to_agent, id);
        CREATE INDEX IF NOT EXISTS messages_from_idx ON messages(from_agent, id);

        CREATE TABLE IF NOT EXISTS asks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_agent TEXT NOT NULL,
            to_agent TEXT NOT NULL,
            text TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS replies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ask_id INTEGER NOT NULL,
            from_agent TEXT NOT NULL,
            text TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (ask_id) REFERENCES asks(id)
        );
        CREATE INDEX IF NOT EXISTS replies_ask_idx ON replies(ask_id);
    "#)?;
    Ok(())
}
```

- [ ] **Step 4: Add the `Message` and `Reply` structs**

Add near the `Agent` struct in `src/broker/store.rs`:
```rust
#[derive(Debug, Clone)]
pub struct Message {
    pub id: i64,
    pub from_name: String,
    pub to_name: Option<String>,
    pub text: String,
    pub ask_id: Option<i64>,
    pub in_reply_to: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub id: i64,
    pub ask_id: i64,
    pub from_name: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 5: Implement `tell` / `read_messages_for`**

Add to `impl Store`:
```rust
pub fn tell(&self, from: &str, to: Option<&str>, text: &str) -> Result<i64> {
    let conn = self.conn.lock().unwrap();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO messages (from_agent, to_agent, text, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![from, to, text, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Returns all messages with id > `since` that are visible to `agent`:
/// either DM'd to them, or broadcast (to_agent IS NULL) and not from them.
pub fn read_messages_for(&self, agent: &str, since: i64) -> Result<Vec<Message>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(r#"
        SELECT id, from_agent, to_agent, text, ask_id, in_reply_to, created_at
        FROM messages
        WHERE id > ?1
          AND (to_agent = ?2 OR (to_agent IS NULL AND from_agent != ?2))
        ORDER BY id
    "#)?;
    let rows = stmt.query_map(params![since, agent], |row| {
        Ok(Message {
            id: row.get(0)?,
            from_name: row.get(1)?,
            to_name: row.get::<_, Option<String>>(2)?,
            text: row.get(3)?,
            ask_id: row.get::<_, Option<i64>>(4)?,
            in_reply_to: row.get::<_, Option<i64>>(5)?,
            created_at: row.get::<_, String>(6)?
                .parse()
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
```

- [ ] **Step 6: Implement `ask` / `post_reply` / `replies_for_ask`**

Add to `impl Store`:
```rust
/// Result of `ask`: the new ask's id and the corresponding message row's id.
pub struct AskResult {
    pub ask_id: i64,
    pub message_id: i64,
}

/// Result of `post_reply`: the new reply's id, the message row id for the reply, and the original asker.
pub struct ReplyResult {
    pub reply_id: i64,
    pub message_id: i64,
    pub original_asker: String,
}

/// Posts an ask. Also writes a corresponding message row (so the recipient sees it via read_messages).
pub fn ask(&self, from: &str, to: &str, text: &str) -> Result<AskResult> {
    let conn = self.conn.lock().unwrap();
    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO asks (from_agent, to_agent, text, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![from, to, text, now],
    )?;
    let ask_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO messages (from_agent, to_agent, text, ask_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![from, to, text, ask_id, now],
    )?;
    let message_id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(AskResult { ask_id, message_id })
}

/// Records a reply for `ask_id` and writes a linking message row from the reply's author to the original asker.
pub fn post_reply(&self, from: &str, ask_id: i64, text: &str) -> Result<ReplyResult> {
    let conn = self.conn.lock().unwrap();
    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    // Look up original asker so the message goes to the right person.
    let original_from: String = tx.query_row(
        "SELECT from_agent FROM asks WHERE id = ?1",
        params![ask_id],
        |row| row.get(0)
    ).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => anyhow!("ask {} not found", ask_id),
        other => anyhow::Error::from(other),
    })?;

    tx.execute(
        "INSERT INTO replies (ask_id, from_agent, text, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![ask_id, from, text, now],
    )?;
    let reply_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO messages (from_agent, to_agent, text, in_reply_to, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![from, original_from, text, ask_id, now],
    )?;
    let message_id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(ReplyResult { reply_id, message_id, original_asker: original_from })
}

pub fn replies_for_ask(&self, ask_id: i64) -> Result<Vec<Reply>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, ask_id, from_agent, text, created_at FROM replies WHERE ask_id = ?1 ORDER BY id"
    )?;
    let rows = stmt.query_map(params![ask_id], |row| {
        Ok(Reply {
            id: row.get(0)?,
            ask_id: row.get(1)?,
            from_name: row.get(2)?,
            text: row.get(3)?,
            created_at: row.get::<_, String>(4)?.parse()
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test --test store_test`
Expected: 6 tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/broker/store.rs tests/store_test.rs
git commit -m "feat: store messages, asks, replies tables and queries"
```

---

## Task 5: ipc module — framing and types

**Files:**
- Create: `src/ipc.rs`
- Modify: `src/lib.rs`

The broker and the shim talk via length-prefixed JSON over a Unix socket. We define a small request/response enum here so both sides can serialize/deserialize from one source of truth.

- [ ] **Step 1: Write the failing test (round-trip)**

Create `src/ipc.rs`:
```rust
//! Length-prefixed JSON IPC protocol.
//!
//! Wire format: 4-byte big-endian length prefix, then UTF-8 JSON body.
//! Used between the broker daemon and (a) the mcp-shim, (b) the launcher.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Auth handshake from a shim — identifies the agent.
    Authenticate { agent_token: String },
    Tell { from: String, to: Option<String>, text: String, urgent: bool },
    Ask { from: String, to: String, text: String },
    PostReply { from: String, ask_id: i64, text: String },
    ReadMessages { agent: String, since: i64 },
    CheckReplies { ask_id: i64 },
    /// Block until at least one reply exists, or timeout.
    WaitForReply { ask_id: i64, timeout_ms: u64 },
    ListAgents,
    /// Subscribe to live message stream (for `tail`).
    SubscribeStream,
    /// Graceful shutdown signal (used by `stop`).
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    AgentInfo { name: String, cli_kind: String },
    TellAck { message_id: i64 },
    AskAck { ask_id: i64 },
    ReplyAck { reply_id: i64 },
    Messages { messages: Vec<MessageDto> },
    Replies { replies: Vec<ReplyDto> },
    Agents { agents: Vec<AgentDto> },
    /// Streamed event (one of many) for SubscribeStream.
    StreamEvent { message: MessageDto },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageDto {
    pub id: i64,
    pub from: String,
    pub to: Option<String>,
    pub text: String,
    pub ask_id: Option<i64>,
    pub in_reply_to: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplyDto {
    pub id: i64,
    pub ask_id: i64,
    pub from: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentDto {
    pub name: String,
    pub cli_kind: String,
}

/// Sync framing helpers (used by the hook subcommand which is one-shot).
pub fn write_frame_sync<W: Write>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = (payload.len() as u32).to_be_bytes();
    w.write_all(&len)?;
    w.write_all(payload)?;
    w.flush()
}

pub fn read_frame_sync<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Async framing helpers (used by the broker server and mcp-shim).
pub async fn write_frame_async<W: AsyncWriteExt + Unpin>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = (payload.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(payload).await?;
    w.flush().await
}

pub async fn read_frame_async<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_request() {
        let req = Request::Tell {
            from: "alice".into(),
            to: Some("bob".into()),
            text: "hi".into(),
            urgent: false,
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let parsed: Request = serde_json::from_slice(&bytes).unwrap();
        match parsed {
            Request::Tell { from, to, text, urgent } => {
                assert_eq!(from, "alice");
                assert_eq!(to.as_deref(), Some("bob"));
                assert_eq!(text, "hi");
                assert!(!urgent);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_response() {
        let resp = Response::Messages {
            messages: vec![MessageDto {
                id: 1,
                from: "alice".into(),
                to: Some("bob".into()),
                text: "hi".into(),
                ask_id: None,
                in_reply_to: None,
                created_at: "2026-05-09T10:00:00Z".into(),
            }],
        };
        let bytes = serde_json::to_vec(&resp).unwrap();
        let parsed: Response = serde_json::from_slice(&bytes).unwrap();
        match parsed {
            Response::Messages { messages } => assert_eq!(messages.len(), 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn sync_framing_round_trip() {
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, b"hello").unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let frame = read_frame_sync(&mut cursor).unwrap();
        assert_eq!(frame, b"hello");
    }
}
```

- [ ] **Step 2: Add to `src/lib.rs`**

```rust
pub mod broker;
pub mod cli;
pub mod ipc;
pub mod paths;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ipc`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ipc.rs src/lib.rs
git commit -m "feat: ipc module with request/response types and frame helpers"
```

---

## Task 6: Broker daemon skeleton

**Files:**
- Create: `src/broker/server.rs`
- Create: `src/broker/handlers.rs`
- Modify: `src/broker/mod.rs`
- Modify: `src/main.rs`
- Test: `tests/broker_ipc_test.rs` (skeleton, real handlers in tasks 7–9)

The broker is a tokio process: listens on a Unix socket, accepts connections, spawns a per-connection handler that reads framed requests and writes framed responses.

- [ ] **Step 1: Write the failing test (server starts and answers Authenticate)**

Create `tests/broker_ipc_test.rs`:
```rust
use agents_connector::broker::store::Store;
use agents_connector::broker::server;
use agents_connector::ipc::{read_frame_async, write_frame_async, Request, Response};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::UnixStream;

async fn spawn_test_broker() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone).await.unwrap();
    });
    // Allow the listener to bind. A short retry loop is more robust than a sleep.
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (tmp, sock)
}

#[tokio::test]
async fn authenticate_with_valid_token_returns_ok() {
    let (_tmp, sock) = spawn_test_broker().await;
    // Pre-register an agent directly via the store so we have a token.
    let store = Store::open(_tmp.path().join("test.sqlite")).unwrap();
    let token = store.register_agent("alice", "claude").unwrap();

    let mut stream = UnixStream::connect(&sock).await.unwrap();
    let req = Request::Authenticate { agent_token: token };
    let bytes = serde_json::to_vec(&req).unwrap();
    write_frame_async(&mut stream, &bytes).await.unwrap();

    let frame = read_frame_async(&mut stream).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::AgentInfo { name, cli_kind } => {
            assert_eq!(name, "alice");
            assert_eq!(cli_kind, "claude");
        }
        other => panic!("unexpected response: {:?}", other),
    }
}
```

Note: this test re-opens the same SQLite file from two `Store::open` calls. With WAL mode that's fine, but we'll fix the test fixture in a later task by sharing the broker's `Arc<Store>` directly. For now we accept the duplication.

- [ ] **Step 2: Run; expect compilation failure (no server module)**

Run: `cargo test --test broker_ipc_test`
Expected: build error: "no module `server`".

- [ ] **Step 3: Implement `src/broker/server.rs`**

```rust
use crate::broker::handlers;
use crate::broker::store::Store;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::net::UnixListener;
use tracing::{error, info};

/// Run the broker server, listening on `socket_path` and using `store` for persistence.
/// Returns when a Shutdown request is received.
pub async fn serve(store: Arc<Store>, socket_path: &Path) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    info!("broker listening on {}", socket_path.display());

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let store = Arc::clone(&store);
                        let shutdown_tx = shutdown_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, store, shutdown_tx).await {
                                error!("connection error: {:#}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("accept error: {:#}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                info!("broker shutting down");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    store: Arc<Store>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<()> {
    loop {
        let frame = match read_frame_async(&mut stream).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let request: Request = serde_json::from_slice(&frame)?;
        let response = handlers::dispatch(request, &store, &shutdown_tx).await;
        let bytes = serde_json::to_vec(&response)?;
        write_frame_async(&mut stream, &bytes).await?;
    }
}
```

- [ ] **Step 4: Implement `src/broker/handlers.rs` with stubs that handle `Authenticate` only (others return Error)**

```rust
use crate::broker::store::Store;
use crate::ipc::{Request, Response};
use std::sync::Arc;

pub async fn dispatch(
    req: Request,
    store: &Arc<Store>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) -> Response {
    match req {
        Request::Authenticate { agent_token } => match store.agent_by_token(&agent_token) {
            Ok(Some(agent)) => Response::AgentInfo { name: agent.name, cli_kind: agent.cli_kind },
            Ok(None) => Response::Error { message: "unknown agent token".into() },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Shutdown => {
            let _ = shutdown_tx.send(());
            Response::Ok
        }
        _ => Response::Error { message: "not yet implemented".into() },
    }
}
```

- [ ] **Step 5: Update `src/broker/mod.rs`**

```rust
pub mod handlers;
pub mod server;
pub mod store;
```

- [ ] **Step 6: Wire the `Broker` subcommand in `src/main.rs`**

Replace the `Command::Broker` arm:
```rust
Command::Broker { socket, db } => {
    use agents_connector::broker::{server, store::Store};
    use std::sync::Arc;
    let store = Arc::new(Store::open(&db)?);
    server::serve(store, &socket).await?;
    Ok(())
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test --test broker_ipc_test`
Expected: the `authenticate_with_valid_token_returns_ok` test passes.

- [ ] **Step 8: Commit**

```bash
git add src/broker/ src/main.rs tests/broker_ipc_test.rs
git commit -m "feat: broker daemon accepting auth handshake"
```

---

## Task 7: Broker handlers — register, list_agents

**Note:** "register" the IPC request (used by the launcher when adding an agent) is distinct from `Store::register_agent`. The launcher calls the broker over IPC to ask the broker to register a new agent in its store; the broker returns the token, which the launcher then bakes into the spawned shim's command line.

**Files:**
- Modify: `src/ipc.rs` (add `RegisterAgent` request and `RegisterAck` response)
- Modify: `src/broker/handlers.rs`
- Modify: `tests/broker_ipc_test.rs`

- [ ] **Step 1: Extend the ipc enums**

Add to `Request`:
```rust
    RegisterAgent { name: String, cli_kind: String },
```

Add to `Response`:
```rust
    RegisterAck { agent_token: String },
```

- [ ] **Step 2: Add the failing test**

Append to `tests/broker_ipc_test.rs`:
```rust
#[tokio::test]
async fn register_agent_returns_token_and_list_includes_it() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut stream = UnixStream::connect(&sock).await.unwrap();
    let req = Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into() };
    write_frame_async(&mut stream, &serde_json::to_vec(&req).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut stream).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    let token = match resp {
        Response::RegisterAck { agent_token } => agent_token,
        other => panic!("unexpected: {:?}", other),
    };
    assert!(!token.is_empty());

    let req = Request::ListAgents;
    write_frame_async(&mut stream, &serde_json::to_vec(&req).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut stream).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::Agents { agents } => {
            assert_eq!(agents.len(), 1);
            assert_eq!(agents[0].name, "alice");
        }
        other => panic!("unexpected: {:?}", other),
    }
}
```

- [ ] **Step 3: Run; expect failure**

Run: `cargo test --test broker_ipc_test register_agent_returns_token`
Expected: test fails because the handlers return `not yet implemented`.

- [ ] **Step 4: Implement the handlers**

In `src/broker/handlers.rs`, add to the match:
```rust
Request::RegisterAgent { name, cli_kind } => match store.register_agent(&name, &cli_kind) {
    Ok(token) => Response::RegisterAck { agent_token: token },
    Err(e) => Response::Error { message: format!("{:#}", e) },
},
Request::ListAgents => match store.list_agents() {
    Ok(agents) => Response::Agents {
        agents: agents.into_iter().map(|a| crate::ipc::AgentDto {
            name: a.name,
            cli_kind: a.cli_kind,
        }).collect(),
    },
    Err(e) => Response::Error { message: format!("{:#}", e) },
},
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test broker_ipc_test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/ipc.rs src/broker/handlers.rs tests/broker_ipc_test.rs
git commit -m "feat: broker handlers for register and list_agents"
```

---

## Task 8: Broker handlers — tell, read_messages

**Files:**
- Modify: `src/broker/handlers.rs`
- Modify: `tests/broker_ipc_test.rs`

- [ ] **Step 1: Add the failing test**

Append to `tests/broker_ipc_test.rs`:
```rust
#[tokio::test]
async fn tell_and_read_messages_round_trip() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into() }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "bob".into(), cli_kind: "claude".into() }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::Tell {
        from: "alice".into(), to: Some("bob".into()), text: "hello".into(), urgent: false,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    let msg_id = match resp {
        Response::TellAck { message_id } => message_id,
        other => panic!("unexpected: {:?}", other),
    };
    assert!(msg_id > 0);

    write_frame_async(&mut s, &serde_json::to_vec(&Request::ReadMessages {
        agent: "bob".into(), since: 0,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::Messages { messages } => {
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].text, "hello");
            assert_eq!(messages[0].from, "alice");
        }
        other => panic!("unexpected: {:?}", other),
    }
}
```

- [ ] **Step 2: Run; expect failure**

Run: `cargo test --test broker_ipc_test tell_and_read`
Expected: fails (handlers return `not yet implemented`).

- [ ] **Step 3: Implement the handlers**

Add to the match in `src/broker/handlers.rs`:
```rust
Request::Tell { from, to, text, urgent: _ } => {
    // urgent not handled in v1 (Phase 1) — wake mechanism is Phase 3.
    match store.tell(&from, to.as_deref(), &text) {
        Ok(id) => Response::TellAck { message_id: id },
        Err(e) => Response::Error { message: format!("{:#}", e) },
    }
}
Request::ReadMessages { agent, since } => match store.read_messages_for(&agent, since) {
    Ok(msgs) => Response::Messages {
        messages: msgs.into_iter().map(|m| crate::ipc::MessageDto {
            id: m.id,
            from: m.from_name,
            to: m.to_name,
            text: m.text,
            ask_id: m.ask_id,
            in_reply_to: m.in_reply_to,
            created_at: m.created_at.to_rfc3339(),
        }).collect(),
    },
    Err(e) => Response::Error { message: format!("{:#}", e) },
},
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test broker_ipc_test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/broker/handlers.rs tests/broker_ipc_test.rs
git commit -m "feat: broker handlers for tell and read_messages"
```

---

## Task 9: Broker handlers — ask, post_reply, check_replies, wait_for_reply

**Files:**
- Modify: `src/broker/server.rs` (add reply notifier so wait_for_reply can wake)
- Modify: `src/broker/handlers.rs`
- Modify: `tests/broker_ipc_test.rs`

The reply notifier is a tokio broadcast channel scoped to ask-ids. When `post_reply` is called, the broker fires an event for that ask_id; outstanding `wait_for_reply` calls listening on that ask_id wake up.

- [ ] **Step 1: Add a `BrokerCtx` shared between handlers**

Refactor `src/broker/server.rs` to introduce a `BrokerCtx`:
```rust
use std::collections::HashMap;
use tokio::sync::{broadcast, Mutex};

pub struct BrokerCtx {
    pub store: Arc<Store>,
    pub reply_notifiers: Mutex<HashMap<i64, broadcast::Sender<()>>>,
    pub shutdown_tx: broadcast::Sender<()>,
}

impl BrokerCtx {
    pub fn new(store: Arc<Store>, shutdown_tx: broadcast::Sender<()>) -> Self {
        Self {
            store,
            reply_notifiers: Mutex::new(HashMap::new()),
            shutdown_tx,
        }
    }

    pub async fn notifier_for(&self, ask_id: i64) -> broadcast::Sender<()> {
        let mut map = self.reply_notifiers.lock().await;
        map.entry(ask_id)
            .or_insert_with(|| broadcast::channel::<()>(1).0)
            .clone()
    }

    pub async fn fire_reply(&self, ask_id: i64) {
        let map = self.reply_notifiers.lock().await;
        if let Some(tx) = map.get(&ask_id) {
            let _ = tx.send(());
        }
    }
}
```

Update `serve` to create the ctx and pass it down:
```rust
pub async fn serve(store: Arc<Store>, socket_path: &Path) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    info!("broker listening on {}", socket_path.display());

    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
    let ctx = Arc::new(BrokerCtx::new(store, shutdown_tx.clone()));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let ctx = Arc::clone(&ctx);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, ctx).await {
                                error!("connection error: {:#}", e);
                            }
                        });
                    }
                    Err(e) => error!("accept error: {:#}", e),
                }
            }
            _ = shutdown_rx.recv() => {
                info!("broker shutting down");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    ctx: Arc<BrokerCtx>,
) -> Result<()> {
    loop {
        let frame = match read_frame_async(&mut stream).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let request: Request = serde_json::from_slice(&frame)?;
        let response = handlers::dispatch(request, &ctx).await;
        let bytes = serde_json::to_vec(&response)?;
        write_frame_async(&mut stream, &bytes).await?;
    }
}
```

- [ ] **Step 2: Update `src/broker/handlers.rs` to accept `BrokerCtx`**

Replace the `dispatch` signature and all existing handler bodies that reference `store`/`shutdown_tx` to go through `ctx`:
```rust
use crate::broker::server::BrokerCtx;
use crate::ipc::{Request, Response};
use std::sync::Arc;

pub async fn dispatch(req: Request, ctx: &Arc<BrokerCtx>) -> Response {
    match req {
        Request::Authenticate { agent_token } => match ctx.store.agent_by_token(&agent_token) {
            Ok(Some(agent)) => Response::AgentInfo { name: agent.name, cli_kind: agent.cli_kind },
            Ok(None) => Response::Error { message: "unknown agent token".into() },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::RegisterAgent { name, cli_kind } => match ctx.store.register_agent(&name, &cli_kind) {
            Ok(token) => Response::RegisterAck { agent_token: token },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::ListAgents => match ctx.store.list_agents() {
            Ok(agents) => Response::Agents {
                agents: agents.into_iter().map(|a| crate::ipc::AgentDto {
                    name: a.name,
                    cli_kind: a.cli_kind,
                }).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Tell { from, to, text, urgent: _ } => match ctx.store.tell(&from, to.as_deref(), &text) {
            Ok(id) => Response::TellAck { message_id: id },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::ReadMessages { agent, since } => match ctx.store.read_messages_for(&agent, since) {
            Ok(msgs) => Response::Messages {
                messages: msgs.into_iter().map(message_to_dto).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Ask { from, to, text } => match ctx.store.ask(&from, &to, &text) {
            Ok(result) => Response::AskAck { ask_id: result.ask_id },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::PostReply { from, ask_id, text } => match ctx.store.post_reply(&from, ask_id, &text) {
            Ok(result) => {
                ctx.fire_reply(ask_id).await;
                Response::ReplyAck { reply_id: result.reply_id }
            }
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::CheckReplies { ask_id } => match ctx.store.replies_for_ask(ask_id) {
            Ok(replies) => Response::Replies {
                replies: replies.into_iter().map(|r| crate::ipc::ReplyDto {
                    id: r.id,
                    ask_id: r.ask_id,
                    from: r.from_name,
                    text: r.text,
                    created_at: r.created_at.to_rfc3339(),
                }).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::WaitForReply { ask_id, timeout_ms } => {
            // Subscribe BEFORE checking, so a reply that arrives in between still wakes us.
            let notifier = ctx.notifier_for(ask_id).await;
            let mut rx = notifier.subscribe();
            // Cheap path: there might already be replies.
            if let Ok(replies) = ctx.store.replies_for_ask(ask_id) {
                if !replies.is_empty() {
                    return Response::Replies {
                        replies: replies.into_iter().map(|r| crate::ipc::ReplyDto {
                            id: r.id,
                            ask_id: r.ask_id,
                            from: r.from_name,
                            text: r.text,
                            created_at: r.created_at.to_rfc3339(),
                        }).collect(),
                    };
                }
            }
            // Wait for either the notifier or timeout.
            let timeout = std::time::Duration::from_millis(timeout_ms);
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(_) => match ctx.store.replies_for_ask(ask_id) {
                    Ok(replies) => Response::Replies {
                        replies: replies.into_iter().map(|r| crate::ipc::ReplyDto {
                            id: r.id,
                            ask_id: r.ask_id,
                            from: r.from_name,
                            text: r.text,
                            created_at: r.created_at.to_rfc3339(),
                        }).collect(),
                    },
                    Err(e) => Response::Error { message: format!("{:#}", e) },
                },
                Err(_) => Response::Replies { replies: vec![] }, // timeout = empty
            }
        }
        Request::Shutdown => {
            let _ = ctx.shutdown_tx.send(());
            Response::Ok
        }
        Request::SubscribeStream => {
            // Implemented in Task 16. For now: error.
            Response::Error { message: "subscribe_stream not implemented".into() }
        }
    }
}

fn message_to_dto(m: crate::broker::store::Message) -> crate::ipc::MessageDto {
    crate::ipc::MessageDto {
        id: m.id,
        from: m.from_name,
        to: m.to_name,
        text: m.text,
        ask_id: m.ask_id,
        in_reply_to: m.in_reply_to,
        created_at: m.created_at.to_rfc3339(),
    }
}
```

- [ ] **Step 3: Add the failing test for ask/reply**

Append to `tests/broker_ipc_test.rs`:
```rust
#[tokio::test]
async fn ask_reply_check_round_trip() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into() }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "bob".into(), cli_kind: "claude".into() }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(), to: "bob".into(), text: "still there?".into(),
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let ask_id = match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::AskAck { ask_id } => ask_id,
        other => panic!("unexpected: {:?}", other),
    };

    write_frame_async(&mut s, &serde_json::to_vec(&Request::PostReply {
        from: "bob".into(), ask_id, text: "yes".into(),
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::ReplyAck { .. } => {}
        other => panic!("unexpected: {:?}", other),
    }

    write_frame_async(&mut s, &serde_json::to_vec(&Request::CheckReplies { ask_id }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::Replies { replies } => {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].text, "yes");
            assert_eq!(replies[0].from, "bob");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn wait_for_reply_blocks_then_returns() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into() }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "bob".into(), cli_kind: "claude".into() }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(), to: "bob".into(), text: "ready?".into(),
    }).unwrap()).await.unwrap();
    let ask_id = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::AskAck { ask_id } => ask_id,
        other => panic!("unexpected: {:?}", other),
    };

    // Spawn a writer that posts a reply after 200ms.
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let mut s2 = UnixStream::connect(&sock_clone).await.unwrap();
        write_frame_async(&mut s2, &serde_json::to_vec(&Request::PostReply {
            from: "bob".into(), ask_id, text: "go".into(),
        }).unwrap()).await.unwrap();
        let _ = read_frame_async(&mut s2).await.unwrap();
    });

    write_frame_async(&mut s, &serde_json::to_vec(&Request::WaitForReply { ask_id, timeout_ms: 2000 }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::Replies { replies } => {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].text, "go");
        }
        other => panic!("unexpected: {:?}", other),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test broker_ipc_test`
Expected: all 5 tests pass (1 from earlier + 4 from this task's lineage).

- [ ] **Step 5: Commit**

```bash
git add src/broker/ tests/broker_ipc_test.rs
git commit -m "feat: broker ask/reply handlers with wait_for_reply notifier"
```

---

## Task 10: mcp-shim subcommand (stdio MCP server bridging to broker)

This is the trickiest task in the plan. The shim runs as a subprocess of the agent's CLI; the CLI talks to it over stdio MCP. The shim translates each MCP tool call into a broker IPC request.

**Files:**
- Create: `src/shim/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Sketch the public API**

The shim subcommand:
1. Connects to the broker via Unix socket.
2. Sends `Authenticate { agent_token }` and confirms its identity.
3. Starts an MCP server on stdio (using `rmcp`), exposing tools that map to broker IPC.
4. Stays alive until stdio EOF (i.e., the parent CLI closes).

Tools to expose (named to match the broker's request kinds):
- `tell(to: Option<String>, text: String, urgent: bool) -> i64` (returns message_id)
- `ask(to: String, text: String) -> i64` (returns ask_id)
- `wait_for_reply(ask_id: i64, timeout_ms: u64) -> Vec<ReplyDto>`
- `check_replies(ask_id: i64) -> Vec<ReplyDto>`
- `read_messages(since: i64) -> Vec<MessageDto>` (the shim injects its own agent name)
- `post_reply(ask_id: i64, text: String) -> i64`
- `list_agents() -> Vec<AgentDto>`

- [ ] **Step 2: Verify rmcp's tool-server pattern**

Open https://docs.rs/rmcp/latest/rmcp/ and find the recommended pattern for defining a tool server. As of rmcp ~0.1.x, this is typically a struct that derives `#[tool_router]` (or similar), with `#[tool(...)]`-annotated async methods. **Confirm the exact pattern at execution time** and adjust the code below.

If rmcp's API differs significantly from what's shown below, update Step 3's code to match. The contract — seven tools each translating to one IPC request — does not change.

- [ ] **Step 3: Implement the shim**

Create `src/shim/mod.rs`:
```rust
//! MCP shim: stdio MCP server that bridges to the broker via Unix IPC.
//!
//! The shim is launched as `agents-connector mcp-shim --socket <path> --agent-token <T>`.
//! It is the MCP server that the CLI (Claude Code, etc.) connects to over stdio.

use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::Mutex;

pub struct BrokerClient {
    stream: Mutex<UnixStream>,
    pub agent_name: String,
}

impl BrokerClient {
    pub async fn connect(socket: &PathBuf, agent_token: &str) -> Result<Self> {
        let mut stream = UnixStream::connect(socket).await
            .with_context(|| format!("connecting to broker at {}", socket.display()))?;

        let req = Request::Authenticate { agent_token: agent_token.to_string() };
        write_frame_async(&mut stream, &serde_json::to_vec(&req)?).await?;
        let frame = read_frame_async(&mut stream).await?;
        let resp: Response = serde_json::from_slice(&frame)?;
        let agent_name = match resp {
            Response::AgentInfo { name, .. } => name,
            Response::Error { message } => return Err(anyhow!("auth failed: {}", message)),
            other => return Err(anyhow!("unexpected auth response: {:?}", other)),
        };

        Ok(Self {
            stream: Mutex::new(stream),
            agent_name,
        })
    }

    pub async fn request(&self, req: Request) -> Result<Response> {
        let mut s = self.stream.lock().await;
        write_frame_async(&mut *s, &serde_json::to_vec(&req)?).await?;
        let frame = read_frame_async(&mut *s).await?;
        let resp: Response = serde_json::from_slice(&frame)?;
        Ok(resp)
    }
}

/// MCP tool server: every method maps to one IPC request and unpacks one IPC response.
///
/// Note on rmcp API: `rmcp` exposes an attribute macro pair (`#[tool_router]` + `#[tool]`)
/// for declaring tool servers, plus `ServiceExt::serve` to bind a transport. The exact
/// names may shift between minor versions. If your rmcp version uses different names
/// (e.g. `tool_handler`, `serve_stdio`), rename the attributes and the service-binding
/// call accordingly — the seven tool-method bodies do NOT need to change.
#[derive(Clone)]
pub struct Shim {
    client: Arc<BrokerClient>,
}

#[rmcp::tool_router]
impl Shim {
    #[rmcp::tool(description = "Send a fire-and-forget message. `to` is the recipient agent's name, or omit for a broadcast.")]
    async fn tell(&self, to: Option<String>, text: String, urgent: Option<bool>) -> Result<i64, String> {
        let resp = self.client.request(Request::Tell {
            from: self.client.agent_name.clone(),
            to,
            text,
            urgent: urgent.unwrap_or(false),
        }).await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::TellAck { message_id } => Ok(message_id),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }

    #[rmcp::tool(description = "Ask another agent a question. Returns an ask_id you can use with wait_for_reply or check_replies.")]
    async fn ask(&self, to: String, text: String) -> Result<i64, String> {
        let resp = self.client.request(Request::Ask {
            from: self.client.agent_name.clone(),
            to,
            text,
        }).await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::AskAck { ask_id } => Ok(ask_id),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }

    #[rmcp::tool(description = "Block until at least one reply to `ask_id` arrives, or `timeout_ms` elapses. Returns all replies (empty on timeout).")]
    async fn wait_for_reply(&self, ask_id: i64, timeout_ms: u64) -> Result<serde_json::Value, String> {
        let resp = self.client.request(Request::WaitForReply { ask_id, timeout_ms })
            .await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::Replies { replies } => Ok(serde_json::to_value(replies).unwrap()),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }

    #[rmcp::tool(description = "Non-blocking poll: return all replies recorded so far for `ask_id`.")]
    async fn check_replies(&self, ask_id: i64) -> Result<serde_json::Value, String> {
        let resp = self.client.request(Request::CheckReplies { ask_id })
            .await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::Replies { replies } => Ok(serde_json::to_value(replies).unwrap()),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }

    #[rmcp::tool(description = "Fetch all messages addressed to you (or broadcast) with id greater than `since`. Use the highest `id` from the result as the next `since`.")]
    async fn read_messages(&self, since: i64) -> Result<serde_json::Value, String> {
        let resp = self.client.request(Request::ReadMessages {
            agent: self.client.agent_name.clone(),
            since,
        }).await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::Messages { messages } => Ok(serde_json::to_value(messages).unwrap()),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }

    #[rmcp::tool(description = "Reply to an outstanding ask. Use `ask_id` from a message you received with a non-null `ask_id` field.")]
    async fn post_reply(&self, ask_id: i64, text: String) -> Result<i64, String> {
        let resp = self.client.request(Request::PostReply {
            from: self.client.agent_name.clone(),
            ask_id,
            text,
        }).await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::ReplyAck { reply_id } => Ok(reply_id),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }

    #[rmcp::tool(description = "List all agents currently registered in this session.")]
    async fn list_agents(&self) -> Result<serde_json::Value, String> {
        let resp = self.client.request(Request::ListAgents)
            .await.map_err(|e| format!("{:#}", e))?;
        match resp {
            Response::Agents { agents } => Ok(serde_json::to_value(agents).unwrap()),
            Response::Error { message } => Err(message),
            other => Err(format!("unexpected: {:?}", other)),
        }
    }
}

/// Run the MCP stdio server, translating tool calls to broker IPC.
pub async fn run(socket: PathBuf, agent_token: String) -> Result<()> {
    let client = Arc::new(BrokerClient::connect(&socket, &agent_token).await?);
    tracing::info!(agent = %client.agent_name, "shim authenticated");

    let shim = Shim { client };

    // Bind the rmcp tool server to stdio. Verify the exact ServiceExt method
    // name in your rmcp version — common names are `serve`, `serve_stdio`,
    // or `into_service().run_stdio()`. The shim instance carries our tool methods.
    use rmcp::ServiceExt;
    let server = shim.serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}
```

- [ ] **Step 4: Wire the subcommand in `src/main.rs`**

Replace the `Command::McpShim` arm:
```rust
Command::McpShim { socket, agent_token } => {
    agents_connector::shim::run(socket, agent_token).await
}
```

- [ ] **Step 5: Add to `src/lib.rs`**

```rust
pub mod broker;
pub mod cli;
pub mod ipc;
pub mod paths;
pub mod shim;
```

- [ ] **Step 6: Verify it compiles; adjust rmcp attribute / service-binding names if needed**

Run: `cargo build`
Expected: builds cleanly.

If you see errors about `#[tool_router]` or `#[tool]` not being found, your rmcp version uses different attribute names — check `cargo doc --open -p rmcp` for the current macros and rename. If `ServiceExt::serve` / `rmcp::transport::stdio()` don't exist, look for `serve_stdio`, `Service::run`, or similar in the docs and substitute. Do NOT change the seven tool method bodies — only the rmcp boilerplate around them.

- [ ] **Step 7: Commit**

```bash
git add src/shim/ src/lib.rs src/main.rs
git commit -m "feat: mcp-shim bridging stdio MCP to broker IPC"
```

---

## Task 11: tmux module — command wrappers

**Files:**
- Create: `src/tmux.rs`
- Modify: `src/lib.rs`

Thin wrappers around shelling out to `tmux`. We keep this isolated so it's easy to mock or swap.

- [ ] **Step 1: Write `src/tmux.rs`**

```rust
//! Thin wrappers around tmux CLI commands.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn has_session(name: &str) -> Result<bool> {
    let status = Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("running `tmux has-session`")?;
    Ok(status.success())
}

pub fn new_detached_session(name: &str, workdir: Option<&PathBuf>) -> Result<()> {
    let mut cmd = Command::new("tmux");
    cmd.args(["new-session", "-d", "-s", name]);
    if let Some(d) = workdir {
        cmd.args(["-c", &d.to_string_lossy()]);
    }
    let status = cmd.status().context("running `tmux new-session`")?;
    if !status.success() {
        anyhow::bail!("tmux new-session failed");
    }
    Ok(())
}

pub fn split_window_below(session: &str, percent: u32, command: &str) -> Result<()> {
    let target = format!("{}:0", session);
    let status = Command::new("tmux")
        .args(["split-window", "-t", &target, "-v", "-p", &percent.to_string(), command])
        .status()
        .context("running `tmux split-window`")?;
    if !status.success() {
        anyhow::bail!("tmux split-window failed");
    }
    Ok(())
}

pub fn new_window(session: &str, name: &str, env: &[(&str, &str)], command: &str) -> Result<()> {
    let mut cmd = Command::new("tmux");
    cmd.args(["new-window", "-t", session, "-n", name]);
    for (k, v) in env {
        cmd.args(["-e", &format!("{}={}", k, v)]);
    }
    cmd.arg(command);
    let status = cmd.status().context("running `tmux new-window`")?;
    if !status.success() {
        anyhow::bail!("tmux new-window failed");
    }
    Ok(())
}

pub fn kill_session(name: &str) -> Result<()> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .status()
        .context("running `tmux kill-session`")?;
    Ok(())
}

pub fn attach_session(name: &str) -> Result<()> {
    // Replace current process with tmux attach; this never returns on success.
    use std::os::unix::process::CommandExt;
    let err = Command::new("tmux").args(["attach-session", "-t", name]).exec();
    Err(anyhow::Error::from(err))
}

/// Returns the current tmux session name from $TMUX, if running inside tmux.
pub fn current_session() -> Option<String> {
    let tmux = std::env::var("TMUX").ok()?;
    // $TMUX format: <socket-path>,<pid>,<session-id>
    // We need to ask tmux for the session name explicitly.
    let _ = tmux;
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#S"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}
```

- [ ] **Step 2: Add to `src/lib.rs`**

```rust
pub mod broker;
pub mod cli;
pub mod ipc;
pub mod paths;
pub mod shim;
pub mod tmux;
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: compiles.

No unit tests for this module — it's a thin shell wrapper. End-to-end validation comes in Task 21.

- [ ] **Step 4: Commit**

```bash
git add src/tmux.rs src/lib.rs
git commit -m "feat: tmux command wrappers"
```

---

## Task 12: subcommand — start

**Files:**
- Create: `src/subcommands/mod.rs`
- Create: `src/subcommands/start.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

`start` creates the session dir, launches the broker as a detached child process, creates a tmux session with a tail pane, and attaches.

- [ ] **Step 1: Implement `src/subcommands/start.rs`**

```rust
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn run(session: &str, workdir: Option<PathBuf>) -> Result<()> {
    let session_dir = paths::session_dir(session)?;
    if session_dir.exists() {
        anyhow::bail!(
            "session `{}` already exists at {}\n\
             Use `agents-connector resume {}` to bring it back, or pick a different name.",
            session, session_dir.display(), session
        );
    }
    if tmux::has_session(session)? {
        anyhow::bail!(
            "tmux session `{}` already exists. Pick a different name or kill the existing tmux session.",
            session
        );
    }
    std::fs::create_dir_all(&session_dir)?;

    let db = paths::session_db(session)?;
    let socket = paths::session_socket(session)?;
    let pid_file = paths::session_pid_file(session)?;
    let log = paths::session_log(session)?;

    // Spawn the broker daemon detached.
    let exe = std::env::current_exe()?;
    let log_file = std::fs::File::create(&log)?;
    let log_file_err = log_file.try_clone()?;
    let child = Command::new(&exe)
        .args([
            "broker",
            "--socket", &socket.to_string_lossy(),
            "--db", &db.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .context("spawning broker daemon")?;

    std::fs::write(&pid_file, child.id().to_string())?;

    // Wait for the socket to appear (broker is up).
    let mut ok = false;
    for _ in 0..200 {
        if socket.exists() { ok = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if !ok {
        anyhow::bail!("broker failed to start within 5 seconds; check {}", log.display());
    }

    // Create tmux session with a tail pane.
    tmux::new_detached_session(session, workdir.as_ref())?;
    let tail_command = format!(
        "{} tail {}",
        exe.to_string_lossy(),
        session
    );
    tmux::split_window_below(session, 25, &tail_command)?;

    println!("session `{}` started.", session);
    println!("attach with: agents-connector attach {}", session);

    // Attach the user's terminal.
    tmux::attach_session(session)?;
    Ok(())
}
```

- [ ] **Step 2: Create `src/subcommands/mod.rs`**

```rust
pub mod start;
```

- [ ] **Step 3: Add to `src/lib.rs`**

```rust
pub mod broker;
pub mod cli;
pub mod ipc;
pub mod paths;
pub mod shim;
pub mod subcommands;
pub mod tmux;
```

- [ ] **Step 4: Wire in `src/main.rs`**

Replace the `Command::Start` arm:
```rust
Command::Start { session } => {
    agents_connector::subcommands::start::run(&session, None)
}
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build`
Expected: compiles.

No automated test here — this command shells out to tmux and spawns a daemon. We'll smoke-test it manually in Task 21.

- [ ] **Step 6: Commit**

```bash
git add src/subcommands/ src/lib.rs src/main.rs
git commit -m "feat: start subcommand spawns broker and tmux session"
```

---

## Task 13: subcommand — list

**Files:**
- Create: `src/subcommands/list.rs`
- Modify: `src/subcommands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement**

Create `src/subcommands/list.rs`:
```rust
use crate::paths;
use anyhow::Result;
use std::fs;

pub fn run() -> Result<()> {
    let dir = paths::sessions_dir()?;
    if !dir.exists() {
        println!("no sessions yet.");
        return Ok(());
    }
    let mut sessions: Vec<String> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    sessions.sort();

    if sessions.is_empty() {
        println!("no sessions yet.");
        return Ok(());
    }

    println!("{:<20} {:<10} {:<10}", "NAME", "STATUS", "PID");
    for s in sessions {
        let pid_file = paths::session_pid_file(&s)?;
        let (status, pid) = match fs::read_to_string(&pid_file) {
            Ok(p) => {
                let pid: i32 = p.trim().parse().unwrap_or(0);
                if pid > 0 && process_alive(pid) {
                    ("running".to_string(), p.trim().to_string())
                } else {
                    ("stopped".to_string(), "—".to_string())
                }
            }
            Err(_) => ("stopped".to_string(), "—".to_string()),
        };
        println!("{:<20} {:<10} {:<10}", s, status, pid);
    }
    Ok(())
}

fn process_alive(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid), None).is_ok()
}
```

- [ ] **Step 2: Add to `src/subcommands/mod.rs`**

```rust
pub mod list;
pub mod start;
```

- [ ] **Step 3: Wire in `src/main.rs`**

```rust
Command::List => agents_connector::subcommands::list::run(),
```

- [ ] **Step 4: Build + smoke test**

Run: `cargo build && cargo run -- list`
Expected: build succeeds. `list` prints "no sessions yet." (or a table if `start` has been run).

- [ ] **Step 5: Commit**

```bash
git add src/subcommands/ src/main.rs
git commit -m "feat: list subcommand"
```

---

## Task 14: subcommand — stop

**Files:**
- Create: `src/subcommands/stop.rs`
- Modify: `src/subcommands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement**

Create `src/subcommands/stop.rs`:
```rust
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::net::UnixStream;

pub async fn run(session: &str, kill_tmux: bool) -> Result<()> {
    let socket = paths::session_socket(session)?;
    if !socket.exists() {
        println!("session `{}` not running.", session);
    } else {
        // Send Shutdown over IPC.
        let mut s = UnixStream::connect(&socket).await
            .with_context(|| format!("connecting to broker for session `{}`", session))?;
        write_frame_async(&mut s, &serde_json::to_vec(&Request::Shutdown)?).await?;
        let frame = read_frame_async(&mut s).await?;
        let _: Response = serde_json::from_slice(&frame)?;

        // Wait for socket to disappear.
        for _ in 0..200 {
            if !socket.exists() { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    let pid_file = paths::session_pid_file(session)?;
    if pid_file.exists() {
        std::fs::remove_file(&pid_file)?;
    }

    if kill_tmux && tmux::has_session(session)? {
        tmux::kill_session(session)?;
    }

    println!("session `{}` stopped.", session);
    Ok(())
}
```

- [ ] **Step 2: Add to `src/subcommands/mod.rs`**

```rust
pub mod list;
pub mod start;
pub mod stop;
```

- [ ] **Step 3: Wire in `src/main.rs`**

```rust
Command::Stop { session, kill_tmux } => {
    agents_connector::subcommands::stop::run(&session, kill_tmux).await
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds.

- [ ] **Step 5: Commit**

```bash
git add src/subcommands/ src/main.rs
git commit -m "feat: stop subcommand sends graceful shutdown to broker"
```

---

## Task 15: subcommand — attach

**Files:**
- Create: `src/subcommands/attach.rs`
- Modify: `src/subcommands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement**

Create `src/subcommands/attach.rs`:
```rust
use crate::tmux;
use anyhow::{bail, Result};

pub fn run(session: &str) -> Result<()> {
    if !tmux::has_session(session)? {
        bail!("no tmux session `{}`. Use `agents-connector start {}` to create one.", session, session);
    }
    tmux::attach_session(session)?;
    Ok(())
}
```

- [ ] **Step 2: Add to `src/subcommands/mod.rs`**

```rust
pub mod attach;
pub mod list;
pub mod start;
pub mod stop;
```

- [ ] **Step 3: Wire in `src/main.rs`**

```rust
Command::Attach { session } => agents_connector::subcommands::attach::run(&session),
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds.

- [ ] **Step 5: Commit**

```bash
git add src/subcommands/ src/main.rs
git commit -m "feat: attach subcommand"
```

---

## Task 16: subcommand — tail (with broker SubscribeStream support)

**Files:**
- Modify: `src/broker/server.rs` (add a stream broadcast)
- Modify: `src/broker/handlers.rs` (implement `SubscribeStream`)
- Create: `src/subcommands/tail.rs`
- Modify: `src/subcommands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add a message broadcast channel to `BrokerCtx`**

Modify `src/broker/server.rs`:
```rust
use crate::ipc::MessageDto;

pub struct BrokerCtx {
    pub store: Arc<Store>,
    pub reply_notifiers: Mutex<HashMap<i64, broadcast::Sender<()>>>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub message_stream: broadcast::Sender<MessageDto>,
}

impl BrokerCtx {
    pub fn new(store: Arc<Store>, shutdown_tx: broadcast::Sender<()>) -> Self {
        let (msg_tx, _) = broadcast::channel::<MessageDto>(256);
        Self {
            store,
            reply_notifiers: Mutex::new(HashMap::new()),
            shutdown_tx,
            message_stream: msg_tx,
        }
    }
    // ... existing methods unchanged
}
```

In the same file, after every successful `tell`/`ask`/`post_reply` in `handlers.rs`, the broker should publish to `message_stream`. Easiest: do it in the handler. (Step 3 below shows it; this is the prep step.)

- [ ] **Step 2: Update tell/ask/post_reply handlers to broadcast on `message_stream`**

In `src/broker/handlers.rs`, replace the bodies of the `Tell`, `Ask`, and `PostReply` arms with the versions below. Each clones field copies BEFORE moving them into the store call so we can build the DTO afterwards.

```rust
Request::Tell { from, to, text, urgent: _ } => {
    let from_dto = from.clone();
    let to_dto = to.clone();
    let text_dto = text.clone();
    match ctx.store.tell(&from, to.as_deref(), &text) {
        Ok(message_id) => {
            let dto = crate::ipc::MessageDto {
                id: message_id,
                from: from_dto,
                to: to_dto,
                text: text_dto,
                ask_id: None,
                in_reply_to: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            let _ = ctx.message_stream.send(dto);
            Response::TellAck { message_id }
        }
        Err(e) => Response::Error { message: format!("{:#}", e) },
    }
}
Request::Ask { from, to, text } => {
    let from_dto = from.clone();
    let to_dto = to.clone();
    let text_dto = text.clone();
    match ctx.store.ask(&from, &to, &text) {
        Ok(result) => {
            let dto = crate::ipc::MessageDto {
                id: result.message_id,
                from: from_dto,
                to: Some(to_dto),
                text: text_dto,
                ask_id: Some(result.ask_id),
                in_reply_to: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            let _ = ctx.message_stream.send(dto);
            Response::AskAck { ask_id: result.ask_id }
        }
        Err(e) => Response::Error { message: format!("{:#}", e) },
    }
}
Request::PostReply { from, ask_id, text } => {
    let from_dto = from.clone();
    let text_dto = text.clone();
    match ctx.store.post_reply(&from, ask_id, &text) {
        Ok(result) => {
            ctx.fire_reply(ask_id).await;
            let dto = crate::ipc::MessageDto {
                id: result.message_id,
                from: from_dto,
                to: Some(result.original_asker),
                text: text_dto,
                ask_id: None,
                in_reply_to: Some(ask_id),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            let _ = ctx.message_stream.send(dto);
            Response::ReplyAck { reply_id: result.reply_id }
        }
        Err(e) => Response::Error { message: format!("{:#}", e) },
    }
}
```

These replace the simpler versions written in Tasks 8 and 9.

- [ ] **Step 3: Implement `SubscribeStream`**

The catch: a single connection now needs to deliver many response frames (the stream events) without further requests. The existing handler model is request → response (single). We extend the connection handler to handle SubscribeStream as a special case: hand the connection over to a streaming task, and exit the request loop.

Modify `handle_connection` in `src/broker/server.rs`:
```rust
async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    ctx: Arc<BrokerCtx>,
) -> Result<()> {
    loop {
        let frame = match read_frame_async(&mut stream).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let request: Request = serde_json::from_slice(&frame)?;
        if matches!(request, Request::SubscribeStream) {
            return run_stream(stream, ctx).await;
        }
        let response = handlers::dispatch(request, &ctx).await;
        let bytes = serde_json::to_vec(&response)?;
        write_frame_async(&mut stream, &bytes).await?;
    }
}

async fn run_stream(mut stream: tokio::net::UnixStream, ctx: Arc<BrokerCtx>) -> Result<()> {
    let mut rx = ctx.message_stream.subscribe();
    // Send an Ok ack once subscribed so client knows the stream is live.
    write_frame_async(&mut stream, &serde_json::to_vec(&Response::Ok)?).await?;
    loop {
        match rx.recv().await {
            Ok(dto) => {
                let frame = serde_json::to_vec(&Response::StreamEvent { message: dto })?;
                if write_frame_async(&mut stream, &frame).await.is_err() {
                    return Ok(()); // client disconnected
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return Ok(()),
        }
    }
}
```

Now the `SubscribeStream` handler in `dispatch` is unreachable (we route before dispatch), so leave it returning Error or remove the arm — leave the Error arm in place as a defensive default.

- [ ] **Step 4: Implement `tail` subcommand**

Create `src/subcommands/tail.rs`:
```rust
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::{paths, tmux};
use anyhow::{anyhow, Result};
use tokio::net::UnixStream;

pub async fn run(session: Option<String>) -> Result<()> {
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("session `{}` not running.", session);
    }

    let mut s = UnixStream::connect(&socket).await?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::SubscribeStream)?).await?;

    // Expect an initial Ok.
    let frame = read_frame_async(&mut s).await?;
    match serde_json::from_slice::<Response>(&frame)? {
        Response::Ok => {}
        other => anyhow::bail!("unexpected subscribe response: {:?}", other),
    }

    println!("[{}] tail starting…", session);
    loop {
        let frame = match read_frame_async(&mut s).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        let resp: Response = serde_json::from_slice(&frame)?;
        if let Response::StreamEvent { message } = resp {
            let to = message.to.clone().unwrap_or_else(|| "@everyone".into());
            println!(
                "{}  {:>10} → {:<10}  {}",
                &message.created_at[11..19], // HH:MM:SS slice
                message.from,
                to,
                message.text
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Add to `src/subcommands/mod.rs`**

```rust
pub mod attach;
pub mod list;
pub mod start;
pub mod stop;
pub mod tail;
```

- [ ] **Step 6: Wire in `src/main.rs`**

```rust
Command::Tail { session } => agents_connector::subcommands::tail::run(session).await,
```

- [ ] **Step 7: Build**

Run: `cargo build`
Expected: builds.

- [ ] **Step 8: Run existing tests to confirm no regressions**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/broker/ src/subcommands/ src/main.rs
git commit -m "feat: tail subcommand subscribing to broker message stream"
```

---

## Task 17: Claude adapter — MCP config generator

**Files:**
- Create: `src/adapters/mod.rs`
- Create: `src/adapters/claude.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/adapters/mod.rs`**

```rust
//! CLI adapters: per-CLI config generation for MCP and hooks.

pub mod claude;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    // Codex, Gemini — Phase 2/3
}

impl CliKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "claude" => Ok(CliKind::Claude),
            other => anyhow::bail!("unsupported cli kind: {}. Phase 1 supports: claude.", other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self { CliKind::Claude => "claude" }
    }
}
```

- [ ] **Step 2: Implement `src/adapters/claude.rs`**

```rust
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
    // The hook script runs our `hook` subcommand which checks for new messages and
    // emits a follow-up prompt via Claude's hook output protocol.
    let settings = json!({
        "hooks": {
            "Stop": [{
                "matcher": "",
                "hooks": [{
                    "type": "command",
                    "command": format!(
                        "{} hook --socket {} --agent-token {} --event stop",
                        binary_path.to_string_lossy(),
                        socket_path.to_string_lossy(),
                        agent_token
                    )
                }]
            }]
        }
    });
    let settings_path = agent_dir.join("settings.json");
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;

    Ok(Generated { mcp_config_path, settings_path })
}
```

- [ ] **Step 3: Add to `src/lib.rs`**

```rust
pub mod adapters;
pub mod broker;
pub mod cli;
pub mod ipc;
pub mod paths;
pub mod shim;
pub mod subcommands;
pub mod tmux;
```

- [ ] **Step 4: Add a unit test for config generation**

At the bottom of `src/adapters/claude.rs`:
```rust
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
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib adapters`
Expected: 1 test passes.

- [ ] **Step 6: Commit**

```bash
git add src/adapters/ src/lib.rs
git commit -m "feat: claude adapter generates mcp config and stop-hook settings"
```

---

## Task 18: hook subcommand

The hook subcommand is invoked by Claude Code at end-of-turn. It connects to the broker, fetches messages since the agent's last high-water-mark, and prints a Claude Code "additional context" payload.

**State:** the high-water-mark per agent needs persistence across hook invocations. We'll use a tiny per-agent file in the session's agent dir: `<agent_dir>/last_seen_message_id`.

**Files:**
- Create: `src/hook/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement `src/hook/mod.rs`**

```rust
//! Hook subcommand: runs at end-of-turn, checks for new messages, emits
//! additional context for the CLI's hook protocol.

use crate::ipc::{read_frame_sync, write_frame_sync, Request, Response};
use anyhow::{Context, Result};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub fn run(socket: PathBuf, agent_token: String, event: String) -> Result<()> {
    if event != "stop" {
        // Other events (PostToolUse etc.) not yet handled.
        return Ok(());
    }

    let mut stream = UnixStream::connect(&socket)
        .with_context(|| format!("connecting to broker at {}", socket.display()))?;

    // Authenticate.
    let req = Request::Authenticate { agent_token: agent_token.clone() };
    write_frame_sync(&mut stream, &serde_json::to_vec(&req)?)?;
    let frame = read_frame_sync(&mut stream)?;
    let agent_name = match serde_json::from_slice::<Response>(&frame)? {
        Response::AgentInfo { name, .. } => name,
        Response::Error { message } => anyhow::bail!("auth failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };

    // Read messages since high-water-mark.
    // The session dir is the parent of the broker socket; the per-agent dir is
    // <session>/agents/<agent-name>/, matching the layout `add` writes into.
    let session_dir = socket.parent()
        .ok_or_else(|| anyhow::anyhow!("malformed socket path"))?;
    let agent_dir = session_dir.join("agents").join(&agent_name);
    std::fs::create_dir_all(&agent_dir)?;
    let hwm_file = agent_dir.join("last_seen_message_id");
    let since: i64 = std::fs::read_to_string(&hwm_file).ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    let req = Request::ReadMessages { agent: agent_name.clone(), since };
    write_frame_sync(&mut stream, &serde_json::to_vec(&req)?)?;
    let frame = read_frame_sync(&mut stream)?;
    let msgs = match serde_json::from_slice::<Response>(&frame)? {
        Response::Messages { messages } => messages,
        other => anyhow::bail!("unexpected: {:?}", other),
    };

    if msgs.is_empty() {
        return Ok(()); // No additional context to inject.
    }

    // Update high-water-mark.
    if let Some(last) = msgs.last() {
        std::fs::write(&hwm_file, last.id.to_string())?;
    }

    // Emit Claude Code hook JSON: { "additionalContext": "...messages..." }.
    // (Verify exact field name against current Claude Code docs at execution time;
    //  the field used by Stop hooks for follow-up text may differ.)
    let mut text = String::from("[agents-connector] You have new messages:\n");
    for m in &msgs {
        let to = m.to.as_deref().unwrap_or("@everyone");
        text.push_str(&format!("- from {} → {}: {}\n", m.from, to, m.text));
    }
    text.push_str("\nUse the `read_messages` MCP tool with `since` set to the latest id you've handled to retrieve again, or use `tell`/`ask` to respond.");

    let payload = serde_json::json!({ "additionalContext": text });
    println!("{}", payload);

    Ok(())
}
```

- [ ] **Step 2: Add to `src/lib.rs`**

```rust
pub mod adapters;
pub mod broker;
pub mod cli;
pub mod hook;
pub mod ipc;
pub mod paths;
pub mod shim;
pub mod subcommands;
pub mod tmux;
```

- [ ] **Step 3: Wire in `src/main.rs`**

```rust
Command::Hook { socket, agent_token, event } => {
    agents_connector::hook::run(socket, agent_token, event)
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds.

- [ ] **Step 5: Verify exact Claude Code Stop hook output schema**

Open the latest Claude Code hooks docs (search "Claude Code hooks Stop" or check `~/.claude/` for examples). Confirm:
- The JSON field name for injecting additional context (`additionalContext` is the assumption above; might be `additional_context` or `block` or different entirely depending on hook event).
- Whether the hook must exit 0 to inject, or whether it uses a special exit code.

If the field name differs, fix the `payload` line in Step 1's code accordingly.

- [ ] **Step 6: Commit**

```bash
git add src/hook/ src/lib.rs src/main.rs
git commit -m "feat: hook subcommand for Claude Code Stop event"
```

---

## Task 19: Claude adapter — wire add subcommand

**Files:**
- Create: `src/subcommands/add.rs`
- Modify: `src/subcommands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement**

Create `src/subcommands/add.rs`:
```rust
use crate::adapters::{claude, CliKind};
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use tokio::net::UnixStream;

pub async fn run(
    cli_kind: String,
    name: String,
    session: Option<String>,
    workdir: Option<PathBuf>,
) -> Result<()> {
    let kind = CliKind::parse(&cli_kind)?;
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`. Use `agents-connector start {}` first.", session, session);
    }

    // 1. Ask broker to register the agent.
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    let req = Request::RegisterAgent { name: name.clone(), cli_kind: kind.as_str().to_string() };
    write_frame_async(&mut s, &serde_json::to_vec(&req)?).await?;
    let frame = read_frame_async(&mut s).await?;
    let token = match serde_json::from_slice::<Response>(&frame)? {
        Response::RegisterAck { agent_token } => agent_token,
        Response::Error { message } => anyhow::bail!("register failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    // 2. Generate per-CLI config.
    let agent_dir = paths::session_agent_dir(&session, &name)?;
    let exe = std::env::current_exe()?;
    let generated = match kind {
        CliKind::Claude => claude::generate(&agent_dir, &exe, &socket, &token)?,
    };

    // 3. Build the launch command. For Claude Code:
    //    claude --mcp-config <mcp.json> --settings <settings.json>
    //    (verify flag names match current Claude Code; older versions use --mcp-config-file etc.)
    let launch_cmd = match kind {
        CliKind::Claude => format!(
            "claude --mcp-config {} --settings {}",
            shell_quote(&generated.mcp_config_path.to_string_lossy()),
            shell_quote(&generated.settings_path.to_string_lossy())
        ),
    };

    // 4. tmux new-window inside the session.
    let workdir_str = workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let cd_prefix = workdir_str.as_ref().map(|d| format!("cd {} && ", shell_quote(d))).unwrap_or_default();
    let full_cmd = format!("{}{}", cd_prefix, launch_cmd);
    tmux::new_window(&session, &name, &[], &full_cmd)?;

    println!("agent `{}` ({}) added to session `{}`.", name, kind.as_str(), session);
    println!("MCP config: {}", generated.mcp_config_path.display());
    println!("Settings:   {}", generated.settings_path.display());

    Ok(())
}

fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./=:".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}
```

- [ ] **Step 2: Add to `src/subcommands/mod.rs`**

```rust
pub mod add;
pub mod attach;
pub mod list;
pub mod start;
pub mod stop;
pub mod tail;
```

- [ ] **Step 3: Wire in `src/main.rs`**

```rust
Command::Add { cli_kind, name, session, workdir } => {
    agents_connector::subcommands::add::run(cli_kind, name, session, workdir).await
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds.

- [ ] **Step 5: Verify Claude flag names**

`claude --help` should show `--mcp-config` and `--settings` (or whatever the current name is). If different, update `launch_cmd` in step 1.

- [ ] **Step 6: Commit**

```bash
git add src/subcommands/ src/main.rs
git commit -m "feat: add subcommand spawns Claude pane with MCP and Stop hook"
```

---

## Task 20: End-to-end synthetic integration test

This test exercises the broker + a synthetic MCP-like client (i.e., we drive the IPC layer directly, not through `rmcp`) to validate the full chat flow without depending on a real Claude Code install.

**Files:**
- Create: `tests/e2e_test.rs`

- [ ] **Step 1: Write the test**

```rust
use agents_connector::broker::store::Store;
use agents_connector::broker::server;
use agents_connector::ipc::{read_frame_async, write_frame_async, Request, Response};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::UnixStream;

async fn spawn_broker() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (tmp, sock)
}

#[tokio::test]
async fn alice_asks_bob_who_replies_and_alice_sees_reply() {
    let (_tmp, sock) = spawn_broker().await;

    // Connect as alice.
    let mut alice = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(),
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut alice).await.unwrap();

    // Connect as bob.
    let mut bob = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut bob, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(),
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut bob).await.unwrap();

    // alice asks bob.
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(), to: "bob".into(), text: "are you ready?".into(),
    }).unwrap()).await.unwrap();
    let ask_id = match serde_json::from_slice::<Response>(&read_frame_async(&mut alice).await.unwrap()).unwrap() {
        Response::AskAck { ask_id } => ask_id,
        other => panic!("unexpected: {:?}", other),
    };

    // bob reads messages — sees the ask.
    write_frame_async(&mut bob, &serde_json::to_vec(&Request::ReadMessages {
        agent: "bob".into(), since: 0,
    }).unwrap()).await.unwrap();
    let msgs = match serde_json::from_slice::<Response>(&read_frame_async(&mut bob).await.unwrap()).unwrap() {
        Response::Messages { messages } => messages,
        other => panic!("unexpected: {:?}", other),
    };
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text, "are you ready?");
    assert_eq!(msgs[0].ask_id, Some(ask_id));

    // bob replies.
    write_frame_async(&mut bob, &serde_json::to_vec(&Request::PostReply {
        from: "bob".into(), ask_id, text: "yes".into(),
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut bob).await.unwrap();

    // alice waits for reply (should return immediately).
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::WaitForReply {
        ask_id, timeout_ms: 1000,
    }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut alice).await.unwrap()).unwrap() {
        Response::Replies { replies } => {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].text, "yes");
            assert_eq!(replies[0].from, "bob");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn broadcast_visible_to_all_other_agents() {
    let (_tmp, sock) = spawn_broker().await;
    for n in &["alice", "bob", "carol"] {
        let mut s = UnixStream::connect(&sock).await.unwrap();
        write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
            name: n.to_string(), cli_kind: "claude".into(),
        }).unwrap()).await.unwrap();
        let _ = read_frame_async(&mut s).await.unwrap();
    }

    let mut alice = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::Tell {
        from: "alice".into(), to: None, text: "hi all".into(), urgent: false,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut alice).await.unwrap();

    for n in &["bob", "carol"] {
        let mut s = UnixStream::connect(&sock).await.unwrap();
        write_frame_async(&mut s, &serde_json::to_vec(&Request::ReadMessages {
            agent: n.to_string(), since: 0,
        }).unwrap()).await.unwrap();
        let msgs = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
            Response::Messages { messages } => messages,
            other => panic!("unexpected: {:?}", other),
        };
        assert_eq!(msgs.len(), 1, "{} should see one broadcast", n);
        assert_eq!(msgs[0].text, "hi all");
    }

    // alice does NOT see her own broadcast.
    let mut alice = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::ReadMessages {
        agent: "alice".into(), since: 0,
    }).unwrap()).await.unwrap();
    let msgs = match serde_json::from_slice::<Response>(&read_frame_async(&mut alice).await.unwrap()).unwrap() {
        Response::Messages { messages } => messages,
        other => panic!("unexpected: {:?}", other),
    };
    assert_eq!(msgs.len(), 0);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test e2e_test`
Expected: 2 tests pass.

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add tests/e2e_test.rs
git commit -m "test: end-to-end ask/reply and broadcast flows"
```

---

## Task 21: Manual smoke test + README

**Files:**
- Create: `README.md`
- Modify: `Cargo.toml` (set repository URL etc.)

- [ ] **Step 1: Manual smoke test (with real Claude Code, two windows)**

Build a release binary and put it on PATH:
```bash
cargo build --release
sudo cp target/release/agents-connector /usr/local/bin/
```

Start a session:
```bash
agents-connector start demo
```

Inside the new tmux session, in the top pane, add two Claudes:
```bash
agents-connector add claude --name alice
```
Press Ctrl-B + n to switch to alice's window. Wait for Claude Code to start. Then in the top pane:
```bash
agents-connector add claude --name bob
```

Switch back to alice's window. Prompt alice:
> "Use the `tell` tool from `agents_connector` to send `bob` the message: 'hello bob'."

Watch the tail pane (bottom of original window). You should see `alice → bob  hello bob` appear.

Switch to bob's window. Press Enter to give Claude a turn (or wait for the Stop hook to fire after Claude's next turn — depending on Claude's behavior, you may need to send a fresh prompt). Verify bob sees the message via the Stop hook injection.

If the Stop hook doesn't inject as expected, check Task 18 Step 5 — the hook output schema may differ from what we assumed. Fix and retry.

- [ ] **Step 2: Document any deviations from the plan**

If you had to change `rmcp` API calls (Task 10), the Claude flag names (Task 19), or the Stop hook output schema (Task 18), record those deviations in a `NOTES.md` file at the repo root for future maintainers.

- [ ] **Step 3: Write `README.md`**

```markdown
# agents-connector

A multi-agent CLI communication substrate. Lets multiple AI CLI agents
(Claude Code, eventually Codex, Gemini CLI) running in separate tmux panes
exchange messages through a single shared session.

## Status

**v0.1 (Phase 1)** — Two Claude Code instances can chat via `tell`/`ask`/`reply`.

## Install

```bash
brew install tmux  # prerequisite
cargo install --path .
```

## Usage

```bash
# Start a session.
agents-connector start review-pod

# Inside the tmux session, add agents.
agents-connector add claude --name writer
agents-connector add claude --name reviewer

# Optional: tail the chat from another terminal.
agents-connector tail review-pod

# Shut down when done.
agents-connector stop review-pod
```

In a Claude window, the `agents_connector` MCP server exposes tools:
- `tell(to, text, urgent)` — send a fire-and-forget message
- `ask(to, text)` — ask a question, get an `ask_id`
- `wait_for_reply(ask_id, timeout_ms)` — block until reply arrives
- `check_replies(ask_id)` — non-blocking poll for replies
- `read_messages(since)` — fetch messages since a high-water-mark
- `post_reply(ask_id, text)` — reply to an outstanding ask
- `list_agents()` — see who's in the session

## Architecture

See `docs/superpowers/specs/` for the design rationale.

## Roadmap

- Phase 2: Codex adapter, `resume`/`restart`/`remove` subcommands.
- Phase 3: Gemini adapter, tmux send-keys wake fallback.
- Phase 4: Packaging (Homebrew, prebuilt releases).
```

- [ ] **Step 4: Commit**

```bash
git add README.md NOTES.md
git commit -m "docs: README and any deviations from plan"
```

---

## Verification: definition of done

At the end of all 21 tasks, the following must be true:

1. `cargo build --release` succeeds.
2. `cargo test` shows all tests passing.
3. Manual smoke test from Task 21 Step 1 works: two Claudes can chat, one's message reaches the other via Stop hook injection.
4. `agents-connector start demo`, then `Ctrl-B D` to detach, then `agents-connector stop demo`, then `agents-connector start demo` (different name OR after a session-dir cleanup) works without panics.
5. `agents-connector list` shows running and stopped sessions correctly.
6. The plan doc (this file) and a README.md are checked in.

---

## What we explicitly didn't build (and where it lives)

| Feature | Plan |
|---|---|
| Codex adapter | Plan 2 (next) |
| Gemini adapter | Plan 3 |
| `resume` / `restart` / `remove` subcommands | Plan 2 |
| Urgent wake via tmux send-keys | Plan 3 |
| Hook adapters for Codex/Gemini | Plans 2/3 |
| Homebrew formula, GitHub Actions release pipeline | Plan 4 |
| `urgent: true` actually doing anything | Plan 3 (today it's accepted but ignored) |

These deferrals are deliberate. The substrate as specified is enough to demonstrate the value proposition (two models chatting without you copy-pasting) and gives you a real product surface to extend.
