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
