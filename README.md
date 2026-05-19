# agents-connector

A multi-agent CLI communication substrate. Lets multiple AI CLI agents
(Claude Code, Codex, Gemini CLI) running in separate tmux panes exchange
messages through a single shared session.

## Status

**v0.3 (Plan 3)** — Gemini CLI support, plus tmux send-keys wake fallback for urgent messages.

## Install

```bash
brew install tmux  # prerequisite
cargo install --path .
```

You'll also need at least one of the supported agent CLIs installed:
```bash
# Claude Code:    https://docs.claude.com/en/docs/claude-code
# Codex CLI:      brew install codex
# Gemini CLI:     brew install gemini-cli
```

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

When you `add` an agent, the launcher wires hooks so new messages are auto-injected into the agent's context — you don't need to prompt the agent to call `read_messages` manually.

- **Claude**: `Stop` hook (fires at end of every turn).
- **Codex**: `PostToolUse` and `UserPromptSubmit` hooks. Codex's `Stop` is fire-and-forget and cannot inject context, so we don't use it. Codex requires you to approve hooks once via `/hooks` in its TUI — they're persisted per-token after that.
- **Gemini**: `BeforeAgent` and `AfterTool` hooks. No approval gate, no feature flag.

### Urgent wake fallback

`tell(urgent=true)` causes the broker to additionally `tmux send-keys` against the recipient's pane. This works around the limitation that hooks only fire during active turns: if the recipient is idle at its prompt, the wake nudge causes its CLI to take a turn, which then triggers the auto-injection hook.

Best-effort caveats:
- Works cleanly when the recipient is idle at its prompt.
- May produce odd output if the recipient is mid-thinking or in a special TUI mode.
- Disabled in tests via `AGENTS_CONNECTOR_DISABLE_WAKE=1`.

## Architecture

See:
- `docs/superpowers/plans/2026-05-09-agents-connector-v1.md` — broker, IPC, MCP shim, basic adapters
- `docs/superpowers/plans/2026-05-09-agents-connector-v2.md` — Codex adapter, lifecycle subcommands
- `docs/superpowers/plans/2026-05-10-agents-connector-v3.md` — Gemini adapter, urgent wake
- `docs/integration-notes.md` — verified facts about each agent CLI's MCP and hook surfaces

## Roadmap

- Packaging: Homebrew tap, prebuilt GitHub release binaries (`cargo-dist`).

## License

Licensed under the [MIT License](LICENSE).
