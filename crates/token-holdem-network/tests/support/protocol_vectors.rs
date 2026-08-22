use libp2p::{identity::Keypair, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use token_holdem_domain::{
    Chips, HandOutcome, HandReceipt, MatchId, PlayerAction, StakeLevel, TableId, TranscriptHash,
};
use token_holdem_identity::{
    CoSignedReceipt, DeviceCertificate, DeviceIdentity, ParticipantSignature, RootIdentity,
};
use token_holdem_network::{encode_payload, JoinIntent, PoolTicket, SignedHandAction};

const BASE_TIME_MS: u64 = 1_700_000_000_000;
const CERTIFICATE_LIFETIME_MS: u64 = 365 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVectorFile {
    pub protocol_version: u8,
    pub format: String,
    pub vectors: Vec<ProtocolVector>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVector {
    pub name: String,
    pub object_version: u8,
    pub cbor_hex: String,
    pub canonical_hex: String,
    pub digest_hex: String,
}

struct IdentityFixture {
    root: RootIdentity,
    device: DeviceIdentity,
    certificate: DeviceCertificate,
}

pub fn build_protocol_vectors() -> ProtocolVectorFile {
    let first = identity_fixture(1, 11, "Windows 主设备 A");
    let second = identity_fixture(2, 12, "Windows 主设备 B");
    let level = stake_level();
    let ticket_a = pool_ticket(&first, &level, 21, 41_001, [31; 16]);
    let ticket_b = pool_ticket(&second, &level, 22, 41_002, [32; 16]);
    ticket_a
        .verify_at(BASE_TIME_MS + 2_000)
        .expect("固定票据 A 必须能通过验签");
    ticket_b
        .verify_at(BASE_TIME_MS + 2_000)
        .expect("固定票据 B 必须能通过验签");
    let join_intent = JoinIntent::issue(
        TableId::new([61; 32]),
        ticket_a.clone(),
        BASE_TIME_MS + 2_000,
        BASE_TIME_MS + 61_000,
        [33; 16],
        &first.device,
        first.certificate.clone(),
    )
    .expect("固定入桌意图必须有效");
    join_intent
        .verify_at(BASE_TIME_MS + 2_000)
        .expect("固定入桌意图必须能通过验签");
    let action = SignedHandAction::issue(
        [41; 32],
        7,
        [40; 32],
        3,
        [42; 32],
        1,
        PlayerAction::RaiseTo(Chips::new(1_240_000)),
        BASE_TIME_MS + 3_000,
        &first.device,
        first.certificate.clone(),
    )
    .expect("固定牌桌动作必须有效");
    action
        .verify_for(
            &[41; 32],
            7,
            &[40; 32],
            3,
            &[42; 32],
            1,
            first.root.player_id(),
            first.device.public_key(),
            BASE_TIME_MS + 4_000,
        )
        .expect("固定牌桌动作必须能通过验签");
    let receipt = co_signed_receipt(&first, &second);

    ProtocolVectorFile {
        protocol_version: 9,
        format: "CBOR wire bytes + canonical signing bytes + BLAKE3 digest".to_owned(),
        vectors: vec![
            vector(
                "pool-ticket-v1",
                1,
                &ticket_a,
                ticket_a.canonical_unsigned_bytes(),
            ),
            vector(
                "join-intent-v1",
                1,
                &join_intent,
                join_intent.canonical_unsigned_bytes(),
            ),
            vector(
                "signed-hand-action-v2",
                2,
                &action,
                action.canonical_unsigned_bytes(),
            ),
            vector(
                "co-signed-hand-receipt-v1",
                1,
                &receipt,
                receipt.receipt.canonical_bytes(),
            ),
        ],
    }
}

fn identity_fixture(root_seed: u8, device_seed: u8, label: &str) -> IdentityFixture {
    let root = RootIdentity::from_seed([root_seed; 32]);
    let device = DeviceIdentity::from_seed([device_seed; 32]);
    let certificate = root
        .issue_device_certificate(
            device.public_key(),
            label,
            BASE_TIME_MS,
            BASE_TIME_MS + CERTIFICATE_LIFETIME_MS,
        )
        .expect("固定设备证书必须有效");
    IdentityFixture {
        root,
        device,
        certificate,
    }
}

fn stake_level() -> StakeLevel {
    StakeLevel::new(
        "10k-20k",
        Chips::new(10_000),
        Chips::new(20_000),
        Chips::new(800_000),
        Chips::new(2_000_000),
        2,
        6,
    )
    .expect("固定牌桌级别必须有效")
}

fn pool_ticket(
    identity: &IdentityFixture,
    level: &StakeLevel,
    peer_seed: u8,
    port: u16,
    nonce: [u8; 16],
) -> PoolTicket {
    let peer = deterministic_peer(peer_seed);
    let address = format!("/ip4/127.0.0.1/tcp/{port}/p2p/{peer}")
        .parse::<Multiaddr>()
        .expect("固定会话地址必须有效");
    PoolTicket::issue(
        peer.to_bytes(),
        vec![address.to_vec()],
        level.clone(),
        Chips::new(1_000_000),
        BASE_TIME_MS + 1_000,
        BASE_TIME_MS + 61_000,
        nonce,
        &identity.device,
        identity.certificate.clone(),
    )
    .expect("固定公开池票据必须有效")
}

fn deterministic_peer(seed: u8) -> PeerId {
    Keypair::ed25519_from_bytes([seed; 32])
        .expect("固定 libp2p 密钥必须有效")
        .public()
        .to_peer_id()
}

fn co_signed_receipt(first: &IdentityFixture, second: &IdentityFixture) -> CoSignedReceipt {
    let receipt = HandReceipt::settle(
        MatchId::new([51; 16]),
        7,
        "10k-20k",
        TranscriptHash::new([52; 32]),
        BASE_TIME_MS + 4_000,
        vec![
            HandOutcome {
                player_id: first.root.player_id(),
                device_public_key: first.device.public_key(),
                starting_stack: Chips::new(1_000_000),
                ending_stack: Chips::new(1_180_000),
            },
            HandOutcome {
                player_id: second.root.player_id(),
                device_public_key: second.device.public_key(),
                starting_stack: Chips::new(1_000_000),
                ending_stack: Chips::new(820_000),
            },
        ],
    )
    .expect("固定结算凭证必须有效");
    let signatures = vec![
        ParticipantSignature::create(
            &receipt,
            &first.device,
            first.certificate.clone(),
            BASE_TIME_MS + 4_000,
        )
        .expect("参与方 A 必须能签名"),
        ParticipantSignature::create(
            &receipt,
            &second.device,
            second.certificate.clone(),
            BASE_TIME_MS + 4_000,
        )
        .expect("参与方 B 必须能签名"),
    ];
    let signed = CoSignedReceipt {
        receipt,
        signatures,
    };
    signed
        .verify(BASE_TIME_MS + 4_000)
        .expect("固定共签凭证必须有效");
    signed
}

fn vector<T: Serialize>(
    name: &str,
    object_version: u8,
    value: &T,
    canonical: Vec<u8>,
) -> ProtocolVector {
    let digest = blake3::hash(&canonical);
    ProtocolVector {
        name: name.to_owned(),
        object_version,
        cbor_hex: hex::encode(encode_payload(value).expect("固定对象必须能编码为 CBOR")),
        canonical_hex: hex::encode(canonical),
        digest_hex: digest.to_hex().to_string(),
    }
}
