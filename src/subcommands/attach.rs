use crate::tmux;
use anyhow::{bail, Result};

pub fn run(session: &str) -> Result<()> {
    if !tmux::has_session(session)? {
        bail!("no tmux session `{}`. Use `agents-connector start {}` to create one.", session, session);
    }
    tmux::attach_session(session)?;
    Ok(())
}
