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
