# Contributing to Token Poker

Token Poker welcomes bug fixes, protocol review, test vectors, documentation, localization, and accessibility improvements. This project handles identity keys, Codex MCP permissions, peer-to-peer networking, and experimental cryptography. A pull request that hides new business logic inside a React component or bypasses protocol validation will not be accepted.

## Development environment

The repository pins Rust `1.97.1` and Node.js `24.1.0`. Windows is the primary supported platform, and PowerShell changes must pass both syntax validation and the Windows CI job.

```powershell
rustup show active-toolchain
node --version

Push-Location ui
npm ci
Pop-Location

Push-Location plugins/token-holdem/mcp
npm ci
Pop-Location
```

## Architecture rules

- Dependencies point inward: UI and network adapters -> application use cases -> domain.
- The domain layer must not import React, MCP, libp2p, Codex, or storage implementations.
- External capabilities require an application port and an adapter. Test doubles must not leak into production domain models.
- Do not introduce TypeScript `any`, swallowed errors, implicit persistence, or unbounded network input.
- Never place player private keys, recovery phrases, complete Codex account responses, or private card material in logs, fixtures, screenshots, or issues.
- Localization keys belong in the typed i18n catalog. Do not add language conditionals throughout components.

## Pull request workflow

1. Update the relevant protocol document when behavior or wire compatibility changes.
2. Keep each commit focused. Delete obsolete code instead of commenting it out.
3. Run the complete quality gate:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
./scripts/check-syntax.ps1
python ./scripts/check-source-language.py
node scripts/verify-p2p-hand.mjs
node scripts/verify-complete-session.mjs

Push-Location ui
npm run build
npm run lint
npm test
Pop-Location
```

4. If canonical encoding, signature domains, protocol versions, or signed structures change, explicitly regenerate `test-vectors/protocol-8/core.json`, explain the diff, and prove that incompatible old vectors fail as expected.
5. A pull request description must state the behavioral change, boundary cases, verification evidence, and remaining risks.

## Release discipline

The release is an unsigned ZIP, not a standalone installer. A release tag must match the workspace version in every manifest. GitHub Releases are generated only by the checked-in workflow.

Never replace an existing release asset while keeping its old SHA-256. A checksum verifies byte integrity; it does not authenticate the publisher.

Contributions are accepted under `MIT OR Apache-2.0` unless explicitly stated otherwise by the contributor.
