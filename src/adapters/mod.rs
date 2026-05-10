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
