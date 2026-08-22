# Changelog

All notable changes to Token Poker are documented here. The project follows [Semantic Versioning](https://semver.org/).

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

[0.3.1]: https://github.com/rainyflash/token-poker/releases/tag/v0.3.1
[0.3.0]: https://github.com/rainyflash/token-poker/releases/tag/v0.3.0
