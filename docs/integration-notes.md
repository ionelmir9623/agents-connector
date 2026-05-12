# Integration notes

Verified facts about each CLI we integrate with. Update whenever a new fact is confirmed against current docs / behavior.

For each entry: cite the docs URL, the version verified against, and the date.

---

## Claude Code

**Docs:** https://code.claude.com/docs/en/hooks
**MCP config:** `~/.claude/.mcp.json` or `--mcp-config <file>` flag
**Settings file:** `--settings <file>` flag accepts a JSON file with `hooks`, `env`, etc.
**Verified against:** 2026-05-11; confirmed Stop hook fires but does NOT inject additionalContext (see correction below).

### Hooks

| Event | Can inject `additionalContext`? | Notes |
|---|---|---|
| `SessionStart` | Yes | What we use — fires once when the agent boots; perfect for the welcome DM |
| `UserPromptSubmit` | Yes | What we use — fires when the user submits a prompt |
| `PostToolUse` | Yes | What we use — fires after every tool call |
| `PreToolUse` | Yes | Available; not used |
| `UserPromptExpansion` | Yes | Available; not used |
| `Stop` | **NO** | Supports only top-level `decision: "block"`. Cannot inject context. **v1 incorrectly used this and was silently no-op.** |
| `SubagentStop` | No | Same as Stop — block-only |
| `PreCompact` / `ConfigChange` / `PostToolUseFailure` / `PostToolBatch` | No | Block-only |

**Output schema (UserPromptSubmit / PostToolUse / SessionStart):** nested with required `hookEventName` field — identical shape to Codex's hooks.
```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "..."
  }
}
```

**Settings file shape (`settings.json`) — three hooks:**
```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "/path/to/script --event user_prompt_submit --cli-kind claude ..."
      }]
    }],
    "PostToolUse": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "/path/to/script --event post_tool_use --cli-kind claude ..."
      }]
    }],
    "SessionStart": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "/path/to/script --event session_start --cli-kind claude ..."
      }]
    }]
  }
}
```

**Required to enable:** nothing — hooks fire by default once configured in settings.

**Stop-hook correction:** Earlier versions of this doc claimed Stop supports flat `{"additionalContext": "..."}` injection. That was wrong. Per https://code.claude.com/docs/en/hooks the "Decision control" table lists Stop alongside other block-only events. Stop accepts only `{"decision": "block", "reason": "..."}` and ignores everything else. Confirmed empirically: hook log showed Stop firing and emitting the JSON, but Claude's next turn did not see the additionalContext.

---

## Codex CLI

**Docs:** https://developers.openai.com/codex/hooks (+ https://developers.openai.com/codex/cli/reference for flags)
**MCP config:** `~/.codex/config.toml` under `[mcp_servers.<name>]` (or per-invocation `-c key=value` overrides)
**Per-invocation override:** `-c key=value` (dotted path; value parsed as TOML); repeatable
**Working directory flag:** `--cd <path>`
**Config home env var:** `CODEX_HOME` (defaults to `~/.codex`); auth lives there too, so overriding it requires also providing/symlinking auth.json
**Verified against:** codex-cli 0.130.0, 2026-05-10

### Hooks

| Event | Can inject `additionalContext`? | Notes |
|---|---|---|
| `SessionStart` | Yes | Fires when the session begins/resumes |
| `UserPromptSubmit` | Yes | What we use — fires when user types to the agent |
| `PostToolUse` | Yes | What we use — fires after every tool call |
| `Stop` | **NO** | Fire-and-forget; cannot inject. Don't use for context injection. |
| `PreToolUse` | (only blocks/permits) | Cannot inject context |
| `PermissionRequest` | (only blocks/permits) | Cannot inject context |

**Output schema (UserPromptSubmit / PostToolUse):** nested with **required `hookEventName`** field
```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "..."
  }
}
```

The `hookEventName` value is **PascalCase** matching the event ("UserPromptSubmit", "PostToolUse", "SessionStart", etc.). Codex rejects payloads without it: `error: hook returned invalid user prompt submit JSON output`.

**TOML schema for hooks** (in `config.toml` or via `-c`):
```toml
[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/path/to/script --args ..."
```

Note the **two levels of nesting**: each `[[hooks.<event>]]` is a *group* containing a `hooks` array of actual hook entries. Each entry needs `type = "command"` and `command`.

**Required to enable:** `[features] hooks = true` (formerly `codex_hooks` — the old name still works but emits a deprecation warning). Equivalent CLI flag: `--enable hooks`.

**Approval gate:** Codex requires explicit user approval per hook before it runs. Open `/hooks` in the codex TUI to review and approve. Persisted per-token, so survives `restart`.

**Inline form for `-c` overrides:**
```
-c features.hooks=true
-c 'hooks.UserPromptSubmit=[{ hooks = [{ type = "command", command = "..." }] }]'
```

**Known issue (#17532):** Repo-local `.codex/config.toml` hooks may not fire in interactive sessions even when correctly configured. Workaround: use global `~/.codex/config.toml` or per-invocation `-c` overrides. We use `-c` overrides.

### Other Codex notes

- `codex` (no subcommand) = interactive TUI; `codex exec` = non-interactive.
- `--ignore-user-config` skips loading global config.toml but still uses CODEX_HOME for auth.
- `--json` flag streams events to stdout as JSONL; **not** a hook substitute — it's read-only event output.

---

## Gemini CLI

**Docs:** https://github.com/google-gemini/gemini-cli (README) | https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md (hooks) | https://google-gemini.github.io/gemini-cli/docs/get-started/configuration.html (config)
**Install:** `brew install gemini-cli` or `npm install -g @google/gemini-cli`
**Command:** `gemini`
**Settings file:** `~/.gemini/settings.json` (global) merged with `<cwd>/.gemini/settings.json` (project-local). No per-invocation `-c` override and no `--settings <file>` flag — config is purely file-based.
**Per-invocation isolation:** must come from running gemini in a per-agent cwd that contains its own `.gemini/settings.json`.
**Working-dir flag:** no `--cd <path>`. To give gemini access to a different directory tree while running from a per-agent cwd, use `--include-directories <path>`.
**Verified against:** gemini 0.41.2, 2026-05-10.

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

**`gemini hooks` subcommand:** exists in 0.41.2 with only a `migrate` subcommand (converts Claude Code hooks). Not used by us — we write the settings.json directly.

---

## Conventions

- When a fact in this doc changes due to an upstream version bump, update the version + date stamp at the top of the section.
- Cite the docs URL where each fact came from. If a fact came from running the binary or reading source, note that.
- "Verified" means we've actually tested the integration end-to-end at least once. "Documented" means the docs say so but we haven't tested.
