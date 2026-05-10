# `agents-connector` v3 (Plan 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Gemini CLI as a third supported adapter (with auto-injection hooks) and add a tmux send-keys wake fallback so urgent messages can wake idle agents (Claude/Codex/Gemini alike) without depending on hooks alone.

**Architecture:** Extends v2 additively. Gemini gets its own `adapters/gemini.rs` that writes a project-local `<agent_dir>/.gemini/settings.json` (Gemini doesn't have a `-c` override or a custom-config-file flag, so config-via-cwd is the only clean option). Gemini's hooks are wired to `BeforeAgent` and `AfterTool` (both can inject `additionalContext`; Gemini's `Stop`-equivalent is `AfterAgent` which cannot inject). Wake fallback: when a `tell` or `ask` is sent with `urgent: true`, the broker shells out to `tmux send-keys -t <session>:<agent_name>` to nudge the recipient's pane — best-effort, may collide with active TUI state. The broker now needs to know the session name, so we pass `--session <name>` when the launcher spawns the broker daemon.

**Tech Stack:** Same as v2 — Rust, tokio, clap, rusqlite, rmcp 1.6. No new dependencies.

**Gemini CLI facts (verified against https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md and https://google-gemini.github.io/gemini-cli/docs/get-started/configuration.html — to be re-verified at Task 0 against the user's installed version):**

- Install: `brew install gemini-cli` or `npm install -g @google/gemini-cli`. Command name: `gemini`.
- Settings file: `~/.gemini/settings.json` (global) merged with `<cwd>/.gemini/settings.json` (project-local). MCP servers go under `mcpServers`. Hooks go under `hooks`.
- **No** per-invocation `-c key=value` flag. **No** `--settings <file>` flag. **No** `GEMINI_HOME` env var. Per-agent isolation must come from running each agent's gemini process in a per-agent cwd.
- Hook events: `BeforeAgent`, `AfterAgent`, `BeforeTool`, `AfterTool`, `SessionStart`, `SessionEnd`, `Notification`, `PreCompress`.
- `additionalContext` injection allowed by: `SessionStart`, `BeforeAgent`, `AfterTool` only.
- Hook output JSON does NOT require a `hookEventName` field (unlike Codex).
- No feature flag to enable hooks. No approval gate.
- `--include-directories <path>` adds a workspace directory; useful when the agent's cwd is its config dir but the user wants gemini to work in their project tree.

**Final state at end of plan:**
- `agents-connector add gemini --name researcher` works alongside `add claude` and `add codex`.
- A Gemini agent's `BeforeAgent` and `AfterTool` hooks auto-inject queued messages.
- `tell(urgent=true)` or `ask(urgent=true)` on the broker side fires `tmux send-keys` against the recipient's pane (best-effort, no guarantees if the recipient is mid-thinking).
- All v2 tests still pass; new tests cover the wake plumbing and Gemini adapter output.

---

## File structure

**Modified files:**
- `src/cli.rs` — add `--session <name>` to the hidden `Broker` subcommand
- `src/main.rs` — pass `session` through to broker
- `src/broker/server.rs` — `BrokerCtx` gains `session: Option<String>`; `serve` takes it
- `src/broker/handlers.rs` — `Tell` and `Ask` consult `urgent` flag and fire wake
- `src/broker/wake.rs` — NEW: tmux send-keys wrapper
- `src/broker/mod.rs` — declare `pub mod wake;`
- `src/subcommands/start.rs` — pass `--session <name>` when spawning broker
- `src/subcommands/resume.rs` — same
- `src/adapters/mod.rs` — extend `CliKind` with `Gemini` variant
- `src/subcommands/launch.rs` — wire Gemini branch
- `src/hook/mod.rs` — accept Gemini events (`before_agent`, `after_tool`) and emit Gemini's payload shape
- `docs/integration-notes.md` — fill in Gemini section with verified facts
- `README.md` — usage examples covering Gemini + urgent wake

**New files:**
- `src/adapters/gemini.rs` — Gemini settings.json generator
- `tests/gemini_adapter_test.rs`
- `tests/wake_test.rs` — exercises the urgent flag path with a mock-tmux env var

---

## Sequencing

```
0. Verify Gemini install + re-confirm doc-derived facts against current binary
1. Update docs/integration-notes.md with verified Gemini facts
2. Hook subcommand: handle Gemini event names + Gemini output schema
3. Gemini adapter: write settings.json generator + tests
4. Wire Gemini into CliKind + launch_in_tmux
5. Wake plumbing: broker learns session name; tmux send-keys helper
6. Wake firing: urgent flag on Tell/Ask triggers wake; wake_test
7. README + manual smoke test
```

---

## Task 0: Verify Gemini CLI install + re-confirm doc facts

**Files:** none (verification only).

The plan-author already researched Gemini's hook surface (see header). Task 0 is the implementer cross-checking against the locally installed version.

- [ ] **Step 1: Confirm Gemini CLI is installed**

Run: `gemini --version`
Expected: a version string. If absent, install:
```bash
brew install gemini-cli
# OR
npm install -g @google/gemini-cli
```

- [ ] **Step 2: Confirm hook event names**

Visit https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md (or `gemini --help` if it has a hooks section). Confirm:
- Hook events include `BeforeAgent`, `AfterTool`, `SessionStart` (the three we'll use).
- Output schema for `BeforeAgent` and `AfterTool` is `{ "hookSpecificOutput": { "additionalContext": "..." } }`.
- No feature flag is required.
- No `hookEventName` field is required in the output (unlike Codex).

If anything has materially changed since the plan was written, stop and update Tasks 2-4 to match.

- [ ] **Step 3: Confirm working-directory model**

Run: `gemini --help | head -40`. Confirm:
- No `--settings <file>` flag exists.
- No `--config <file>` flag exists.
- `--include-directories` exists (we'll use it to give gemini access to user's project tree while running from a per-agent cwd).

If gemini has gained a `--settings <file>` flag in a recent release, the plan should be revised to use it (cleaner than per-agent cwd) — stop and adjust Tasks 3-4.

- [ ] **Step 4: No commit yet — proceed when checks pass.**

---

## Task 1: Update `docs/integration-notes.md` Gemini section

**Files:** Modify `/Users/frog/code/terminal_tool/docs/integration-notes.md`

The notes file currently has a Gemini placeholder. Replace it with the verified facts from Task 0.

- [ ] **Step 1: Replace the placeholder**

Find the `## Gemini CLI` section (currently a "TBD" stub) and replace with:

```markdown
## Gemini CLI

**Docs:** https://github.com/google-gemini/gemini-cli (README) | https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md (hooks) | https://google-gemini.github.io/gemini-cli/docs/get-started/configuration.html (config)
**Install:** `brew install gemini-cli` or `npm install -g @google/gemini-cli`
**Command:** `gemini`
**Settings file:** `~/.gemini/settings.json` (global) merged with `<cwd>/.gemini/settings.json` (project-local). No per-invocation `-c` override and no `--settings <file>` flag — config is purely file-based.
**Per-invocation isolation:** must come from running gemini in a per-agent cwd that contains its own `.gemini/settings.json`.
**Working-dir flag:** no `--cd <path>`. To give gemini access to a different directory tree while running from a per-agent cwd, use `--include-directories <path>`.
**Verified against:** gemini-cli (version per Task 0), 2026-05-10.

### Hooks

| Event | Can inject `additionalContext`? | Notes |
|---|---|---|
| `SessionStart` | Yes | Fires when the session begins/resumes |
| `BeforeAgent` | Yes | What we use — fires before the model takes a turn (covers human-typed-prompt case) |
| `AfterTool` | Yes | What we use — fires after every tool call |
| `BeforeTool` | (only blocks/permits) | Cannot inject context |
| `AfterAgent` | No | Gemini's "Stop"-equivalent. Cannot inject. |
| `SessionEnd` | No | Cleanup only |
| `Notification` | No | UI events |
| `PreCompress` | No | Compression event |

**Output schema (BeforeAgent / AfterTool):** Note the absence of `hookEventName` (unlike Codex):
```json
{
  "hookSpecificOutput": {
    "additionalContext": "..."
  }
}
```

**Settings file shape (`<cwd>/.gemini/settings.json`):**
```json
{
  "mcpServers": {
    "agents_connector": {
      "command": "/path/to/agents-connector",
      "args": ["mcp-shim", "--socket", "...", "--agent-token", "..."],
      "env": {}
    }
  },
  "hooks": {
    "BeforeAgent": [{
      "matchers": ["*"],
      "command": "/path/to/agents-connector hook --socket ... --agent-token ... --event before_agent --cli-kind gemini"
    }],
    "AfterTool": [{
      "matchers": ["*"],
      "command": "/path/to/agents-connector hook --socket ... --agent-token ... --event after_tool --cli-kind gemini"
    }]
  }
}
```

**Required to enable:** Nothing. Hooks fire by default once configured in settings.

**Approval gate:** None.
```

- [ ] **Step 2: Commit**

```bash
cd /Users/frog/code/terminal_tool
git add docs/integration-notes.md
git commit -m "docs: integration-notes: fill in verified Gemini hook facts"
```

---

## Task 2: Hook subcommand — handle Gemini events + payload shape

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/hook/mod.rs`

The hook subcommand currently dispatches by `(cli_kind, event)`. Extend it to recognize `gemini` as a `cli_kind`, with events `before_agent` and `after_tool` (snake_case on the CLI side, mapped to PascalCase event names ... actually Gemini doesn't require `hookEventName` in output, so we don't need a mapping — the snake_case → PascalCase conversion is only relevant for Codex).

- [ ] **Step 1: Update `injects_context` to recognize Gemini events**

Replace the existing `injects_context` function in `src/hook/mod.rs`:
```rust
fn injects_context(cli_kind: &str, event: &str) -> bool {
    match (cli_kind, event) {
        ("claude", "stop") => true,
        ("codex", "post_tool_use") => true,
        ("codex", "user_prompt_submit") => true,
        ("gemini", "before_agent") => true,
        ("gemini", "after_tool") => true,
        ("gemini", "session_start") => true,
        _ => false,
    }
}
```

- [ ] **Step 2: Update `format_payload` to emit Gemini's schema**

Replace `format_payload` and the `codex_event_name` helper:
```rust
fn format_payload(cli_kind: &str, event: &str, text: &str) -> serde_json::Value {
    match cli_kind {
        // Claude Code: flat top-level field.
        "claude" => serde_json::json!({ "additionalContext": text }),
        // Codex: nested under hookSpecificOutput, requires hookEventName.
        "codex" => {
            let hook_event_name = codex_event_name(event);
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": hook_event_name,
                    "additionalContext": text,
                }
            })
        }
        // Gemini CLI: nested under hookSpecificOutput, NO hookEventName field.
        "gemini" => serde_json::json!({
            "hookSpecificOutput": {
                "additionalContext": text,
            }
        }),
        _ => serde_json::json!({}),
    }
}

fn codex_event_name(event: &str) -> &'static str {
    match event {
        "post_tool_use" => "PostToolUse",
        "user_prompt_submit" => "UserPromptSubmit",
        "session_start" => "SessionStart",
        "stop" => "Stop",
        _ => "Unknown",
    }
}
```

- [ ] **Step 3: Add a unit test for Gemini payload shape**

At the bottom of `src/hook/mod.rs`, add:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_payload_omits_hook_event_name() {
        let payload = format_payload("gemini", "before_agent", "hello");
        assert_eq!(payload["hookSpecificOutput"]["additionalContext"], "hello");
        assert!(payload["hookSpecificOutput"]["hookEventName"].is_null());
    }

    #[test]
    fn codex_payload_includes_hook_event_name() {
        let payload = format_payload("codex", "post_tool_use", "hello");
        assert_eq!(payload["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(payload["hookSpecificOutput"]["additionalContext"], "hello");
    }

    #[test]
    fn claude_payload_is_flat() {
        let payload = format_payload("claude", "stop", "hello");
        assert_eq!(payload["additionalContext"], "hello");
        assert!(payload["hookSpecificOutput"].is_null());
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cd /Users/frog/code/terminal_tool && cargo test --lib hook
```
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/hook/mod.rs
git commit -m "feat(hook): support gemini events (before_agent, after_tool) and payload"
```

---

## Task 3: Gemini adapter — settings.json generator

**Files:**
- Create: `/Users/frog/code/terminal_tool/src/adapters/gemini.rs`
- Create: `/Users/frog/code/terminal_tool/tests/gemini_adapter_test.rs`

The Gemini adapter writes a project-local `.gemini/settings.json` inside the agent_dir. When gemini launches with cwd=agent_dir, it picks up these settings and merges with the user's global config.

- [ ] **Step 1: Implement `src/adapters/gemini.rs`**

```rust
//! Gemini CLI adapter.
//!
//! Gemini reads MCP servers and hooks from `<cwd>/.gemini/settings.json` (merged with
//! `~/.gemini/settings.json`). It has no per-invocation `-c` override and no
//! `--settings <file>` flag, so per-agent isolation requires running gemini in a
//! per-agent cwd. We write the agent's settings to `<agent_dir>/.gemini/settings.json`
//! and the launcher cd's into `<agent_dir>` before exec-ing gemini.
//!
//! Schema reference: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

pub struct Generated {
    /// The agent_dir itself — gemini should be launched with this as cwd
    /// so its `.gemini/settings.json` is discovered as project-local config.
    pub launch_cwd: PathBuf,
}

/// Writes `<agent_dir>/.gemini/settings.json` with our MCP server entry and
/// the `BeforeAgent` + `AfterTool` hooks pointing at our hook subcommand.
pub fn generate(
    agent_dir: &Path,
    binary_path: &Path,
    socket_path: &Path,
    agent_token: &str,
) -> Result<Generated> {
    let dot_gemini = agent_dir.join(".gemini");
    std::fs::create_dir_all(&dot_gemini)?;

    let bin = binary_path.to_string_lossy().to_string();
    let sock = socket_path.to_string_lossy().to_string();

    let hook_before_agent = format!(
        "{} hook --socket {} --agent-token {} --event before_agent --cli-kind gemini",
        bin, sock, agent_token
    );
    let hook_after_tool = format!(
        "{} hook --socket {} --agent-token {} --event after_tool --cli-kind gemini",
        bin, sock, agent_token
    );

    let settings = json!({
        "mcpServers": {
            "agents_connector": {
                "command": bin,
                "args": [
                    "mcp-shim",
                    "--socket", sock,
                    "--agent-token", agent_token,
                ],
                "env": {}
            }
        },
        "hooks": {
            "BeforeAgent": [{
                "matchers": ["*"],
                "command": hook_before_agent,
            }],
            "AfterTool": [{
                "matchers": ["*"],
                "command": hook_after_tool,
            }],
        }
    });

    let settings_path = dot_gemini.join("settings.json");
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;

    Ok(Generated { launch_cwd: agent_dir.to_path_buf() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_settings_with_mcp_and_hooks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_dir = tmp.path().join("alice");
        let bin = PathBuf::from("/usr/local/bin/agents-connector");
        let sock = PathBuf::from("/tmp/sock");
        let result = generate(&agent_dir, &bin, &sock, "TOK-G").unwrap();

        // Returned cwd is the agent_dir.
        assert_eq!(result.launch_cwd, agent_dir);

        // settings.json was written under .gemini/
        let settings_path = agent_dir.join(".gemini").join("settings.json");
        assert!(settings_path.exists());

        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();

        // MCP server present.
        let mcp = &parsed["mcpServers"]["agents_connector"];
        assert_eq!(mcp["command"], "/usr/local/bin/agents-connector");
        assert_eq!(mcp["args"][0], "mcp-shim");
        assert_eq!(mcp["args"][2], "/tmp/sock");
        assert_eq!(mcp["args"][4], "TOK-G");

        // Both hooks present and point at our hook subcommand with correct event + cli-kind.
        let before = &parsed["hooks"]["BeforeAgent"][0]["command"];
        let after = &parsed["hooks"]["AfterTool"][0]["command"];
        assert!(before.as_str().unwrap().contains("--event before_agent"));
        assert!(before.as_str().unwrap().contains("--cli-kind gemini"));
        assert!(after.as_str().unwrap().contains("--event after_tool"));
        assert!(after.as_str().unwrap().contains("--cli-kind gemini"));
    }
}
```

- [ ] **Step 2: Add a public adapter test**

Create `tests/gemini_adapter_test.rs`:
```rust
use agents_connector::adapters::gemini;
use std::path::PathBuf;

#[test]
fn writes_settings_with_required_keys() {
    let tmp = tempfile::TempDir::new().unwrap();
    let agent_dir = tmp.path().join("alice");
    let bin = PathBuf::from("/usr/local/bin/agents-connector");
    let sock = PathBuf::from("/tmp/sock");

    let result = gemini::generate(&agent_dir, &bin, &sock, "TOK").unwrap();
    assert!(agent_dir.join(".gemini/settings.json").exists());
    assert_eq!(result.launch_cwd, agent_dir);

    let body = std::fs::read_to_string(agent_dir.join(".gemini/settings.json")).unwrap();
    assert!(body.contains("\"agents_connector\""));
    assert!(body.contains("\"BeforeAgent\""));
    assert!(body.contains("\"AfterTool\""));
}
```

- [ ] **Step 3: Run tests**

```bash
cd /Users/frog/code/terminal_tool && cargo test gemini
```
Expected: 2 tests pass (1 unit, 1 integration).

- [ ] **Step 4: Commit**

```bash
git add src/adapters/gemini.rs tests/gemini_adapter_test.rs
git commit -m "feat(adapters): gemini settings.json with MCP + BeforeAgent/AfterTool hooks"
```

---

## Task 4: Wire Gemini into `CliKind` + `launch_in_tmux`

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/adapters/mod.rs`
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/launch.rs`

- [ ] **Step 1: Extend `CliKind` enum**

In `src/adapters/mod.rs`:
```rust
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
```

- [ ] **Step 2: Wire Gemini branch in `launch::launch_in_tmux`**

In `src/subcommands/launch.rs`, update the import:
```rust
use crate::adapters::{claude, codex, gemini, CliKind};
```

Add a `CliKind::Gemini` arm to the `match spec.kind`:
```rust
CliKind::Gemini => {
    // Gemini reads .gemini/settings.json from cwd. We write it to agent_dir
    // and cd into agent_dir before launching. The user's project workdir (if any)
    // is added via --include-directories so gemini can read/write there.
    let generated = gemini::generate(&agent_dir, &exe, broker_socket, &spec.token)?;
    let mut parts: Vec<String> = vec![
        format!("cd {} &&", shell_quote(&generated.launch_cwd.to_string_lossy())),
        "gemini".into(),
    ];
    if let Some(dir) = spec.workdir.as_ref() {
        parts.push("--include-directories".into());
        parts.push(shell_quote(&dir.to_string_lossy()));
    }
    parts.join(" ")
}
```

- [ ] **Step 3: Verify build + tests**

```bash
cd /Users/frog/code/terminal_tool && cargo build && cargo test
```
Expected: everything passes.

- [ ] **Step 4: Smoke test the CLI dispatch**

```bash
cargo run -- add gemini --name foo
```
Expected: an error like `broker not running`. Confirms `CliKind::parse("gemini")` succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/mod.rs src/subcommands/launch.rs
git commit -m "feat: wire gemini adapter into CliKind and launch helper"
```

---

## Task 5: Wake plumbing — broker learns session name + tmux helper

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/cli.rs` (add `--session` to `Broker` subcommand)
- Modify: `/Users/frog/code/terminal_tool/src/main.rs` (pass session through)
- Modify: `/Users/frog/code/terminal_tool/src/broker/server.rs` (`BrokerCtx` carries session)
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/start.rs` (pass `--session` when spawning broker)
- Modify: `/Users/frog/code/terminal_tool/src/subcommands/resume.rs` (same)
- Create: `/Users/frog/code/terminal_tool/src/broker/wake.rs`
- Modify: `/Users/frog/code/terminal_tool/src/broker/mod.rs`

The broker currently knows nothing about tmux. To send-keys to an agent's pane, it needs the session name. Pass it as a launch arg.

- [ ] **Step 1: Add `--session` to the Broker subcommand**

In `src/cli.rs`, replace the `Broker` variant:
```rust
    /// Internal: run the broker daemon. Users should not invoke directly.
    #[command(hide = true)]
    Broker {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        db: std::path::PathBuf,
        #[arg(long)]
        session: Option<String>,
    },
```

- [ ] **Step 2: Pass `session` through in `src/main.rs`**

Replace the `Command::Broker` arm:
```rust
Command::Broker { socket, db, session } => {
    use agents_connector::broker::{server, store::Store};
    use std::sync::Arc;
    let store = Arc::new(Store::open(&db)?);
    server::serve(store, &socket, session).await?;
    Ok(())
}
```

- [ ] **Step 3: Update `BrokerCtx` and `serve`**

In `src/broker/server.rs`, add a field and update `new` + `serve`:
```rust
pub struct BrokerCtx {
    pub store: Arc<Store>,
    pub reply_notifiers: Mutex<HashMap<i64, broadcast::Sender<()>>>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub message_stream: broadcast::Sender<MessageDto>,
    pub session: Option<String>,
}

impl BrokerCtx {
    pub fn new(store: Arc<Store>, shutdown_tx: broadcast::Sender<()>, session: Option<String>) -> Self {
        let (msg_tx, _) = broadcast::channel::<MessageDto>(256);
        Self {
            store,
            reply_notifiers: Mutex::new(HashMap::new()),
            shutdown_tx,
            message_stream: msg_tx,
            session,
        }
    }
    // ... existing methods unchanged
}
```

Update the `serve` signature:
```rust
pub async fn serve(store: Arc<Store>, socket_path: &Path, session: Option<String>) -> Result<()> {
    // ... existing body, but the line that creates ctx changes:
    let ctx = Arc::new(BrokerCtx::new(store, shutdown_tx.clone(), session));
    // ... rest unchanged
}
```

- [ ] **Step 4: Update existing tests that call `server::serve`**

In `tests/broker_ipc_test.rs` and `tests/e2e_test.rs`, the `spawn_test_broker` / `spawn_broker` helpers call `server::serve(store, &sock_clone)`. Add a third arg `None`:
```rust
server::serve(store, &sock_clone, None).await.unwrap();
```

- [ ] **Step 5: Pass `--session` in `start.rs`**

In `src/subcommands/start.rs`, find the `Command::new(&exe).args(["broker", ...])` call and add `--session`:
```rust
let child = Command::new(&exe)
    .args([
        "broker",
        "--socket", &socket.to_string_lossy(),
        "--db", &db.to_string_lossy(),
        "--session", session,
    ])
    // ...
```

(Note: variable `session` is `&str` here.)

- [ ] **Step 6: Same in `resume.rs`**

Add `"--session", session,` to the analogous broker-spawn call in `src/subcommands/resume.rs`.

- [ ] **Step 7: Implement `src/broker/wake.rs`**

```rust
//! Tmux send-keys wake helper.
//!
//! When an `urgent` message arrives, the broker fires this against the recipient's
//! tmux pane to nudge an idle CLI into taking a turn. Best-effort — succeeds even
//! if tmux isn't running or the target pane doesn't exist (logs and moves on).
//!
//! Set `AGENTS_CONNECTOR_DISABLE_WAKE=1` to no-op (used by tests).

use std::process::Command;

/// Send `text` followed by Enter to `<session>:<agent_name>`.
pub fn nudge(session: &str, agent_name: &str, text: &str) {
    if std::env::var("AGENTS_CONNECTOR_DISABLE_WAKE").is_ok() {
        return;
    }
    let target = format!("{}:{}", session, agent_name);
    let result = Command::new("tmux")
        .args(["send-keys", "-t", &target, text, "Enter"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!(target = %target, status = ?s, "tmux send-keys non-zero"),
        Err(e) => tracing::warn!(target = %target, error = %e, "tmux send-keys failed"),
    }
}
```

- [ ] **Step 8: Add `pub mod wake;` to `src/broker/mod.rs`**

```rust
//! Broker daemon: SQLite store + IPC server.

pub mod handlers;
pub mod server;
pub mod store;
pub mod wake;
```

- [ ] **Step 9: Verify everything still compiles and tests pass**

```bash
cd /Users/frog/code/terminal_tool && cargo build && cargo test
```
Expected: everything passes (no behavioral change yet — wake is plumbing only; firing comes in Task 6).

- [ ] **Step 10: Commit**

```bash
git add src/cli.rs src/main.rs src/broker/ src/subcommands/start.rs src/subcommands/resume.rs tests/
git commit -m "feat(broker): plumb session name + add tmux send-keys wake helper"
```

---

## Task 6: Wake firing — urgent flag triggers tmux send-keys

**Files:**
- Modify: `/Users/frog/code/terminal_tool/src/broker/handlers.rs`
- Create: `/Users/frog/code/terminal_tool/tests/wake_test.rs`

The broker's `Tell` and `Ask` handlers receive the `urgent` flag (Tell already has it; Ask does not but is best-effort and treated as non-urgent — keep that behavior). When `urgent` is true and we have a session name and a recipient name, fire wake.

- [ ] **Step 1: Update `Tell` handler in `src/broker/handlers.rs`**

Find the `Request::Tell` arm. Replace its body with:
```rust
Request::Tell { from, to, text, urgent } => {
    let from_dto = from.clone();
    let to_dto = to.clone();
    let text_dto = text.clone();
    let urgent_recipient = if urgent { to.clone() } else { None };
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
            if let (Some(session), Some(agent)) = (ctx.session.as_deref(), urgent_recipient.as_deref()) {
                crate::broker::wake::nudge(session, agent, "[agents-connector] urgent message — please check");
            }
            Response::TellAck { message_id }
        }
        Err(e) => Response::Error { message: format!("{:#}", e) },
    }
}
```

(Broadcast urgents — `to: None` — are not waked because there's no specific recipient. That's a deliberate v3 simplification; broadcasts skip the wake path.)

- [ ] **Step 2: Add a wake test using the disable env var**

Create `tests/wake_test.rs`:
```rust
//! Verifies the urgent flag wake path:
//! - Plumbs through Tell when urgent=true and to is set.
//! - Does NOT plumb through when urgent=false.
//! - Does NOT plumb through when to is None (broadcast).
//! - Always no-ops when AGENTS_CONNECTOR_DISABLE_WAKE is set.
//!
//! These tests don't actually invoke tmux — they set the disable env var so the
//! wake helper short-circuits. We're verifying the dispatch logic, not tmux's
//! behavior (that's only verifiable in a real terminal).

use agents_connector::broker::{server, store::Store};
use agents_connector::ipc::{read_frame_async, write_frame_async, Request, Response};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::UnixStream;

#[tokio::test]
async fn urgent_tell_completes_with_wake_disabled() {
    std::env::set_var("AGENTS_CONNECTOR_DISABLE_WAKE", "1");

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, Some("test-session".into())).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    // Send an URGENT tell; with wake disabled this should still ack normally.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::Tell {
        from: "alice".into(),
        to: Some("bob".into()),
        text: "WAKE UP".into(),
        urgent: true,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::TellAck { message_id } => assert!(message_id > 0),
        other => panic!("unexpected: {:?}", other),
    }

    std::env::remove_var("AGENTS_CONNECTOR_DISABLE_WAKE");
}
```

- [ ] **Step 3: Run all tests**

```bash
cd /Users/frog/code/terminal_tool && cargo test
```
Expected: every existing test still passes; new wake test passes (1).

- [ ] **Step 4: Commit**

```bash
git add src/broker/handlers.rs tests/wake_test.rs
git commit -m "feat(broker): fire tmux send-keys wake when urgent=true on Tell"
```

---

## Task 7: README + manual smoke test

**Files:**
- Modify: `/Users/frog/code/terminal_tool/README.md`

- [ ] **Step 1: Update Status, Usage, Roadmap**

Find the existing v0.2 sections and replace with v0.3 versions.

```markdown
## Status

**v0.3 (Plan 3)** — Gemini CLI support, plus tmux send-keys wake fallback for urgent messages.

## Usage

```bash
# Start a session.
agents-connector start review-pod

# Add agents (any combo of claude / codex / gemini):
agents-connector add claude --name writer
agents-connector add codex --name reviewer-1
agents-connector add gemini --name reviewer-2

# Optional: tail the chat from another terminal.
agents-connector tail review-pod

# Refresh an agent's model context (same identity, chat history preserved):
agents-connector restart --name reviewer-1

# Remove an agent (kills the pane, frees the name):
agents-connector remove --name reviewer-2

# Stop the session (broker exits; tmux preserved unless --kill-tmux).
agents-connector stop review-pod

# Bring it back later, with all agents auto-relaunched:
agents-connector resume review-pod
# Or skip the auto-relaunch:
agents-connector resume review-pod --no-agents
```

In each agent window, the `agents_connector` MCP server exposes tools:
- `tell(to, text, urgent)` — fire-and-forget message; `urgent=true` triggers a tmux send-keys wake against the recipient's pane (best-effort)
- `ask(to, text)` — ask a question, get an `ask_id`
- `wait_for_reply(ask_id, timeout_ms)` — block until reply
- `check_replies(ask_id)` — non-blocking poll
- `read_messages(since)` — fetch messages since a high-water-mark
- `post_reply(ask_id, text)` — reply to an ask
- `list_agents()` — see who's in the session

### Hook-based auto-injection

When you `add` an agent, the launcher wires hooks so new messages are auto-injected into the agent's context:

- **Claude**: `Stop` hook (fires at end of every turn).
- **Codex**: `PostToolUse` and `UserPromptSubmit` hooks. Codex's `Stop` is fire-and-forget and cannot inject context, so we don't use it. Codex requires you to approve hooks once via `/hooks` in its TUI.
- **Gemini**: `BeforeAgent` and `AfterTool` hooks. No approval gate, no feature flag.

### Urgent wake fallback

`tell(urgent=true)` causes the broker to additionally `tmux send-keys` against the recipient's pane. Best-effort: works cleanly when the recipient is idle at its prompt; may produce odd output if the recipient is mid-thinking or in a special TUI mode.

## Roadmap

- Plan 4: Packaging (Homebrew tap, prebuilt GitHub releases).
```

- [ ] **Step 2: Final verification**

```bash
cd /Users/frog/code/terminal_tool
cargo build --release
cargo test
agents-connector --help | head -20  # confirms gemini command works in dispatch
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: README for v0.3 (Plan 3)"
```

- [ ] **Step 4: Manual smoke test (optional)**

Real-world test:
1. `cargo install --path .`
2. `agents-connector start v3-test`
3. `agents-connector add claude --name writer`
4. `agents-connector add gemini --name reviewer`
5. In writer's pane: prompt to send a `tell` to `reviewer` with `urgent: true`.
6. Watch reviewer's pane — the urgent wake should type a system note + Enter, gemini should take a turn, BeforeAgent hook fires, message gets injected, gemini responds.
7. Check `~/.agents-connector/sessions/v3-test/hook_log.txt` for the gemini event.

---

## Verification: definition of done

1. `cargo build --release` succeeds.
2. `cargo test` shows all tests passing (existing + new ~33 tests).
3. `agents-connector --help` shows the same 9 subcommands.
4. `agents-connector add gemini --name foo` no longer errors with "unsupported cli kind".
5. `agents-connector add claude --name x` followed by sending an urgent tell from another agent does NOT error (wake is best-effort, may or may not visually wake; that's fine).
6. `docs/integration-notes.md` Gemini section is filled in with verified facts.

---

## What we explicitly didn't build (deferred to Plan 4)

- Packaging / Homebrew formula / GitHub Actions release pipeline
- Per-agent high-water-mark tracking on the broker (each agent still manages its own `since`)
- `wait_for_reply` urgent-coupled wake (currently only `Tell.urgent` triggers wake; making `ask`'s wake-the-recipient option is a small follow-up)
- Wake for broadcast messages (Tell with `to: None` doesn't wake; deliberately scoped out)
- Idle-detection heuristics on the recipient pane (currently always send-keys regardless of pane state)
- Networked / multi-machine support
- Authentication / multi-tenant
