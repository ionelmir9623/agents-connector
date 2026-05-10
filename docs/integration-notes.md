# Integration notes

Verified facts about each CLI we integrate with. Update whenever a new fact is confirmed against current docs / behavior.

For each entry: cite the docs URL, the version verified against, and the date.

---

## Claude Code

**Docs:** https://docs.claude.com/en/docs/claude-code/hooks
**MCP config:** `~/.claude/.mcp.json` or `--mcp-config <file>` flag
**Settings file:** `--settings <file>` flag accepts a JSON file with `hooks`, `env`, etc.
**Verified against:** v1 implementation 2026-05-09; smoke test confirmed Stop hook fires.

### Hooks

| Event | Can inject `additionalContext`? | Notes |
|---|---|---|
| `Stop` | Yes (top-level field) | What we use for v1 — fires at end of every turn |
| `UserPromptSubmit` | Yes | Available; not used in v1 |
| `PostToolUse` | Yes | Available; not used in v1 |

**Output schema (Stop):** flat top-level field
```json
{ "additionalContext": "..." }
```

**Settings file shape (`settings.json`) for a Stop hook:**
```json
{
  "hooks": {
    "Stop": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "/path/to/script --args ..."
      }]
    }]
  }
}
```

**Required to enable:** nothing — hooks fire by default once configured in settings.

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

**Docs:** TBD (research at start of Plan 3)
**MCP support:** TBD
**Hook support:** TBD
**Verified against:** TBD

To be filled in when implementing the Gemini adapter (Plan 3). Research checklist:
1. Does `gemini --help` mention MCP? Find the actual MCP config location.
2. Does Gemini have a hook system? Find the docs URL.
3. If so: which events can inject context? What's the output schema?
4. Per-invocation config override or env var (analogous to `CODEX_HOME` / `--mcp-config`)?
5. Working directory flag?

---

## Conventions

- When a fact in this doc changes due to an upstream version bump, update the version + date stamp at the top of the section.
- Cite the docs URL where each fact came from. If a fact came from running the binary or reading source, note that.
- "Verified" means we've actually tested the integration end-to-end at least once. "Documented" means the docs say so but we haven't tested.
