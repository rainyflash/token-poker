# Changelog

All notable changes to Token Poker are documented here. The project follows [Semantic Versioning](https://semver.org/).

## [0.4.15] - 2026-08-26

### Fixed

- Make the UI hand state monotonic, so delayed key-exchange, shuffle, or private-deal progress can no longer replace an already playable betting state.
- Keep the shared-runtime and MCP recovery projections on the newest hand and discard stale protocol progress after private cards or betting state become authoritative.
- Accept signed settlement receipts within a bounded sixty-second clock-skew window, preventing millisecond-level differences between player devices from freezing receipt consensus and the next hand.

### Added

- Add regression coverage for out-of-order hand events, stale previous-hand events, same-sequence betting-state enrichment, and bounded receipt clock skew.

## [0.4.14] - 2026-08-26

### Fixed

- Synchronize signed matchmaking tickets and table advertisements directly between connected players, so Circuit Relay connectivity no longer depends on Gossipsub mesh timing.
- Coalesce unchanged matchmaking-directory events instead of flooding the runtime log while a player waits.
- Warn explicitly when the same persistent player identity attempts to occupy two devices at once.

### Changed

- Advance the control and Identify protocol generation to 11 for the new reliable matchmaking-directory exchange.
- Verify two-player convergence with matchmaking Gossipsub deliberately disabled, both directly and through Circuit Relay.

## [0.4.13] - 2026-08-24

### Fixed

- Preserve the signed table-admission endpoint when moving from matchmaking into a room.
- Deliver the initial join intent directly over the reliable request-response protocol while retaining Gossipsub as redundant propagation.
- Retain and actively dial the admission peer until the local player appears in a signed membership proposal.
- Remove the duplicate friend-room dial now owned by the session admission boundary.

### Added

- Fault-inject room-topic Gossipsub loss during the complete two-player session test.
- Repeat the fault-injected matchmaking and complete-hand test through a Circuit Relay topology in CI.

## [0.4.12] - 2026-08-24

### Fixed

- Give the real Windows PowerShell launcher smoke test a sixty-second process budget so a loaded GitHub Windows runner cannot reject a valid helper startup at ten seconds.
- Report installer timeouts without claiming the production five-minute duration when a caller deliberately configures a different deadline.

## [0.4.11] - 2026-08-24

### Fixed

- Require a request-correlated game-core acknowledgement before the UI treats a leave-table command as accepted, while restoring the action on an explicit failure.
- Keep leave controls locked only for the current room so rejoining a table cannot inherit stale pending state.
- Wait for the signed membership certificate to remove a departing player before closing the local room, avoiding stale seats for the remaining players.
- Deliver signed leave intents through the reliable consensus path in addition to bounded Gossipsub retries.
- Make the forward-update release gate generate canonical ZIP paths on both Windows PowerShell 5.1 and PowerShell 7, while preserving updater diagnostics on failure.

### Changed

- A connected player leaves after the current hand and membership convergence instead of disappearing from local UI state immediately.
- A player who signs a leave intent and then disconnects for ten seconds deterministically aborts the unfinished hand. The aborted hand creates no settlement receipt and does not affect persistent statistics; remaining players reform the room for the next hand.
- Add bounded local cleanup for a stalled hand, stalled membership convergence, and an absolute safe-leave deadline.
- Advance the public network protocol to version 10, gameplay topic namespaces to version 2, and the shared-runtime protocol to version 6 so incompatible clients cannot silently mix.

### Added

- Added protocol-10 fixed vectors and a two-peer integration test covering normal boundary leave, disconnect abandonment, survivor convergence, and the absence of an incomplete-hand receipt.
- Added UI and MCP regression coverage for correlated leave confirmation, retry behavior, remount cleanup, and aborted-hand projection.

## [0.4.10] - 2026-08-24

### Fixed

- Require a correlated acknowledgement from the local game core before automatic identity creation can report success.
- Retry transient identity initialization failures with a bounded backoff while reusing the same recovery secret.
- Restore the active player identity, matchmaking state, room, and hand after Codex remounts the plugin or the diagnostic event buffer is truncated.
- Hydrate every newly mounted Codex view from sequence zero instead of trusting a stale watermark replayed by the host.
- Keep an identity attempt alive while its own confirmation updates React state, preserving the recovery kit generated for that identity.

### Changed

- Advance the shared-runtime protocol to version 5 so this release cannot attach to an incompatible older game core.

### Added

- Added regression coverage for correlated command results, bounded identity retries, current-state projection, remount hydration, and cross-conversation recovery.

## [0.4.9] - 2026-08-24

### Fixed

- Teach the bundled agent workflow to discover lazy plugin tools before diagnosing a cross-task load failure.

## [0.4.8] - 2026-08-23

### Fixed

- Launch the Windows updater as a supervised child process so PowerShell cannot report success without executing the installer script.
- Isolate Windows PowerShell 5.1 from Codex's bundled PowerShell 7 module path, restoring built-in commands such as `Get-FileHash`.
- Wait for the atomic updater result to become visible after process exit instead of misreporting a short filesystem delay as `ENOENT`.
- Include the updater exit status and log path in missing-result errors, and preserve PowerShell invocation and stack details in the install log.
- Replace a probabilistic roster-coordinator fixture that could randomly fail an otherwise valid release build.

### Added

- Added a real Windows PowerShell regression test that launches from Node.js with a deliberately poisoned `PSModulePath`.
- Added one checked version command for the Cargo workspace, npm packages, plugin manifest, and lockfiles.

### Upgrade note

- Versions 0.4.7 and earlier contain the broken updater launcher and cannot reliably repair themselves online. Install 0.4.8 once from the extracted release ZIP; online updates after 0.4.8 use the corrected launcher.

## [0.4.7] - 2026-08-23

### Fixed

- Treat `pagehide`, hidden visibility, and document freeze as temporary suspension instead of terminating the MCP session.
- Recreate inline resize observation and force a fresh React snapshot when a suspended Token Poker view becomes visible again.
- Reserve terminal cleanup for the MCP Apps `ui/resource-teardown` lifecycle signal.
- Advertise only the inline and fullscreen display modes currently supported by the Codex Desktop host.

### Added

- Added regression coverage for page hide/show, visibility restoration, resize observer recovery, and non-terminal polling continuity.

### Known limitation

- Codex Desktop can still request an MCP App resource before `thread/resume` completes when reopening a task. The request is rejected before it reaches Token Poker; reopening Token Poker after the task resumes restores the shared runtime. See [openai/codex#34195](https://github.com/openai/codex/issues/34195).

## [0.4.6] - 2026-08-23

### Fixed

- Stop orphaned UI polling when Codex tears down an iframe, unloads a task, or reports a terminal `thread not found` error.
- Abort the underlying MCP request on timeout or teardown instead of leaving hidden calls alive after the UI disappears.
- Disable automatic size observation in fullscreen and picture-in-picture modes, while retaining explicit resize notifications for inline rendering.
- Keep a visible boot or render failure panel when the host bridge or React tree cannot initialize instead of presenting a blank page.
- Guard the connection handshake so an iframe closed during startup cannot issue late display-mode or polling requests.

### Added

- Added executable lifecycle regression tests for teardown, terminal host errors, request cancellation, display-mode resizing, and broken UI dependencies.
- Added a forward-update contract test so every release proves it can install its immediate successor before publication.

## [0.4.4] - 2026-08-23

### Fixed

- Wait for the isolated updater to finish and verify its version-matched result before asking the user to restart Codex.
- Store every verified marketplace payload in a persistent content-addressed directory instead of registering an extracted download folder.
- Verify the Codex marketplace root, installed plugin version, source path, and cache manifest before reporting a successful update.
- Keep the running MCP alive during cross-version installation so it can observe and report updater failures.

## [0.4.3] - 2026-08-23

### Added

- Added a 30-second action clock for every seat, with an automatic check when no call is required and an automatic fold otherwise.
- Added dealer, small-blind, big-blind, and latest-action indicators to every player seat.

### Fixed

- Made safe leave immediately visible and disabled repeated leave or betting actions while the current hand finishes.
- Displayed the local player's committed chips and latest action without allowing the action console to cover them.
- Rejected stale shared runtimes after the hand-state protocol gained action-clock and seat-action fields.

## [0.4.2] - 2026-08-22

### Fixed

- Fixed Windows desktop installation when Codex is installed but the optional `codex` CLI command is absent from `PATH`.
- Added direct Windows App package discovery and a verified temporary executable bootstrap for official plugin registration commands.

### Changed

- Moved Codex runtime discovery and bootstrap cleanup into a separately tested installer component.

## [0.4.1] - 2026-08-22

### Added

- Added a double-click Windows installer entrypoint with package-completeness checks, persistent logs, visible success and failure states, and automated regression tests.

### Changed

- Made the PowerShell installation core automatically choose first installation, same-version repair, or cross-version upgrade.

## [0.4.0] - 2026-08-22

### Added

- Added an in-app, user-confirmed updater with isolated download staging, package-manifest validation, and SHA-256 verification.
- Added a copied Codex App Server runtime for reliable access to official lifetime Token usage on Windows Store installations.

### Changed

- Moved release packaging to the `token-poker-plugin-v<version>-windows-x64.zip` naming contract.

## [0.3.1] - 2026-08-22

### Changed

- Pinned `rust-libp2p` to security-fixed official upstream revision `170c3c81ddd80e7c58b0500563e00a09139e8545` until the 0.57 release is published.
- Updated the Rust MSRV declaration to 1.88 and adapted UPnP and Relay event handling to the new upstream API.

### Security

- Removed vulnerable `yamux 0.12.1` and upgraded the dependency graph to `yamux 0.14.0`, addressing GHSA-vxx9-2994-q338.
- Upgraded `hickory-proto` from 0.25.2 to 0.26.1, addressing GHSA-q2qq-hmj6-3wpp.
- Verified that the upgraded client can reserve Relay capacity and register through both TCP and QUIC against the existing Singapore community node.

## [0.3.0] - 2026-08-22

### Added

- English and Simplified Chinese UI with system-language detection and a persistent manual override.
- A fourth, higher reasoning stake tier and 100x stake scaling across all tiers.
- Dynamic public tables that accept two to six players without splitting matchmaking by table size.
- A machine-readable `latest.json` release manifest for safe update discovery.
- A reproducible Windows x64 plugin package and GitHub Actions release pipeline.
- Explicit public-address advertisement for Docker-based community relay nodes.
- An end-to-end Circuit Relay reservation regression test.

### Changed

- Renamed the product from Token Hold'em to Token Poker.
- Reworked the lobby, table, identity, and statistics views around Codex design tokens.
- Moved task-to-task game continuity into one per-user runtime reached through a Windows named pipe.
- Made identity creation automatic after the Codex account fingerprint becomes available.

### Security

- Official lifetime Token is read only through the Codex App Server `account/usage/read` method.
- Plugin releases include per-file and archive SHA-256 metadata.
- Release ZIPs remain unsigned; checksums verify integrity, not publisher identity.

[0.4.14]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.14
[0.4.13]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.13
[0.4.12]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.12
[0.4.11]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.11
[0.4.10]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.10
[0.4.9]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.9
[0.4.8]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.8
[0.4.7]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.7
[0.4.6]: https://github.com/rainyflash/token-poker/compare/v0.4.4...7c7a9cd
[0.4.4]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.4
[0.4.3]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.3
[0.4.2]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.2
[0.4.1]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.1
[0.4.0]: https://github.com/rainyflash/token-poker/releases/tag/v0.4.0
[0.3.1]: https://github.com/rainyflash/token-poker/releases/tag/v0.3.1
[0.3.0]: https://github.com/rainyflash/token-poker/releases/tag/v0.3.0
