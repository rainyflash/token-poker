# Community Node Operations

## Why at least one node must stay online

A newly installed client has no successful-node cache and needs a long-running community node for cold start. The node does not decide matches or run game logic. It provides libp2p Rendezvous, Kademlia, Circuit Relay v2, and optional encrypted archive storage. Publicly reachable player clients may later volunteer as additional capacity.

The repository does not deploy a VPS automatically. The release directory in `config/community-nodes.json` contains the current public bootstrap addresses. Releasing a changed stable PeerId or address requires a new plugin release.

## Minimum VPS requirements

- Linux x86-64 with Docker Engine and Docker Compose v2.
- A public IPv4 address or publicly resolvable DNS name.
- Inbound `4001/tcp` and `4001/udp` in both the host firewall and cloud security group.
- A persistent Docker volume. Deleting the volume changes the PeerId and invalidates addresses embedded in existing plugin releases.

Start with 1 vCPU and 1 GiB RAM. Default hard limits are 64 reservations, 16 concurrent circuits, one reservation and two circuits per peer, two hours per circuit, and 64 MiB transferred per circuit. These are safety ceilings, not bandwidth guarantees.

## Start the node

From the repository root on the VPS:

```bash
cp deploy/community-node.env.example deploy/community-node.env
# Edit both addresses in deploy/community-node.env to use this VPS's public IP or DNS name.
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml up -d --build
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml ps
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml logs --no-log-prefix community-node
```

The explicit external addresses are mandatory when Docker bridge networking is
used. Container-local `172.x` addresses are not usable relay advertisements;
declaring a node public without advertising its port-forwarded endpoint leaves
Circuit Relay reservations unusable.

The first `ready` JSON event contains the stable `peer_id`. Also verify:

- `volunteer_status.role = active_discovery_relay`;
- one TCP and one QUIC `listen_address`; and
- `archive_node_ready`.

## Update the bundled directory

Run the generator on the release machine instead of manually editing Multiaddrs:

```powershell
node scripts/set-community-node.mjs `
  --host poker.example.com `
  --peer-id 12D3KooWReplaceWithTheCompletePeerId
```

`--host` also accepts a public IPv4 address. The script validates the host and PeerId, generates TCP and QUIC addresses, and atomically updates `config/community-nodes.json`.

Then run:

```powershell
node --test scripts/community-network.test.mjs
node scripts/verify-community-node.mjs
python ./scripts/build-plugin.py
```

`verify-community-node.mjs` tests public TCP and QUIC dialing, a relay reservation, and Rendezvous registration from the release machine. Container health and local VPS logs are not substitutes for this external check.

Explicit command-line nodes take precedence over the bundled directory; the bundled directory takes precedence over the bounded 30-day successful-node cache.

## Operations

Inspect status and recent reservations:

```bash
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml ps
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml logs --since 30m community-node
```

Preserve the volume while rebuilding:

```bash
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml pull --ignore-buildable
docker compose --env-file deploy/community-node.env -f deploy/docker-compose.community-node.yml up -d --build
```

Back up the `community-node-data` volume. It contains the stable libp2p identity, archive signing key, and encrypted recovery and receipt replicas. A leaked backup exposes node keys and opaque user ciphertext. A lost volume changes the PeerId and destroys archive availability.

## Honest limits

- One VPS remains a cold-start dependency. Remove that single point by shipping nodes from multiple independent operators.
- Relays observe endpoint IPs, timing, and traffic volume. Noise protects application content, not metadata.
- Player clients become active relays only after explicit consent, AC power, an unmetered network, and proven public reachability.
- NAT, CGNAT, and firewalls mean many players will remain clients forever. A loaded relay behavior is not proof of public reachability.
