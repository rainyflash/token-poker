# Public Matchmaking Protocol

## Scope

Public matchmaking does not depend on a project-operated server. Clients exchange messages through configured Rendezvous and Kademlia bootstrap nodes, volunteer Circuit Relays, and existing libp2p connections. Discovery infrastructure forwards data but owns no player keys and cannot choose the table, seat order, or buy-in.

This protocol ends when every player confirms the same table membership. Mental Poker, action ordering, receipt signing, and the next-hand barrier belong to the table protocol.

## Discovery layer

Clients merge sources in this order: explicit command-line addresses, the bundled community directory, then the validated 30-day cache. Limits are eight Rendezvous nodes, four relays, and sixteen archive entries.

After connecting to Rendezvous, a client registers a PeerRecord signed by its libp2p network identity. Incremental discovery runs every 20 seconds and accepts at most 256 players and eight addresses per player. Discovery records are only dial hints for Kademlia, explicit Gossipsub peers, and direct connections. They do not establish player identity or table membership.

Public peers that advertise Rendezvous or Relay Hop through Identify may become additional candidates. Only publicly usable addresses that complete Rendezvous registration or a relay reservation enter the cache. Private, CGNAT, link-local, and relayed addresses never become cold-start entries.

Community nodes may censor, delay, or return Sybil peers, but they cannot forge device-signed tickets. Generic STUN is not a direct input to the current libp2p TCP/QUIC implementation.

## Signed objects

| Object | Signer | Bound fields |
| --- | --- | --- |
| `MatchTicket` v2 | Ticket device | Player, device, session PeerId, up to eight dial addresses, stake, buy-in, player count, timestamps, and nonce |
| `SignedMatchProposal` v2 | Any ticket device in the group | Canonically ordered tickets, player set, proposal digest, and proposer ticket |
| `MatchAcceptance` v1 | Device for each seat | Proposal digest, original ticket, player, device, and acceptance time |

Each object uses a separate signature domain. A ticket signature cannot be reused for a proposal or acceptance. The persistent root identity signs the device certificate attached to each ticket.

## Deterministic grouping

1. Validate versions, device certificates, signatures, lifetimes, buy-ins, player counts, and dial addresses.
2. Keep only tickets compatible with the local stake and target size.
3. If one `PlayerId` publishes multiple tickets, keep the lowest TicketId.
4. Sort tickets by TicketId and partition them into fixed-size groups. An incomplete tail continues waiting.
5. Ticket order becomes seat order. Any member may sign the same canonical proposal without changing its table ID.

The hexadecimal proposal digest is the stable table ID.

A candidate group must remain stable for 750 ms on the local monotonic clock before rank zero may propose. Every 1.5 seconds, the next ranked member becomes eligible to take over. Monotonic time controls only local send eligibility and never enters the proposal digest.

## Sidecar state machine

```text
idle -> searching -> proposing -> confirming -> ready
  ^          |            |            |
  +----------+------------+------------+-- cancel
             +-- ticket expiry -> requeue -> searching
```

- `searching`: periodically publish the local ticket and validate incoming candidates.
- `proposing`: members gain proposal eligibility in deterministic order; each accepts only a proposal containing its exact original ticket.
- `confirming`: every device signs independently while the sidecar attempts direct dials using ticket addresses.
- `ready`: after all acceptances arrive, emit the common table ID, one-based seats, player order, and PeerId order.

If the local ticket expires before `ready`, the sidecar preserves stake and buy-in, creates a new nonce and ticket, clears stale proposal state, emits `matchmaking_requeued`, and continues without another user action.

Gossipsub uses strict message signatures. Ticket, proposal, and acceptance message sources must match the corresponding session PeerId. Messages are capped at 128 KiB, the candidate pool at 256 tickets, and ticket lifetime at 120 seconds.

## Consistency limitation

Gossipsub is eventually consistent, not a total-order log. Partitions and races may expose different candidate sets. The implementation locks the first cryptographically valid proposal containing its local ticket and rejects conflicting reuse. This prevents keyless mutation but does not guarantee globally fair queuing or Byzantine consensus.

## Verification

`node scripts/verify-p2p-hand.mjs` starts a real Rendezvous sidecar and two player sidecars that do not know each other's address. The guest deliberately drops all room-topic Gossipsub publications, so admission must use the signed request-response path instead of succeeding by accident through eventual gossip. The test verifies discovery, reliable admission, identical table IDs, distinct seats, and a complete hand.

`node scripts/verify-p2p-hand.mjs --relay` repeats the same fault-injected session with both players reserving a Circuit Relay path. It verifies that matchmaking survives NAT-style relay topology while direct connection upgrades remain optional.

`node scripts/verify-complete-session.mjs` covers private-room membership and complete hands. `node scripts/verify-volunteer-network.mjs` covers Relay v2 reservations, service limits, Rendezvous registration, and stable PeerId recovery.
