//! CLI adapters: per-CLI config generation for MCP and hooks.

pub mod claude;
pub mod codex;
pub mod gemini;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    Codex,
    Gemini,
}

impl CliKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "claude" => Ok(CliKind::Claude),
            "codex" => Ok(CliKind::Codex),
            "gemini" => Ok(CliKind::Gemini),
            other => anyhow::bail!("unsupported cli kind: {}. Supported: claude, codex, gemini.", other),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
            CliKind::Gemini => "gemini",
        }
    }
}
