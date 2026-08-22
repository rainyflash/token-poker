---
name: token-holdem
description: "Open or operate Token Poker inside Codex; the plugin reads lifetime Token through the official Codex App Server."
---

# Token Poker Agent Workflow

## When to use this skill

Use this skill when the user explicitly asks to open, play, match, create a private room, or refresh the Token Poker balance. Do not start the table when the user is only discussing gameplay, architecture, or security.

## Open and refresh

1. Call `token_holdem_open`. It attaches to the shared game runtime and reads lifetime Token through the official `account/usage/read` method.
2. Do not open the profile page, transcribe screenshots, estimate usage, or convert another metric into Token.
3. When the user explicitly requests a refresh, call `token_holdem_refresh_official_usage`. The table refresh control invokes the same tool.

When the user asks to restore a table from another task, call `token_holdem_open` again. It reattaches to the current Windows user's shared runtime. Do not claim that the plugin UI itself stays globally mounted across tasks.

## Trust boundary

- Describe the value as server-side statistics returned by the official Codex account-usage API.
- Do not call it an OpenAI-signed proof or a balance that opponents can verify independently.
- Do not describe chips as purchasable, withdrawable, redeemable, transferable, or valuable.
- The table may call only the fixed official usage-refresh tool and cannot submit an arbitrary Token value.

## Failure behavior

If the official API is unavailable, authentication fails, or the value is missing, report the compatibility error and recommend updating Codex. Never fall back to screenshots, chat history, local-session estimation, or manual input.
