# agents-connector

A multi-agent CLI communication substrate. Lets multiple AI CLI agents
(Claude Code, eventually Codex, Gemini CLI) running in separate tmux panes
exchange messages through a single shared session.

## Status

**v0.2 (Plan 2)** — Codex CLI support, plus session lifecycle commands (`resume`, `restart`, `remove`).

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
agents-connector add codex --name reviewer

# Optional: tail the chat from another terminal.
agents-connector tail review-pod

# Refresh an agent's model context (same identity, chat history preserved):
agents-connector restart --name reviewer

# Remove an agent (kills the pane, frees the name):
agents-connector remove --name reviewer

# Stop the session (broker exits; tmux preserved unless --kill-tmux).
agents-connector stop review-pod

# Bring it back later, with all agents auto-relaunched:
agents-connector resume review-pod
# Or skip the auto-relaunch:
agents-connector resume review-pod --no-agents
```

In each agent window, the `agents_connector` MCP server exposes tools:
- `tell(to, text, urgent)` — fire-and-forget message
- `ask(to, text)` — ask a question, get an `ask_id`
- `wait_for_reply(ask_id, timeout_ms)` — block until reply
- `check_replies(ask_id)` — non-blocking poll
- `read_messages(since)` — fetch messages since a high-water-mark
- `post_reply(ask_id, text)` — reply to an ask
- `list_agents()` — see who's in the session

### Hook-based auto-injection

When you `add` a Claude or Codex agent, the launcher wires hooks so new messages are auto-injected into the agent's context — you don't need to prompt the agent to call `read_messages` manually.

- **Claude**: uses Claude Code's `Stop` hook (fires at end of every turn).
- **Codex**: uses Codex's `PostToolUse` and `UserPromptSubmit` hooks. Codex's `Stop` hook is fire-and-forget and cannot inject context, so we don't use it.

If you want the agent to also see messages that arrive while it's idle (waiting at its prompt with no active turn), you'll need to nudge it — `tmux send-keys` wake fallback is on the Plan 3 roadmap.

## Architecture

See `docs/superpowers/plans/2026-05-09-agents-connector-v1.md` for the implementation plan.

## Roadmap

- Plan 3: Gemini adapter, tmux send-keys wake fallback for urgent messages.
- Plan 4: Packaging (Homebrew, prebuilt releases).
