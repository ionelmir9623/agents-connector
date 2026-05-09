//! Filesystem layout helpers.

use std::path::PathBuf;

/// Root directory: `~/.agents-connector/` (or `$XDG_DATA_HOME/agents-connector/`).
pub fn root() -> anyhow::Result<PathBuf> {
    if let Ok(override_path) = std::env::var("AGENTS_CONNECTOR_HOME") {
        if !override_path.is_empty() {
            return Ok(PathBuf::from(override_path));
        }
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
    #[serial_test::serial]
    fn root_respects_override_env_var() {
        std::env::set_var("AGENTS_CONNECTOR_HOME", "/tmp/test-ac");
        assert_eq!(root().unwrap(), PathBuf::from("/tmp/test-ac"));
        std::env::remove_var("AGENTS_CONNECTOR_HOME");
    }

    #[test]
    #[serial_test::serial]
    fn session_paths_compose_correctly() {
        std::env::set_var("AGENTS_CONNECTOR_HOME", "/tmp/test-ac");
        assert_eq!(session_dir("demo").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo"));
        assert_eq!(session_db("demo").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo/db.sqlite"));
        assert_eq!(session_socket("demo").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo/broker.sock"));
        assert_eq!(session_agent_dir("demo", "alice").unwrap(), PathBuf::from("/tmp/test-ac/sessions/demo/agents/alice"));
        std::env::remove_var("AGENTS_CONNECTOR_HOME");
    }

    #[test]
    #[serial_test::serial]
    fn empty_override_falls_back_to_home_dir() {
        std::env::set_var("AGENTS_CONNECTOR_HOME", "");
        let r = root().unwrap();
        assert!(r.is_absolute(), "root() must return an absolute path; got {:?}", r);
        assert!(r.ends_with(".agents-connector"), "root() should fall back to ~/.agents-connector; got {:?}", r);
        std::env::remove_var("AGENTS_CONNECTOR_HOME");
    }
}
