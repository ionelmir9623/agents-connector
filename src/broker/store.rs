//! SQLite-backed agent registry for the broker.

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
    pub workdir: Option<String>,
}

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

    pub fn agent_by_token(&self, token: &str) -> Result<Option<Agent>> {
        let conn = self.conn.lock().unwrap();
        let agent = conn.query_row(
            "SELECT id, name, cli_kind, token, registered_at, removed_at, workdir FROM agents WHERE token = ?1 AND removed_at IS NULL",
            params![token],
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

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, cli_kind, token, registered_at, removed_at, workdir FROM agents WHERE removed_at IS NULL ORDER BY registered_at"
        )?;
        let rows = stmt.query_map([], |row| Ok(Agent {
            id: row.get(0)?,
            name: row.get(1)?,
            cli_kind: row.get(2)?,
            token: row.get(3)?,
            registered_at: row.get::<_, String>(4)?.parse::<DateTime<Utc>>().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            removed_at: row.get::<_, Option<String>>(5)?.map(|s| s.parse::<DateTime<Utc>>()).transpose().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            workdir: row.get::<_, Option<String>>(6)?,
        }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

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
}
