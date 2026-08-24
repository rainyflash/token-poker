---
name: token-holdem
description: "Open or operate Token Poker inside Codex; the plugin reads lifetime Token through the official Codex App Server."
---

# Token Poker Agent Workflow

## When to use this skill

Use this skill when the user explicitly asks to open, play, match, create a private room, or refresh the Token Poker balance. Do not start the table when the user is only discussing gameplay, architecture, or security.

## Open and refresh

1. Resolve and call `token_holdem_open`. Codex may expose it lazily under the fully qualified name `mcp__token_holdem__token_holdem_open`.
2. If the short name is not visible, use the host's available or deferred-tool discovery to search for that exact fully qualified name or the `token_holdem_open` suffix. Do not report that the plugin is missing, unloaded, incompatible, or in use by another agent before completing this lookup.
3. The tool attaches to the shared game runtime and reads lifetime Token through the official `account/usage/read` method.
4. Do not open the profile page, transcribe screenshots, estimate usage, or convert another metric into Token.
5. When the user explicitly requests a refresh, resolve and call `token_holdem_refresh_official_usage`; its fully qualified name may be `mcp__token_holdem__token_holdem_refresh_official_usage`. The table refresh control invokes the same tool.

When the user asks to restore a table from another task, call `token_holdem_open` again. It reattaches to the current Windows user's shared runtime. Another task or agent may already be attached; this does not reserve, lock, or consume the tool or runtime. Do not claim that the plugin UI itself stays globally mounted across tasks.

## Trust boundary

- Describe the value as server-side statistics returned by the official Codex account-usage API.
- Do not call it an OpenAI-signed proof or a balance that opponents can verify independently.
- Do not describe chips as purchasable, withdrawable, redeemable, transferable, or valuable.
- The table may call only the fixed official usage-refresh tool and cannot submit an arbitrary Token value.

## Failure behavior

Classify failures by the layer that actually failed:

- **Tool discovery:** Only after the exact deferred-tool lookup fails may you say that this task did not load the Token Poker tool. Recommend a full Codex restart and a new task, and preserve the distinction from a runtime or account-usage failure.
- **Tool invocation or shared-runtime attach:** Report the concrete MCP startup, named-pipe, or runtime error returned by the call. Do not describe it as another agent owning the game.
- **Official account usage:** If `token_holdem_open` succeeds but the official API is unavailable, authentication fails, or the value is missing, report that specific account-usage compatibility error and recommend updating Codex.

Never fall back to screenshots, chat history, local-session estimation, or manual input. Never emit a prewritten compatibility diagnosis without first performing the applicable lookup or tool call.
