# agents-connector

A multi-agent CLI communication substrate. Lets multiple AI CLI agents
(Claude Code, eventually Codex, Gemini CLI) running in separate tmux panes
exchange messages through a single shared session.

## Status

**v0.1 (Phase 1)** — Two Claude Code instances can chat via `tell`/`ask`/`reply`.

## Install

```bash
brew install tmux  # prerequisite
cargo install --path .
```

## Usage

```bash
# Start a session.
agents-connector start review-pod

# Inside the tmux session, add agents.
agents-connector add claude --name writer
agents-connector add claude --name reviewer

# Optional: tail the chat from another terminal.
agents-connector tail review-pod

# Shut down when done.
agents-connector stop review-pod
```

In a Claude window, the `agents_connector` MCP server exposes tools:
- `tell(to, text, urgent)` — send a fire-and-forget message
- `ask(to, text)` — ask a question, get an `ask_id`
- `wait_for_reply(ask_id, timeout_ms)` — block until reply arrives
- `check_replies(ask_id)` — non-blocking poll for replies
- `read_messages(since)` — fetch messages since a high-water-mark
- `post_reply(ask_id, text)` — reply to an outstanding ask
- `list_agents()` — see who's in the session

## Architecture

See `docs/superpowers/plans/2026-05-09-agents-connector-v1.md` for the implementation plan.

## Roadmap

- Phase 2: Codex adapter, `resume`/`restart`/`remove` subcommands.
- Phase 3: Gemini adapter, tmux send-keys wake fallback.
- Phase 4: Packaging (Homebrew, prebuilt releases).
