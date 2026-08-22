# Token Poker Plugin Distribution

The release is an unsigned Windows x64 Codex plugin, not a standalone desktop application. After installation, open the game from a new Codex task. No game launcher or system Node.js installation is required.

## Install a release ZIP

1. Download the same-version ZIP, `.zip.sha256`, and `latest.json` from the official GitHub Release.
2. Verify SHA-256 and completely extract the ZIP.
3. Open PowerShell in the extracted directory and run:

```powershell
.\install-token-poker.ps1
```

4. Create a new Codex task and ask:

```text
Open Token Poker and read my official lifetime Token.
```

Upgrade an existing installation with:

```powershell
.\install-token-poker.ps1 -Upgrade
```

The installer uses official `codex plugin marketplace` and `codex plugin add` commands, synchronizes the versioned plugin payload, and copies the current desktop Codex App Server into the versioned cache. It does not modify `PATH`, write registry settings, install Node.js, start a Windows service, or configure startup tasks.

The shared game runtime starts only when Token Poker is opened. After the last plugin view closes, an idle runtime with no matchmaking or active table exits after 30 minutes.

## Runtime resolution

The MCP launcher resolves Node in this order:

1. an MCP runtime explicitly supplied by Codex;
2. the runtime managed by Codex desktop; and
3. system `node` as a source-development fallback only.

The release does not contain Node.js, npm, `node_modules`, or the Codex App Server. The installer copies the App Server from the user's current Codex installation because Windows Store package restrictions may prevent launching it in place.

## Task switching

Codex currently mounts third-party plugin UI inside a task. The UI cannot remain globally pinned while switching tasks. Token Poker keeps the P2P session in a separate per-user runtime, so closing one task does not immediately stop the game.

Ask a new task to restore Token Poker. The MCP adapter attaches through the Windows named pipe, replays current state, and refreshes official usage. A complete Codex or Windows shutdown does not guarantee restoration of an unsettled hand.

## Official account usage

The MCP adapter starts the copied official App Server, calls `account/read` and `account/usage/read`, validates the response, and closes the temporary process. The UI can only request that fixed refresh flow.

No CDP, DevTools, DOM injection, cookies, authorization headers, local chat estimation, or manual Token entry is used. Lifetime Token is official service-side usage data but is not a third-party-verifiable OpenAI credential.

## Network cold start

The release contains a versioned community-node directory. When at least one listed node is online, users can cold-start without entering an address. The project does not operate matchmaking, game, relay, or archive servers; listed nodes are volunteer infrastructure.

Local state under `%LOCALAPPDATA%\TokenHoldem` contains volunteer consent, successful-node cache entries, and a stable libp2p node key. It does not contain a player root key, recovery phrase, process device key, or hand receipt.

## Update manifest

Each release includes `latest.json`:

```json
{
  "schema_version": 1,
  "channel": "stable",
  "version": "0.3.0",
  "tag": "v0.3.0",
  "repository": "rainyflash/token-poker",
  "artifacts": [
    {
      "target": "windows-x64",
      "name": "token-poker-plugin-v0.3.0-windows-x64.zip",
      "bytes": 123,
      "sha256": "..."
    }
  ]
}
```

The stable discovery URL is:

```text
https://github.com/rainyflash/token-poker/releases/latest/download/latest.json
```

Consumers must reject unknown schemas, malformed semantic versions, unexpected repositories, unsupported targets, invalid sizes, and SHA-256 mismatches. An updater must stage files and wait for the active plugin processes to exit before installation.

## Integrity and risk

```powershell
(Get-FileHash .\token-poker-plugin-v0.3.0-windows-x64.zip -Algorithm SHA256).Hash.ToLowerInvariant()
Get-Content .\token-poker-plugin-v0.3.0-windows-x64.zip.sha256
```

The digests must match. SHA-256 verifies bytes, not publisher identity. The ZIP, Rust binaries, and PowerShell installer are not Authenticode-signed, so Windows may display an unknown-publisher warning.

Mental Poker remains unaudited and must not be used for real money or redeemable value. Read `SECURITY.md` before distributing a modified build.
