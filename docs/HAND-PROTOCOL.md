# Continuous Table Protocol

## Security boundary

After matchmaking, the same peer group plays multiple hands. The fully accepted membership proposal fixes the table ID, player order, device certificates, PeerIds, and buy-ins. The hand protocol cannot silently replace a seat, device, or stack.

A dedicated Gossipsub topic carries public protocol messages. libp2p request-response over Noise carries private hole-card decryption shares. Every participant can verify public messages; only a card owner receives enough private shares to decrypt that card.

## Mental Poker sequence

1. Every seat generates an ephemeral hand key and broadcasts a proof of key ownership.
2. Peers verify and aggregate keys in canonical seat order.
3. Seat one creates an encrypted shuffle; each later seat re-shuffles it. Every Bayer-Groth proof must verify.
4. Every peer creates verifiable decryption shares for each hole card and sends remote shares only to that card's owner.
5. A seat broadcasts readiness only after decrypting its own cards. Betting cannot begin before every seat reaches the barrier.
6. Community-card shares are published only when the betting street advances. Remaining hole-card shares are revealed only at showdown.
7. Keys, shuffles, public shares, and signed actions enter the transcript in canonical order. The betting state and public Mental Poker state also produce a hole-card-free public pre-state digest.

Burn positions are preserved. At a six-player table, hole cards occupy indices `0..11`, the flop `13..15`, the turn `17`, and the river `19`. Private hole-card shares never enter the public transcript.

## Actions and conflicts

Each action binds the table ID, hand number, expected sequence, public pre-state digest, seat, player, chip amount, and signing time. The root-authorized device key signs the action. There is no permanent action sequencer:

- the current actor validates turn ownership and betting legality, then broadcasts `ActionCommitted`;
- every peer independently verifies the certificate, signature, table, hand, sequence, actor, and pre-state digest;
- an identical retransmission is idempotent;
- two distinct valid actions signed by the correct device for the same sequence and pre-state freeze the hand and produce conflict evidence; and
- gaps, stale digests, wrong actors, and illegal bets are rejected before reaching the domain state machine.

No host relay is required for an opponent's action. A hand stops only when the current actor disappears, a required share is withheld, or valid equivocation is observed.

## Safe leave and abandonment

A leave request is not a local navigation shortcut. The device signs a `LeaveIntent` bound to the table, membership version, and current hand. The sidecar distributes that intent through direct consensus delivery and bounded Gossipsub retries before acknowledging the command to the UI.

If the departing player remains connected, the current hand completes normally. The room closes locally only after a fully accepted membership certificate excludes that player. This prevents one client from disappearing while survivors retain a stale occupied seat.

If a player signs the intent and then remains explicitly disconnected for ten seconds during an active hand, every survivor derives the same abandonment evidence from the table ID, hand number, roster hash, and leave-intent ID. The hand is aborted without a settlement receipt or statistics update. The evidence becomes the previous-hand barrier input, membership removes the departed player, and the remaining seats may start the next hand.

Local cleanup is bounded: a stalled hand may close after ten seconds, membership convergence after fifteen seconds, and every safe-leave attempt after an absolute two-minute deadline. These deadlines only release local resources; they do not fabricate a settlement or a peer signature.

## Settlement and the next hand

Every peer deterministically constructs the same `HandReceipt` from the table ID, hand number, stake, combined transcript digest, settlement time, and zero-sum result. A device signs only the exact receipt that includes its own player ID, public key, and valid certificate.

After all signatures produce one `CoSignedReceipt`, each seat broadcasts `NextHandReady`. Only after the full barrier and with no disconnected member does the table:

1. increment `hand_number`;
2. rotate the dealer;
3. generate new Mental Poker keys; and
4. reset in-table chips to each device-signed buy-in.

Official lifetime Token is never permanently debited. Aggregate wins and losses are recomputed from immutable co-signed receipts.

## Verification

```powershell
cargo build -p token-holdem-sidecar
node scripts/verify-p2p-hand.mjs
```

The test drives one Rendezvous service and two players through discovery, matchmaking, key proofs, sequential shuffles, private dealing, eight signed call/check actions, five public-card reveals, and showdown. It asserts identical table IDs, hand numbers, boards, transcript digests, action sequences, and zero-sum results.

`node scripts/verify-complete-session.mjs` additionally runs three private-room hands, dealer rotation, stack resets, co-signing, archive acknowledgements, archive restart, and identity/statistics recovery on a new device.

## Known limitations

- An unsigned disconnect still pauses the hand. Local clocks are not sufficient evidence for an automatic fold or settlement; deterministic abandonment requires a previously verified leave intent.
- Valid equivocation freezes the hand; automatic arbitration and penalties are not implemented.
- Connections may recover, but an unsettled hand does not survive a complete sidecar process crash.
- The underlying `ziffle` library has not received a production third-party audit.
