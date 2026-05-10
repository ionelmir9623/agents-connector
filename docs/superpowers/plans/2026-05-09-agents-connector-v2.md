# `agents-connector` v2 (Plan 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Codex CLI as a second supported adapter, plus three session-lifecycle subcommands (`resume`, `restart`, `remove`) so users can stop and resume sessions, refresh an agent's model context without losing chat history, and cleanly remove agents.

**Architecture:** Extends the v1 binary additively. Schema gains a `workdir` column on `agents` so resume can relaunch with the right cwd. The `add` subcommand's "spawn-CLI-in-tmux" logic is extracted into a reusable `launch_agent` helper used by `add`, `resume`, and `restart`. Two new IPC variants (`RemoveAgent`, `GetAgent`) let the launcher query and mutate agent state. A new `adapters/codex.rs` writes per-agent `config.toml` (declaring the MCP server) and `hooks.json` (PostToolUse + UserPromptSubmit hooks for auto-message-injection) into an isolated `CODEX_HOME` directory per agent — no global pollution. The `hook` subcommand learns to emit per-CLI output formats (Claude's flat `additionalContext` vs Codex's nested `hookSpecificOutput.additionalContext`).

**Tech Stack:** Same as v1 — Rust, tokio, clap, rusqlite, rmcp 1.6, plus `toml = "0.8"` for Codex's TOML config generation.

**Codex CLI facts (verified against `codex --help` and https://developers.openai.com/codex/hooks on v0.120.0 / v0.130.0):**
- MCP servers are declared in `<CODEX_HOME>/config.toml` under `[mcp_servers.<name>]` with `command`, `args`, `env` keys. `CODEX_HOME` env var defaults to `~/.codex/` but can be overridden — confirmed working with a custom path.
- Hooks are declared in `<CODEX_HOME>/hooks.json` (or inline in `config.toml`).
- Supported hook events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Stop`.
- **Critical:** Codex's `Stop` hook is fire-and-forget — it CANNOT inject `additionalContext`. Only `SessionStart`, `UserPromptSubmit`, and `PostToolUse` can inject via `hookSpecificOutput.additionalContext`. We use `PostToolUse` + `UserPromptSubmit` for ongoing message injection.
- `codex --cd <path>` sets the agent's working directory.
- `codex` (no subcommand) is the interactive TUI; `codex exec` is non-interactive. We launch the TUI.

**Final state at end of plan:** `agents-connector add codex --name reviewer` works alongside `add claude --name writer`. `agents-connector remove --name reviewer` kills the pane and soft-deletes. `agents-connector restart --name reviewer` keeps the same identity but starts with a fresh model context. `agents-connector stop demo` followed by `agents-connector resume demo` brings back the broker and re-launches all non-removed agents in fresh tmux windows. All v1 tests still pass; new tests cover the lifecycle subcommands.

---

## File structure

**Modified files:**
- `src/cli.rs` — add `Resume`, `Restart`, `Remove` subcommand variants; add `--cli-kind` flag to `Hook`
- `src/main.rs` — wire the three new arms; pass `cli_kind` through to hook
- `src/lib.rs` — no changes (modules already declared)
- `src/broker/store.rs` — add `workdir` column, `remove_agent` method, update `Agent` struct
- `src/broker/handlers.rs` — handle `RemoveAgent` and `GetAgent` requests
- `src/ipc.rs` — add `RemoveAgent`, `GetAgent` requests; add `AgentDetails` response; `RegisterAgent.workdir`
- `src/hook/mod.rs` — add `cli_kind` parameter; dispatch output schema (Claude flat vs Codex `hookSpecificOutput`)
- `src/adapters/claude.rs` — pass `--cli-kind claude` in the Stop hook command
- `src/adapters/mod.rs` — extend `CliKind` enum with `Codex` variant
- `src/subcommands/mod.rs` — add four new submodules (launch, remove, restart, resume)
- `src/subcommands/add.rs` — refactor: extract `launch_agent` helper module function
- `tests/store_test.rs` — add tests for the new store methods
- `tests/broker_ipc_test.rs` — add tests for the new IPC handlers

**New files:**
- `src/adapters/codex.rs` — Codex `-c` override builder (no file writing)
- `src/subcommands/launch.rs` — extracted reusable launch helper
- `src/subcommands/resume.rs`
- `src/subcommands/restart.rs`
- `src/subcommands/remove.rs`
- `tests/codex_adapter_test.rs` — adapter-level tests for Codex override generation

**Boundaries (unchanged from v1):**
- `broker` knows nothing about MCP, tmux, or adapters.
- `adapters` write config files; they don't spawn anything.
- `subcommands::launch` knows tmux + adapters but not IPC details.
- `subcommands::*` know IPC and tmux but go through `launch` for any CLI-spawn.

---

## Sequencing

```
0. Verify Codex CLI install + CODEX_HOME recognition
1. Schema migration: add workdir column to agents
2. Store: remove_agent + agent_by_name lookup hardening
3. IPC: RemoveAgent / GetAgent / AgentDetails
4. Broker handlers: RemoveAgent / GetAgent
5. Refactor: extract launch_agent helper from add
6. Subcommand: remove
7. Subcommand: restart
8. Subcommand: resume
9. Hook subcommand: --cli-kind flag + codex event handling
10. Codex adapter: -c override builder (no file writing)
11. Wire Codex into CliKind + launch_in_tmux
12. README + manual smoke test
```

---

## Task 0: Verify Codex CLI install + CODEX_HOME

**Files:** none (verification only).

The plan-author already verified Codex 0.120.0/0.130.0's MCP and hook surfaces. This task confirms the engineer's local install matches.

- [ ] **Step 1: Confirm Codex CLI is installed**

Run: `codex --version`
Expected: `codex-cli 0.120.0` or newer. If absent, install via `brew install codex` or whatever is current.

- [ ] **Step 2: Confirm `CODEX_HOME` env var is recognized**

Run: `CODEX_HOME=/tmp/nonexistent-codex-home codex --version 2>&1 | head -3`
Expected: a warning like `WARNING: ... CODEX_HOME points to "/tmp/nonexistent-codex-home", but that path does not exist`, followed by the version. The warning proves codex reads the env var.

- [ ] **Step 3: Confirm hooks docs match**

Skim https://developers.openai.com/codex/hooks. Confirm:
- Hook events include `PostToolUse` and `UserPromptSubmit`.
- These two events accept `hookSpecificOutput.additionalContext` as their JSON output for context injection.
- Hooks can be declared in `<CODEX_HOME>/hooks.json` next to `config.toml`.

If anything has changed materially since this plan was written, stop and adjust Tasks 9-10 to match.

- [ ] **Step 4: No commit yet — proceed when checks pass.**

---

## Task 1: Schema migration — add `workdir` column to agents

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/broker/store.rs`
- Modify: `/Users/frog/code/terminal_tool/tests/store_test.rs`

The `agents` table needs to remember the cwd so `resume` can relaunch agents in the right directory. This is an additive migration — existing rows get NULL workdir, which means "default" (use the session's default cwd).

- [ ] **Step 1: Update `Agent` struct and `migrate` function**

In `src/broker/store.rs`, find the `Agent` struct and add `pub workdir: Option<String>,` as the last field:

```rust
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: i64,
    pub name: String,
    pub cli_kind: String,
    pub token: String,
    pub registered_at: DateTime<Utc>,
    pub removed_at: Option<DateTime<Utc>>,
    pub workdir: Option<String>,
}
```

Update the `migrate` function. The agents-table creation should now include `workdir TEXT`:
```rust
CREATE TABLE IF NOT EXISTS agents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    cli_kind TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    registered_at TEXT NOT NULL,
    removed_at TEXT,
    workdir TEXT
);
```

For databases created before this column existed, add a defensive `ALTER TABLE`:
```rust
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS agents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            cli_kind TEXT NOT NULL,
            token TEXT NOT NULL UNIQUE,
            registered_at TEXT NOT NULL,
            removed_at TEXT,
            workdir TEXT
        );
        CREATE INDEX IF NOT EXISTS agents_token_idx ON agents(token);

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_agent TEXT NOT NULL,
            to_agent TEXT,
            text TEXT NOT NULL,
            ask_id INTEGER,
            in_reply_to INTEGER,
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

    // Idempotent column add for v1-created databases.
    let has_workdir = conn.query_row(
        "SELECT 1 FROM pragma_table_info('agents') WHERE name = 'workdir'",
        [],
        |_| Ok(true),
    ).optional()?.unwrap_or(false);
    if !has_workdir {
        conn.execute("ALTER TABLE agents ADD COLUMN workdir TEXT", [])?;
    }

    Ok(())
}
```

- [ ] **Step 2: Update `register_agent` to take and persist `workdir`**

Change the signature to:
```rust
pub fn register_agent(&self, name: &str, cli_kind: &str, workdir: Option<&str>) -> Result<String> {
    let token = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO agents (name, cli_kind, token, registered_at, workdir) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, cli_kind, token, now, workdir],
    ).map_err(|e| match e {
        rusqlite::Error::SqliteFailure(ref err, Some(ref s))
            if err.code == rusqlite::ErrorCode::ConstraintViolation
                && s.contains("agents.name") =>
        {
            anyhow!("agent already exists: {}", name)
        }
        other => anyhow::Error::from(other),
    })?;
    Ok(token)
}
```

- [ ] **Step 3: Update all `Agent` row mappers to read the new column**

Three places: `agent_by_token`, `agent_by_name`, `list_agents`. Each constructs an `Agent` struct from a row. The SELECT must include `workdir` and the row mapper must populate the field.

For each, change the SELECT to:
```sql
SELECT id, name, cli_kind, token, registered_at, removed_at, workdir FROM agents ...
```

And the row mapper to add:
```rust
workdir: row.get::<_, Option<String>>(6)?,
```

Apply this consistently to all three methods. (Reviewer flagged the duplication in v1; not refactoring it here because that's a separate change — keep the diff minimal.)

- [ ] **Step 4: Update existing tests that call `register_agent`**

Every test in `tests/store_test.rs` that calls `store.register_agent("name", "kind")` must now pass a third arg. Add `None` to existing call sites:

```rust
store.register_agent("alice", "claude", None).unwrap();
```

Update:
- `opens_creates_schema_and_registers_agent`
- `rejects_duplicate_agent_name`
- `list_agents_returns_all`
- `tells_and_reads_messages`
- `broadcast_tell_visible_to_everyone_but_sender`
- `ask_and_reply_links_correctly`
- `agent_by_token_excludes_soft_deleted`

Also update existing `tests/broker_ipc_test.rs` callers — they go through IPC `RegisterAgent`, which we'll update in Task 3, so for now just check the file compiles after updating store_test.

- [ ] **Step 5: Add a new test verifying workdir round-trips**

Append to `tests/store_test.rs`:
```rust
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
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test store_test`
Expected: all 8 store tests pass (7 existing + 1 new).

- [ ] **Step 7: Commit**

```bash
git add src/broker/store.rs tests/store_test.rs
git commit -m "feat(store): add workdir column to agents"
```

---

## Task 2: Store — `remove_agent` method + `agent_by_name` lookup hardening

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/broker/store.rs`
- Modify: `/Users/frog/code/terminal_tool/tests/store_test.rs`

- [ ] **Step 1: Add `remove_agent` method to `Store`**

In `src/broker/store.rs`, append to `impl Store`:
```rust
/// Soft-delete an agent (sets `removed_at`). Returns the agent's token so the
/// caller can clean up filesystem state. Errors if the agent isn't found or
/// is already removed.
pub fn remove_agent(&self, name: &str) -> Result<String> {
    let conn = self.conn.lock().unwrap();
    let now = Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    // Verify agent exists and is active; capture the token.
    let token: String = tx.query_row(
        "SELECT token FROM agents WHERE name = ?1 AND removed_at IS NULL",
        params![name],
        |row| row.get(0),
    ).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => anyhow!("agent not found or already removed: {}", name),
        other => anyhow::Error::from(other),
    })?;

    tx.execute(
        "UPDATE agents SET removed_at = ?1 WHERE name = ?2",
        params![now, name],
    )?;
    tx.commit()?;
    Ok(token)
}
```

- [ ] **Step 2: Filter `agent_by_name` to active agents**

To match `agent_by_token` (which already filters), update `agent_by_name`:
```rust
pub fn agent_by_name(&self, name: &str) -> Result<Option<Agent>> {
    let conn = self.conn.lock().unwrap();
    let agent = conn.query_row(
        "SELECT id, name, cli_kind, token, registered_at, removed_at, workdir FROM agents WHERE name = ?1 AND removed_at IS NULL",
        params![name],
        |row| Ok(Agent {
            id: row.get(0)?,
            name: row.get(1)?,
            cli_kind: row.get(2)?,
            token: row.get(3)?,
            registered_at: row.get::<_, String>(4)?.parse::<DateTime<Utc>>().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            removed_at: row.get::<_, Option<String>>(5)?.map(|s| s.parse::<DateTime<Utc>>()).transpose().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            workdir: row.get::<_, Option<String>>(6)?,
        })
    ).optional()?;
    Ok(agent)
}
```

- [ ] **Step 3: Add tests**

Append to `tests/store_test.rs`:
```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test store_test`
Expected: 11 tests pass (8 from Task 1 + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src/broker/store.rs tests/store_test.rs
git commit -m "feat(store): remove_agent soft-delete with token return"
```

---

## Task 3: IPC — `RemoveAgent` / `GetAgent` / `AgentDetails`

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/ipc.rs`

- [ ] **Step 1: Add new variants**

In `src/ipc.rs`, add to the `Request` enum:
```rust
    /// Remove an agent (soft-delete). Returns the freed token so caller can clean up files.
    RemoveAgent { name: String },
    /// Look up an agent by name. Returns full details (including token + workdir) needed for restart/resume.
    GetAgent { name: String },
```

Also update the existing `RegisterAgent` variant to carry workdir:
```rust
    RegisterAgent { name: String, cli_kind: String, workdir: Option<String> },
```

Add to the `Response` enum:
```rust
    /// Full agent details, used by the launcher to relaunch / restart an agent.
    AgentDetails {
        name: String,
        cli_kind: String,
        token: String,
        workdir: Option<String>,
    },
    /// Removal acknowledgment; carries the freed token.
    RemoveAck { freed_token: String },
```

- [ ] **Step 2: No tests required at this layer**

The wire-format is exercised end-to-end via the broker tests in Task 4. Since serde derives are mechanical, skip explicit round-trip tests.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds cleanly. You'll see warnings about unused variants until Task 4 wires the broker handlers — that's fine for this commit.

- [ ] **Step 4: Commit**

```bash
git add src/ipc.rs
git commit -m "feat(ipc): RemoveAgent, GetAgent, AgentDetails variants + RegisterAgent.workdir"
```

---

## Task 4: Broker handlers — `RemoveAgent`, `GetAgent`, updated `RegisterAgent`

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/broker/handlers.rs`
- Modify: `/Users/frog/code/terminal_tool/tests/broker_ipc_test.rs`

- [ ] **Step 1: Update `RegisterAgent` arm to forward workdir**

In `src/broker/handlers.rs`, replace the existing `Request::RegisterAgent` arm:
```rust
Request::RegisterAgent { name, cli_kind, workdir } => match ctx.store.register_agent(&name, &cli_kind, workdir.as_deref()) {
    Ok(token) => Response::RegisterAck { agent_token: token },
    Err(e) => Response::Error { message: format!("{:#}", e) },
},
```

- [ ] **Step 2: Add `RemoveAgent` and `GetAgent` arms**

In the same `match`, add (before the `Request::Shutdown` arm):
```rust
Request::RemoveAgent { name } => match ctx.store.remove_agent(&name) {
    Ok(token) => Response::RemoveAck { freed_token: token },
    Err(e) => Response::Error { message: format!("{:#}", e) },
},
Request::GetAgent { name } => match ctx.store.agent_by_name(&name) {
    Ok(Some(agent)) => Response::AgentDetails {
        name: agent.name,
        cli_kind: agent.cli_kind,
        token: agent.token,
        workdir: agent.workdir,
    },
    Ok(None) => Response::Error { message: format!("agent not found: {}", name) },
    Err(e) => Response::Error { message: format!("{:#}", e) },
},
```

- [ ] **Step 3: Update existing broker_ipc tests**

Every existing test that sends `Request::RegisterAgent` needs to include `workdir: None`. Find them in `tests/broker_ipc_test.rs` and update each:
```rust
Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into(), workdir: None }
```

Apply to:
- `register_agent_returns_token_and_list_includes_it` (1 callsite)
- `tell_and_read_messages_round_trip` (2 callsites)
- `ask_reply_check_round_trip` (2 callsites)
- `wait_for_reply_blocks_then_returns` (2 callsites)

Also update the e2e tests in `tests/e2e_test.rs`:
- `alice_asks_bob_who_replies_and_alice_sees_reply` (2 callsites)
- `broadcast_visible_to_all_other_agents` (3 callsites in the loop — change the loop body)

- [ ] **Step 4: Add new tests for RemoveAgent and GetAgent**

Append to `tests/broker_ipc_test.rs`:
```rust
#[tokio::test]
async fn get_agent_returns_full_details() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: Some("/tmp/x".into()),
    }).unwrap()).await.unwrap();
    let token = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::RegisterAck { agent_token } => agent_token,
        other => panic!("unexpected: {:?}", other),
    };

    write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: "alice".into() }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::AgentDetails { name, cli_kind, token: t, workdir } => {
            assert_eq!(name, "alice");
            assert_eq!(cli_kind, "claude");
            assert_eq!(t, token);
            assert_eq!(workdir.as_deref(), Some("/tmp/x"));
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn remove_agent_soft_deletes_via_ipc() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::RemoveAgent { name: "alice".into() }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::RemoveAck { freed_token } => assert!(!freed_token.is_empty()),
        other => panic!("unexpected: {:?}", other),
    }

    // Subsequent GetAgent fails.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: "alice".into() }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::Error { message } => assert!(message.contains("not found")),
        other => panic!("unexpected: {:?}", other),
    }
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: every test passes (existing + 2 new broker tests + 3 new store tests = 27 total).

- [ ] **Step 6: Commit**

```bash
git add src/broker/handlers.rs tests/broker_ipc_test.rs tests/e2e_test.rs
git commit -m "feat(broker): RemoveAgent/GetAgent handlers + RegisterAgent.workdir"
```

---

## Task 5: Refactor — extract `launch_agent` helper

**Files:**
- Create: `/Users/frog/code/terminal_tool/src/subcommands/launch.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/add.rs`

The `add` subcommand currently does: register agent → generate adapter config → build launch command → tmux new-window. The "generate config + build command + tmux new-window" half is identical for `add`, `restart`, and `resume`. Extract it.

- [ ] **Step 1: Create `src/subcommands/launch.rs`**

```rust
//! Reusable agent-launch helper.
//!
//! Given an agent's identity (name, cli_kind, token, workdir) and the
//! session's broker socket, this regenerates the adapter config files (idempotent)
//! and spawns the CLI in a tmux window of the given session.

use crate::adapters::{claude, CliKind};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Spec {
    pub session: String,
    pub name: String,
    pub kind: CliKind,
    pub token: String,
    pub workdir: Option<PathBuf>,
}

/// Generate adapter config for the agent and spawn it in a new tmux window.
/// Idempotent against the on-disk config files (overwrites with same content).
pub fn launch_in_tmux(spec: &Spec, broker_socket: &Path) -> Result<()> {
    let agent_dir = paths::session_agent_dir(&spec.session, &spec.name)?;
    let exe = std::env::current_exe()?;

    let launch_cmd = match spec.kind {
        CliKind::Claude => {
            let generated = claude::generate(&agent_dir, &exe, broker_socket, &spec.token)?;
            format!(
                "claude --mcp-config {} --settings {}",
                shell_quote(&generated.mcp_config_path.to_string_lossy()),
                shell_quote(&generated.settings_path.to_string_lossy())
            )
        }
        CliKind::Codex => {
            // Filled in by Task 10.
            anyhow::bail!("Codex adapter not yet wired into launch_in_tmux (Task 10)");
        }
    };

    let workdir_str = spec.workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let cd_prefix = workdir_str.as_ref()
        .map(|d| format!("cd {} && ", shell_quote(d)))
        .unwrap_or_default();
    let full_cmd = format!("{}{}", cd_prefix, launch_cmd);

    tmux::new_window(&spec.session, &spec.name, &[], &full_cmd)
        .with_context(|| format!("spawning tmux window for agent `{}`", spec.name))?;
    Ok(())
}

/// Kill a tmux window by `<session>:<agent_name>` target. Idempotent — succeeds even
/// if the window is already gone (returns Ok). Errors only on tmux invocation failure.
pub fn kill_agent_window(session: &str, agent_name: &str) -> Result<()> {
    let target = format!("{}:{}", session, agent_name);
    let _ = std::process::Command::new("tmux")
        .args(["kill-window", "-t", &target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("running `tmux kill-window`")?;
    Ok(())
}

pub fn shell_quote(s: &str) -> String {
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
pub mod launch;
pub mod list;
pub mod start;
pub mod stop;
pub mod tail;
```

- [ ] **Step 3: Refactor `src/subcommands/add.rs` to use `launch::launch_in_tmux`**

Replace the body of `run` with this version. The diff: instead of inlining adapter dispatch + tmux::new_window, it calls launch::launch_in_tmux.

```rust
use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch::{self, Spec};
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

    // 1. Ask broker to register the agent (with workdir).
    let workdir_str = workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    let req = Request::RegisterAgent {
        name: name.clone(),
        cli_kind: kind.as_str().to_string(),
        workdir: workdir_str.clone(),
    };
    write_frame_async(&mut s, &serde_json::to_vec(&req)?).await?;
    let frame = read_frame_async(&mut s).await?;
    let token = match serde_json::from_slice::<Response>(&frame)? {
        Response::RegisterAck { agent_token } => agent_token,
        Response::Error { message } => anyhow::bail!("register failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    // 2. Use the shared launch helper.
    let spec = Spec {
        session: session.clone(),
        name: name.clone(),
        kind,
        token,
        workdir: workdir.clone(),
    };
    launch::launch_in_tmux(&spec, &socket)?;

    println!("agent `{}` ({}) added to session `{}`.", name, kind.as_str(), session);
    Ok(())
}
```

The shell_quote and adapter-dispatch logic that lived in add.rs is now in launch.rs. Delete the local `shell_quote` function from add.rs.

- [ ] **Step 4: Verify build + existing tests still pass**

Run: `cargo build && cargo test`
Expected: everything still passes (no test changes; this is a pure refactor).

- [ ] **Step 5: Commit**

```bash
git add src/subcommands/
git commit -m "refactor: extract launch_agent helper for resume/restart reuse"
```

---

## Task 6: Subcommand — `remove`

**Files:**
- Create: `/Users/frog/code/terminal_tool/src/subcommands/remove.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/cli.rs`
- Modify: `/Users/frog/code/terminal_tool/src/main.rs`

- [ ] **Step 1: Add the CLI variant in `src/cli.rs`**

In the `Command` enum, add (alphabetically near `Add`):
```rust
    /// Remove an agent from the current session.
    Remove {
        #[arg(long)]
        name: String,
        #[arg(long)]
        session: Option<String>,
    },
```

- [ ] **Step 2: Implement `src/subcommands/remove.rs`**

```rust
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch;
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use tokio::net::UnixStream;

pub async fn run(name: &str, session: Option<String>) -> Result<()> {
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`.", session);
    }

    // Send RemoveAgent IPC.
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RemoveAgent { name: name.to_string() })?).await?;
    let frame = read_frame_async(&mut s).await?;
    match serde_json::from_slice::<Response>(&frame)? {
        Response::RemoveAck { .. } => {}
        Response::Error { message } => anyhow::bail!("remove failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    }
    drop(s);

    // Kill the agent's tmux window (idempotent — fine if already gone).
    launch::kill_agent_window(&session, name)?;

    println!("agent `{}` removed from session `{}`.", name, session);
    Ok(())
}
```

- [ ] **Step 3: Add `pub mod remove;` to `src/subcommands/mod.rs`**

```rust
pub mod add;
pub mod attach;
pub mod launch;
pub mod list;
pub mod remove;
pub mod start;
pub mod stop;
pub mod tail;
```

- [ ] **Step 4: Wire in `src/main.rs`**

Add a new arm to the `match` in `main`:
```rust
Command::Remove { name, session } => {
    agents_connector::subcommands::remove::run(&name, session).await
}
```

- [ ] **Step 5: Verify build + existing tests still pass**

Run: `cargo build && cargo test`
Expected: everything passes.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/main.rs src/subcommands/
git commit -m "feat: remove subcommand soft-deletes agent and kills window"
```

---

## Task 7: Subcommand — `restart`

**Files:**
- Create: `/Users/frog/code/terminal_tool/src/subcommands/restart.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/cli.rs`
- Modify: `/Users/frog/code/terminal_tool/src/main.rs`

- [ ] **Step 1: Add the CLI variant in `src/cli.rs`**

```rust
    /// Restart an agent in place (same identity, fresh model context).
    Restart {
        #[arg(long)]
        name: String,
        #[arg(long)]
        session: Option<String>,
    },
```

- [ ] **Step 2: Implement `src/subcommands/restart.rs`**

```rust
use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch::{self, Spec};
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use tokio::net::UnixStream;

pub async fn run(name: &str, session: Option<String>) -> Result<()> {
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`.", session);
    }

    // 1. Ask broker for full agent details.
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: name.to_string() })?).await?;
    let frame = read_frame_async(&mut s).await?;
    let (cli_kind_str, token, workdir) = match serde_json::from_slice::<Response>(&frame)? {
        Response::AgentDetails { cli_kind, token, workdir, .. } => (cli_kind, token, workdir),
        Response::Error { message } => anyhow::bail!("get_agent failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    let kind = CliKind::parse(&cli_kind_str)?;

    // 2. Kill the existing tmux window (no-op if already dead).
    launch::kill_agent_window(&session, name)?;

    // 3. Relaunch via the shared helper.
    let spec = Spec {
        session: session.clone(),
        name: name.to_string(),
        kind,
        token,
        workdir: workdir.map(PathBuf::from),
    };
    launch::launch_in_tmux(&spec, &socket)?;

    println!("agent `{}` restarted in session `{}` (chat history preserved).", name, session);
    Ok(())
}
```

- [ ] **Step 3: Add `pub mod restart;` to `src/subcommands/mod.rs`**

```rust
pub mod add;
pub mod attach;
pub mod launch;
pub mod list;
pub mod remove;
pub mod restart;
pub mod start;
pub mod stop;
pub mod tail;
```

- [ ] **Step 4: Wire in `src/main.rs`**

```rust
Command::Restart { name, session } => {
    agents_connector::subcommands::restart::run(&name, session).await
}
```

- [ ] **Step 5: Verify**

Run: `cargo build && cargo test`
Expected: everything passes.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/main.rs src/subcommands/
git commit -m "feat: restart subcommand keeps identity, fresh context"
```

---

## Task 8: Subcommand — `resume`

**Files:**
- Create: `/Users/frog/code/terminal_tool/src/subcommands/resume.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/cli.rs`
- Modify: `/Users/frog/code/terminal_tool/src/main.rs`

`resume` is essentially `start` for an existing session, plus optional auto-relaunch of saved agents.

- [ ] **Step 1: Add the CLI variant in `src/cli.rs`**

```rust
    /// Resume a stopped session (re-spawn broker + optionally re-launch agents).
    Resume {
        session: String,
        #[arg(long, help = "Skip auto-relaunching saved agents")]
        no_agents: bool,
    },
```

- [ ] **Step 2: Implement `src/subcommands/resume.rs`**

```rust
use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch::{self, Spec};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tokio::net::UnixStream;

pub async fn run(session: &str, no_agents: bool) -> Result<()> {
    let session_dir = paths::session_dir(session)?;
    if !session_dir.exists() {
        anyhow::bail!(
            "no session `{}` to resume. Run `agents-connector start {}` to create a new one.",
            session, session
        );
    }
    let db = paths::session_db(session)?;
    if !db.exists() {
        anyhow::bail!(
            "session `{}` exists but db is missing at {}. Recovery is manual.",
            session, db.display()
        );
    }
    if tmux::has_session(session)? {
        anyhow::bail!(
            "tmux session `{}` is still alive. Use `agents-connector attach {}` instead.",
            session, session
        );
    }

    // Spawn broker daemon (mirrors start.rs).
    let socket = paths::session_socket(session)?;
    let pid_file = paths::session_pid_file(session)?;
    let log = paths::session_log(session)?;
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

    // Wait for the socket to appear.
    let mut ok = false;
    for _ in 0..200 {
        if socket.exists() { ok = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if !ok {
        anyhow::bail!("broker failed to restart within 5 seconds; check {}", log.display());
    }

    // Recreate tmux session with tail pane.
    tmux::new_detached_session(session, None)?;
    let tail_command = format!("{} tail {}", exe.to_string_lossy(), session);
    tmux::split_window_below(session, 25, &tail_command)?;

    if !no_agents {
        // Ask broker for the active agent list and relaunch each.
        let mut s = UnixStream::connect(&socket).await
            .context("connecting to broker for relaunch")?;
        write_frame_async(&mut s, &serde_json::to_vec(&Request::ListAgents)?).await?;
        let frame = read_frame_async(&mut s).await?;
        let agents = match serde_json::from_slice::<Response>(&frame)? {
            Response::Agents { agents } => agents,
            other => anyhow::bail!("unexpected list_agents response: {:?}", other),
        };
        drop(s);

        for a in agents {
            // Need full details (token + workdir) to relaunch.
            let mut s = UnixStream::connect(&socket).await?;
            write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: a.name.clone() })?).await?;
            let frame = read_frame_async(&mut s).await?;
            let (token, workdir) = match serde_json::from_slice::<Response>(&frame)? {
                Response::AgentDetails { token, workdir, .. } => (token, workdir),
                Response::Error { message } => {
                    eprintln!("warning: get_agent({}) failed: {}", a.name, message);
                    continue;
                }
                other => anyhow::bail!("unexpected: {:?}", other),
            };
            drop(s);

            let kind = match CliKind::parse(&a.cli_kind) {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("warning: skipping `{}` (unsupported cli_kind `{}`): {}", a.name, a.cli_kind, e);
                    continue;
                }
            };
            let spec = Spec {
                session: session.to_string(),
                name: a.name.clone(),
                kind,
                token,
                workdir: workdir.map(PathBuf::from),
            };
            if let Err(e) = launch::launch_in_tmux(&spec, &socket) {
                eprintln!("warning: relaunch `{}` failed: {:#}", a.name, e);
            } else {
                println!("relaunched `{}` ({})", a.name, a.cli_kind);
            }
        }
    }

    println!("session `{}` resumed.", session);
    println!("attach with: agents-connector attach {}", session);

    tmux::attach_session(session)?;
    Ok(())
}
```

- [ ] **Step 3: Add `pub mod resume;` to `src/subcommands/mod.rs`**

```rust
pub mod add;
pub mod attach;
pub mod launch;
pub mod list;
pub mod remove;
pub mod restart;
pub mod resume;
pub mod start;
pub mod stop;
pub mod tail;
```

- [ ] **Step 4: Wire in `src/main.rs`**

```rust
Command::Resume { session, no_agents } => {
    agents_connector::subcommands::resume::run(&session, no_agents).await
}
```

- [ ] **Step 5: Verify**

Run: `cargo build && cargo test`
Expected: everything passes.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/main.rs src/subcommands/
git commit -m "feat: resume subcommand restores broker and relaunches agents"
```

---

## Task 9: Hook subcommand — add `--cli-kind` flag and Codex event handling

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/cli.rs`
- Modify: `/Users/frog/code/terminal_tool/src/hook/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/main.rs`
- Modify: `/Users/frog/code/terminal_tool/src/adapters/claude.rs`

The existing hook subcommand handles `event=stop` and emits Claude's flat `{"additionalContext": "..."}`. Codex needs different events (`post_tool_use`, `user_prompt_submit`) and a different output schema (`{"hookSpecificOutput": {"additionalContext": "..."}}`). Add a `--cli-kind` flag so the hook knows which schema to use.

- [ ] **Step 1: Add `--cli-kind` to the `Hook` clap variant**

In `src/cli.rs`, replace the `Hook` variant:
```rust
    /// Internal: invoked by adapter hooks (e.g., Claude Code Stop hook, Codex PostToolUse hook).
    #[command(hide = true)]
    Hook {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        agent_token: String,
        #[arg(long)]
        event: String,
        #[arg(long, default_value = "claude")]
        cli_kind: String,
    },
```

- [ ] **Step 2: Update `src/main.rs` to pass `cli_kind` through**

Replace the `Command::Hook` arm:
```rust
Command::Hook { socket, agent_token, event, cli_kind } => {
    agents_connector::hook::run(socket, agent_token, event, cli_kind)
}
```

- [ ] **Step 3: Update `src/hook/mod.rs` signature and dispatch**

Replace the entire body of `src/hook/mod.rs`:
```rust
//! Hook subcommand: runs at end-of-turn (or other adapter event), checks for new
//! messages, emits CLI-specific JSON to inject them as additional context.

use crate::ipc::{read_frame_sync, write_frame_sync, MessageDto, Request, Response};
use anyhow::{Context, Result};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub fn run(socket: PathBuf, agent_token: String, event: String, cli_kind: String) -> Result<()> {
    // Decide whether this event/cli_kind combination is one we inject for.
    if !injects_context(&cli_kind, &event) {
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
        return Ok(());
    }

    if let Some(last) = msgs.last() {
        std::fs::write(&hwm_file, last.id.to_string())?;
    }

    let text = format_messages(&msgs);
    let payload = format_payload(&cli_kind, &text);
    println!("{}", payload);
    Ok(())
}

fn injects_context(cli_kind: &str, event: &str) -> bool {
    match (cli_kind, event) {
        // Claude Code: Stop is the only event we wire in v1.
        ("claude", "stop") => true,
        // Codex: PostToolUse and UserPromptSubmit can both inject hookSpecificOutput.additionalContext.
        // Codex's Stop CANNOT inject context, so we don't fire this hook for stop.
        ("codex", "post_tool_use") => true,
        ("codex", "user_prompt_submit") => true,
        _ => false,
    }
}

fn format_messages(msgs: &[MessageDto]) -> String {
    let mut text = String::from("[agents-connector] You have new messages:\n");
    for m in msgs {
        let to = m.to.as_deref().unwrap_or("@everyone");
        text.push_str(&format!("- from {} → {}: {}\n", m.from, to, m.text));
    }
    text.push_str("\nUse the `read_messages` MCP tool with `since` set to the latest id you've handled to retrieve again, or use `tell`/`ask` to respond.");
    text
}

fn format_payload(cli_kind: &str, text: &str) -> serde_json::Value {
    match cli_kind {
        // Claude Code: flat additionalContext field.
        "claude" => serde_json::json!({ "additionalContext": text }),
        // Codex: nested under hookSpecificOutput.
        "codex" => serde_json::json!({
            "hookSpecificOutput": { "additionalContext": text }
        }),
        // Unknown CLI: emit a minimal payload that won't break anything.
        _ => serde_json::json!({}),
    }
}
```

- [ ] **Step 4: Update Claude adapter to pass `--cli-kind claude`**

In `src/adapters/claude.rs`, find the line in `generate` that builds the Stop hook command:
```rust
"command": format!(
    "{} hook --socket {} --agent-token {} --event stop",
    shell_quote(&binary_path.to_string_lossy()),
    shell_quote(&socket_path.to_string_lossy()),
    shell_quote(agent_token)
)
```

Replace with:
```rust
"command": format!(
    "{} hook --socket {} --agent-token {} --event stop --cli-kind claude",
    shell_quote(&binary_path.to_string_lossy()),
    shell_quote(&socket_path.to_string_lossy()),
    shell_quote(agent_token)
)
```

Update the existing test `handles_paths_with_spaces` if it asserts on the exact command string — keep the substring assertions but add one for `--cli-kind claude`:
```rust
assert!(cmd.contains("--cli-kind claude"), "got: {}", cmd);
```

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all v1 + v2 tests still pass (the existing Claude adapter test now also asserts the new flag).

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/hook/mod.rs src/main.rs src/adapters/claude.rs
git commit -m "feat(hook): --cli-kind flag dispatches output schema; codex events"
```

---

## Task 10: Codex adapter — `-c` override builder

**Files:**
- Create: `/Users/frog/code/terminal_tool/src/adapters/codex.rs`
- Create: `/Users/frog/code/terminal_tool/tests/codex_adapter_test.rs`

The Codex adapter doesn't write any files. Per the "Codex CLI facts" in the header, we use `-c` (config override) flags to inject the MCP server entry and the hooks inline at launch time. This avoids polluting `~/.codex/config.toml` and avoids the auth-file complications of overriding `CODEX_HOME`.

**No new dependency needed:** the `-c` values are TOML literals, but we build them as plain Rust strings (since they're simple key=value pairs with array/inline-table values, not whole TOML documents). `toml = "0.8"` is NOT required.

- [ ] **Step 1: Implement `src/adapters/codex.rs`**

```rust
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

        // Four overrides: mcp.command, mcp.args, hooks.PostToolUse, hooks.UserPromptSubmit.
        assert_eq!(overrides.len(), 4);
        assert!(overrides[0].starts_with("mcp_servers.agents_connector.command="));
        assert!(overrides[1].starts_with("mcp_servers.agents_connector.args="));
        assert!(overrides[2].starts_with("hooks.PostToolUse="));
        assert!(overrides[3].starts_with("hooks.UserPromptSubmit="));

        // The token appears in both hook commands.
        assert!(overrides[2].contains("TOK-99"));
        assert!(overrides[3].contains("TOK-99"));

        // The socket path appears in mcp.args and hook commands.
        assert!(overrides[1].contains("/tmp/sock"));
        assert!(overrides[2].contains("/tmp/sock"));
    }

    #[test]
    fn handles_paths_with_special_chars() {
        let bin = PathBuf::from("/Users/frog with space/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let overrides = config_overrides(&bin, &sock, "TOK-1");

        // The TOML string-quoted value should contain the space inside the literal.
        assert!(overrides[0].contains("/Users/frog with space/bin/agents-connector"));
    }
}
```

- [ ] **Step 2: Add a public adapter test**

Create `tests/codex_adapter_test.rs`:
```rust
use agents_connector::adapters::codex;
use std::path::PathBuf;

#[test]
fn produces_mcp_and_hook_overrides() {
    let bin = PathBuf::from("/usr/local/bin/agents-connector");
    let sock = PathBuf::from("/tmp/sock");
    let overrides = codex::config_overrides(&bin, &sock, "TOK-1");
    assert_eq!(overrides.len(), 4);

    // Joining all overrides should mention every required piece.
    let joined = overrides.join(" ");
    assert!(joined.contains("mcp_servers.agents_connector"));
    assert!(joined.contains("hooks.PostToolUse"));
    assert!(joined.contains("hooks.UserPromptSubmit"));
    assert!(joined.contains("--cli-kind codex"));
    assert!(joined.contains("--event post_tool_use"));
    assert!(joined.contains("--event user_prompt_submit"));
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test codex_adapter_test && cargo test --lib codex`
Expected: 1 + 2 tests pass (1 integration + 2 unit).

- [ ] **Step 4: Commit**

```bash
git add src/adapters/codex.rs tests/codex_adapter_test.rs
git commit -m "feat(adapters): codex -c overrides for MCP + PostToolUse/UserPromptSubmit hooks"
```

---

## Task 11: Wire Codex into `CliKind` + `launch_in_tmux`

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/adapters/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/launch.rs`

- [ ] **Step 1: Extend `CliKind` enum**

In `src/adapters/mod.rs`:
```rust
//! CLI adapters: per-CLI config generation for MCP and hooks.

pub mod claude;
pub mod codex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    Codex,
}

impl CliKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "claude" => Ok(CliKind::Claude),
            "codex" => Ok(CliKind::Codex),
            other => anyhow::bail!("unsupported cli kind: {}. Supported: claude, codex.", other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
        }
    }
}
```

- [ ] **Step 2: Wire Codex into `launch::launch_in_tmux`**

Replace the existing `CliKind::Codex` arm in `src/subcommands/launch.rs` (currently `bail!`) with:
```rust
CliKind::Codex => {
    let overrides = codex::config_overrides(&exe, broker_socket, &spec.token);
    // Build the codex command: each override goes through `-c <override>`.
    // The whole command is one shell string passed to tmux new-window, so we
    // shell-quote each override's value (the override string itself is TOML).
    let mut parts: Vec<String> = vec!["codex".into()];
    for ov in &overrides {
        parts.push("-c".into());
        parts.push(shell_quote(ov));
    }
    if let Some(dir) = spec.workdir.as_ref() {
        parts.push("--cd".into());
        parts.push(shell_quote(&dir.to_string_lossy()));
    }
    parts.join(" ")
}
```

Also, since we now handle `--cd` for Codex inside the match, **remove the outer `cd_prefix` logic** from `launch_in_tmux` for Codex agents only — but keep it for Claude (Claude has no `--cd` flag, so we use a `cd <dir> &&` shell prefix).

To keep the code clean, restructure `launch_in_tmux` like this:
```rust
pub fn launch_in_tmux(spec: &Spec, broker_socket: &Path) -> Result<()> {
    let agent_dir = paths::session_agent_dir(&spec.session, &spec.name)?;
    let exe = std::env::current_exe()?;

    let full_cmd = match spec.kind {
        CliKind::Claude => {
            let generated = claude::generate(&agent_dir, &exe, broker_socket, &spec.token)?;
            let claude_cmd = format!(
                "claude --mcp-config {} --settings {}",
                shell_quote(&generated.mcp_config_path.to_string_lossy()),
                shell_quote(&generated.settings_path.to_string_lossy())
            );
            // Claude has no --cd; use shell `cd && ` if workdir is set.
            match spec.workdir.as_ref() {
                Some(dir) => format!("cd {} && {}", shell_quote(&dir.to_string_lossy()), claude_cmd),
                None => claude_cmd,
            }
        }
        CliKind::Codex => {
            let overrides = codex::config_overrides(&exe, broker_socket, &spec.token);
            let mut parts: Vec<String> = vec!["codex".into()];
            for ov in &overrides {
                parts.push("-c".into());
                parts.push(shell_quote(ov));
            }
            if let Some(dir) = spec.workdir.as_ref() {
                parts.push("--cd".into());
                parts.push(shell_quote(&dir.to_string_lossy()));
            }
            parts.join(" ")
        }
    };

    tmux::new_window(&spec.session, &spec.name, &[], &full_cmd)
        .with_context(|| format!("spawning tmux window for agent `{}`", spec.name))?;
    Ok(())
}
```

Add the import:
```rust
use crate::adapters::{claude, codex, CliKind};
```

- [ ] **Step 3: Verify build + all tests**

Run: `cargo build && cargo test`
Expected: everything passes.

- [ ] **Step 4: Smoke-test the CLI dispatch**

Run: `cargo run -- add codex --name foo`
Expected output: an error like `broker not running for ...`. What this verifies: `CliKind::parse("codex")` succeeds and the dispatch reaches the broker-check.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/mod.rs src/subcommands/launch.rs
git commit -m "feat: wire codex adapter into CliKind and launch helper"
```

---

## Task 12: README + manual smoke test

**Files:**
- Modify: `/Users/frog/code/terminal_tool/README.md`

- [ ] **Step 1: Update README with new commands**

In `README.md`, replace the "Usage" and "Roadmap" sections:
```markdown
## Usage

```bash
# Start a session.
agents-connector start review-pod

# Inside the tmux session, add agents.
agents-connector add claude --name writer
agents-connector add codex --name reviewer

# Optional: tail the chat from another terminal.
agents-connector tail review-pod

# Refresh an agent's model context (same identity, chat history preserved):
agents-connector restart --name reviewer

# Remove an agent (kills the pane, frees the name):
agents-connector remove --name reviewer

# Stop the session (broker exits; tmux preserved unless --kill-tmux).
agents-connector stop review-pod

# Bring it back later, with all agents auto-relaunched:
agents-connector resume review-pod
# Or skip the auto-relaunch:
agents-connector resume review-pod --no-agents
```

In a Claude window, the `agents_connector` MCP server exposes tools:
- `tell(to, text, urgent)` — fire-and-forget message
- `ask(to, text)` — ask a question, get an `ask_id`
- `wait_for_reply(ask_id, timeout_ms)` — block until reply
- `check_replies(ask_id)` — non-blocking poll
- `read_messages(since)` — fetch messages since a high-water-mark
- `post_reply(ask_id, text)` — reply to an ask
- `list_agents()` — see who's in the session

Codex agents see the same tool surface. Codex doesn't currently have an end-of-turn hook, so Codex agents must call `read_messages` explicitly (your prompt should remind them).

## Status

**v0.2 (Plan 2)** — Codex CLI support, plus session lifecycle commands (`resume`, `restart`, `remove`).

## Roadmap

- Plan 3: Gemini adapter, tmux send-keys wake fallback for urgent messages.
- Plan 4: Packaging (Homebrew, prebuilt releases).
```

- [ ] **Step 2: Manual smoke test (real CLIs required)**

This is your local validation; not enforceable in CI. Steps:

1. `agents-connector start v2-test`
2. `agents-connector add claude --name alice`
3. `agents-connector add codex --name reviewer`
4. In alice's window: prompt to send a `tell` to `reviewer`
5. In reviewer's window: prompt to call `read_messages(since: 0)` and verify the message appears
6. `agents-connector restart --name reviewer` — pane respawns; reviewer's `read_messages(since: 0)` still returns alice's message (history preserved)
7. `agents-connector remove --name reviewer` — pane disappears; `agents-connector add claude --name reviewer` (new agent, same name) works because the old name was freed
8. Detach with `Ctrl-B D`. Run `agents-connector stop v2-test`. Run `agents-connector resume v2-test`. Both alice and the new reviewer should be back in their panes with chat history intact.

If any step fails, document the symptom + the relevant output in `NOTES.md` and either fix or file as a known issue.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: README for v0.2 (Plan 2)"
```

---

## Verification: definition of done

1. `cargo build --release` succeeds.
2. `cargo test` shows all tests passing (existing v1 + new v2 = roughly 30 tests).
3. Manual smoke test from Task 11 Step 2 works end-to-end.
4. `agents-connector --help` shows the three new subcommands (`resume`, `restart`, `remove`) plus the existing six.
5. `agents-connector add codex --name foo` no longer errors with "unsupported cli kind".

---

## What we explicitly didn't build (deferred to Plan 3)

- Gemini CLI adapter
- Tmux send-keys wake fallback (urgent messages waking idle CLIs without a hook)
- Codex hook integration (Codex's end-of-turn hooks, if/when its hook system arrives)
- Per-agent high-water-mark tracking on the broker (currently each agent manages its own `since` value)
- Packaging / Homebrew / GitHub releases (Plan 4)
