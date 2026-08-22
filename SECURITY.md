# Security Model

Token Poker is experimental software. Its Mental Poker implementation has not received an independent production cryptography audit and must not be used for real money or redeemable value.

## Reporting a vulnerability

Do not publish an unpatched vulnerability, working exploit, key material, or real user data in a public issue. Use the repository's **Security -> Report a vulnerability** form and include the affected version, minimal reproduction, impact assessment, and suggested mitigation.

If private vulnerability reporting is unavailable, open a public issue without technical exploit details and ask the maintainers for a private contact method. The project currently offers neither a response-time SLA nor a bug bounty.

## Protected assets

- Player root identity keys and recovery phrases.
- Process-scoped device keys and root-issued device certificates.
- Mental Poker ephemeral keys, shuffle proofs, reveal shares, and transcripts.
- Settlement receipts signed by every hand participant.
- Official Codex account-usage snapshots, account fingerprints, and observation times.
- Stable local libp2p node keys, volunteer consent, and successful-node cache entries.

## Trust boundaries

- Trust the locally installed MCP adapter, Rust sidecar, and byte-verified App Server binary copied from the user's current Codex desktop installation.
- Do not trust the table iframe, model output, remote players, bootstrap nodes, relay nodes, or archive nodes.
- The sandboxed UI can invoke only fixed usage-refresh, table-command, and polling tools. It cannot submit an arbitrary Token snapshot or read the raw App Server response.
- The MCP adapter reads account context only through the official `account/read` and `account/usage/read` methods.
- A libp2p PeerId identifies a transport session, not a player. Player authority requires a root-issued device certificate and a domain-separated signature.
- Discovery nodes provide dial hints. Relays forward Noise-protected traffic but can still observe IP addresses, timing, and traffic volume; relaying is not anonymity.
- Long-term statistics accept only settlement receipts signed by every participating device.

## Official account-usage minimization

The usage path must never use CDP, DevTools, DOM injection, response interception, cookies, authorization headers, or session tokens.

The MCP adapter must:

1. copy the official Codex binary during installation and verify the copy byte-for-byte;
2. complete the `initialize` / `initialized` handshake before calling only `account/read` and `account/usage/read`;
3. accept only a non-negative JavaScript safe integer for `lifetimeTokens`;
4. close the temporary App Server after the request; and
5. avoid persisting raw account responses or authentication material.

The email returned by `account/read` is normalized and domain-hashed in local memory. UI events, P2P messages, receipts, and archive data never contain the original email. The Token snapshot remains in shared-runtime memory and disappears when that runtime exits.

Official reading removes transcription and arbitrary-input errors for honest clients. It does not create an OpenAI-signed credential that opponents can verify, so protocol records retain `peer_verifiable: false`.

## Identity and recovery

The root key is unlocked only briefly to create a recovery package or issue a process-scoped device certificate. Active identity state does not retain the root secret.

Recovery packages use Argon2id key derivation and XChaCha20-Poly1305 encryption. The account-context fingerprint is authenticated as associated data. A remote archive sees only a derived locator and ciphertext; it cannot decrypt the root key without the recovery phrase.

A new device requires the same normalized Codex account context and the original recovery phrase. An email change, lost phrase, wrong phrase, or loss of every encrypted copy can make recovery impossible. Codex login itself is not a key-management service.

Device private keys are zeroized when the sidecar process exits. Reactivating an identity generates a new device key. Certificates may remain valid for 365 days even though their corresponding private keys usually disappear much sooner. The project intentionally does not maintain device history, revocation lists, or old-device approval.

## Cheating and failure behavior

- Invalid shuffle proofs, reveal shares, or transcript mutations abort the hand and produce no receipt.
- Withholding a share, disconnecting, or refusing settlement signatures aborts or disputes the hand. One player cannot create durable statistics alone.
- A malicious client can modify its local Token claim because opponents cannot independently verify the official account response. This is an acknowledged limitation.
- Archive loss is handled through multiple signed replica acknowledgements; no single archive provides an availability guarantee.
- Archive ciphertext tampering fails XChaCha20-Poly1305 authentication. A malicious archive can deny service but cannot forge a decryptable identity.
- Weak recovery phrases may be guessed offline despite the locator not being a bare username hash. Twelve characters is a protocol minimum, not a strength guarantee.
- Serverless public matchmaking cannot eliminate Sybil identities.

## Matchmaking and membership boundaries

- Community Rendezvous returns signed PeerRecords. Clients cap node count, result count, and addresses per player.
- Rendezvous, Kademlia, and AutoNAT infrastructure cannot issue matchmaking tickets on behalf of players.
- The stable local network PeerId stored under `%LOCALAPPDATA%\TokenHoldem` is a transport identity, not a player identity.
- The MCP adapter reaches the shared runtime through a per-user Windows named pipe and does not expose a localhost TCP service.
- Ticket, candidate, address, payload, and lifetime limits are enforced before allocation or signature verification.
- Tickets bind the player, device, session PeerId, dial addresses, stake, buy-in, lifetime, and nonce.
- Strictly signed Gossipsub messages must come from the session PeerId declared in the ticket.
- A proposal becomes local membership truth only after every listed player confirms the exact proposal containing their original ticket.

This is not Byzantine consensus. During partitions, peers may temporarily observe different candidate sets. The sidecar accepts the first cryptographically valid proposal containing its local ticket and rejects conflicting reuse of that ticket. It does not guarantee globally fair queuing or completion during a partition.

## Volunteer discovery and relay

- Volunteer participation is disabled until explicitly granted. Consent is local and is not synchronized to a project server.
- A failed host probe is handled conservatively: unknown conditions may provide discovery but never relay. Metered networking and battery power suspend volunteer service.
- A loaded service is not necessarily publicly reachable. Dedicated configuration, AutoNAT, UPnP, or a confirmed public external address is required before the UI reports an active public role.
- Relay reservations, concurrent circuits, duration, and transferred bytes are bounded. Relay traffic is encrypted end to end by libp2p Noise, but relay operators still observe metadata.

## Archive boundary

Archive nodes store opaque, encrypted identity packages and signed receipt bytes. They do not receive recovery phrases, root private keys, device private keys, hole cards, or raw Codex account responses. Clients verify archive signatures and content hashes before counting a replica.

## Release integrity

GitHub Releases provide an unsigned ZIP, a SHA-256 file, and `latest.json`. The checksum detects corruption and unintended byte changes. Because the checksum is distributed from the same account as the ZIP, it does not independently authenticate the publisher or protect against a compromised repository account.

The in-app updater stages downloads under `%LOCALAPPDATA%\TokenHoldem\updates`, validates the exact stable schema, semantic version, repository, Windows target, artifact name, size, and GitHub URLs, follows redirects only to GitHub release-asset origins, and verifies SHA-256 before enabling installation. A detached PowerShell helper rejects archive traversal and undeclared files, recomputes every package-manifest digest, and then invokes the existing Codex CLI installer. It never overwrites the active MCP or sidecar in place.

The update manifest and package are served by the same GitHub repository account. A compromised maintainer or repository account can therefore publish a malicious ZIP and matching digest. Automatic checking does not imply independent publisher authentication; installation always requires explicit user confirmation.
