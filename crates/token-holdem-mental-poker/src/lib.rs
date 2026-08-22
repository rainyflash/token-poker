#![forbid(unsafe_code)]

//! Network adapter layer for complete Mental Poker primitives.
//!
//! This module uses Bayer-Groth verifiable shuffles, key-ownership proofs, and
//! verifiable decryption shares. The underlying `ziffle` crate remains an
//! unaudited experimental library, so this module is not production cryptography.

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use token_holdem_domain::Card;
use ziffle::{
    AggregatePublicKey, AggregateRevealToken, MaskedDeck, OwnershipProof, PublicKey, RevealToken,
    RevealTokenProof, SecretKey, Shuffle, ShuffleProof, Verified,
};

pub const STANDARD_DECK_SIZE: usize = 52;
const TRANSCRIPT_DOMAIN: &[u8] = b"token-holdem/mental-poker-transcript/v1\0";

type StandardDeck = MaskedDeck<STANDARD_DECK_SIZE>;
type StandardShuffleProof = ShuffleProof<STANDARD_DECK_SIZE>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyAnnouncement {
    pub public_key: Vec<u8>,
    pub ownership_proof: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShufflePacket {
    pub deck: Vec<u8>,
    pub proof: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealSharePacket {
    pub token: Vec<u8>,
    pub proof: Vec<u8>,
}

pub struct PlayerKeyMaterial {
    secret_key: SecretKey,
    public_key: PublicKey,
    announcement: KeyAnnouncement,
}

impl PlayerKeyMaterial {
    pub fn generate(
        engine: &MentalPokerEngine,
        rng: &mut (impl CryptoRng + RngCore),
    ) -> Result<Self, MentalPokerError> {
        let (secret_key, public_key, proof) = engine.shuffle.keygen(rng, &engine.context);
        let announcement = KeyAnnouncement {
            public_key: serialize(&public_key)?,
            ownership_proof: serialize(&proof)?,
        };
        Ok(Self {
            secret_key,
            public_key,
            announcement,
        })
    }

    pub fn announcement(&self) -> &KeyAnnouncement {
        &self.announcement
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VerifiedParticipantKey(Verified<PublicKey>);

pub struct VerifiedDeck {
    deck: Verified<StandardDeck>,
    encoded: Vec<u8>,
}

impl VerifiedDeck {
    pub fn encoded(&self) -> Result<Vec<u8>, MentalPokerError> {
        Ok(self.encoded.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VerifiedRevealShare(Verified<RevealToken>);

pub struct MentalPokerEngine {
    shuffle: Shuffle<STANDARD_DECK_SIZE>,
    context: Vec<u8>,
}

impl MentalPokerEngine {
    pub fn new(context: impl Into<Vec<u8>>) -> Result<Self, MentalPokerError> {
        let context = context.into();
        if context.len() < 16 {
            return Err(MentalPokerError::ContextTooShort);
        }
        Ok(Self {
            shuffle: Shuffle::default(),
            context,
        })
    }

    pub fn verify_key_announcement(
        &self,
        announcement: &KeyAnnouncement,
    ) -> Result<VerifiedParticipantKey, MentalPokerError> {
        let public_key: PublicKey = deserialize(&announcement.public_key)?;
        let proof: OwnershipProof = deserialize(&announcement.ownership_proof)?;
        let verified = proof
            .verify(public_key, &self.context)
            .ok_or(MentalPokerError::InvalidOwnershipProof)?;
        Ok(VerifiedParticipantKey(verified))
    }

    pub fn aggregate_key(
        &self,
        participants: &[VerifiedParticipantKey],
    ) -> Result<AggregateKey, MentalPokerError> {
        if participants.len() < 2 {
            return Err(MentalPokerError::NotEnoughParticipants);
        }
        let keys: Vec<_> = participants
            .iter()
            .map(|participant| participant.0)
            .collect();
        Ok(AggregateKey(AggregatePublicKey::new(&keys)))
    }

    pub fn initial_shuffle(
        &self,
        rng: &mut (impl CryptoRng + RngCore),
        aggregate_key: AggregateKey,
    ) -> Result<ShufflePacket, MentalPokerError> {
        let (deck, proof) = self
            .shuffle
            .shuffle_initial_deck(rng, aggregate_key.0, &self.context);
        Ok(ShufflePacket {
            deck: serialize(&deck)?,
            proof: serialize(&proof)?,
        })
    }

    pub fn verify_initial_shuffle(
        &self,
        aggregate_key: AggregateKey,
        packet: &ShufflePacket,
    ) -> Result<VerifiedDeck, MentalPokerError> {
        let deck: StandardDeck = deserialize(&packet.deck)?;
        let proof: StandardShuffleProof = deserialize(&packet.proof)?;
        let verified = self
            .shuffle
            .verify_initial_shuffle(aggregate_key.0, deck, proof, &self.context)
            .ok_or(MentalPokerError::InvalidShuffleProof)?;
        Ok(VerifiedDeck {
            deck: verified,
            encoded: packet.deck.clone(),
        })
    }

    pub fn reshuffle(
        &self,
        rng: &mut (impl CryptoRng + RngCore),
        aggregate_key: AggregateKey,
        previous: &VerifiedDeck,
    ) -> Result<ShufflePacket, MentalPokerError> {
        let (deck, proof) =
            self.shuffle
                .shuffle_deck(rng, aggregate_key.0, &previous.deck, &self.context);
        Ok(ShufflePacket {
            deck: serialize(&deck)?,
            proof: serialize(&proof)?,
        })
    }

    pub fn verify_reshuffle(
        &self,
        aggregate_key: AggregateKey,
        previous: &VerifiedDeck,
        packet: &ShufflePacket,
    ) -> Result<VerifiedDeck, MentalPokerError> {
        let deck: StandardDeck = deserialize(&packet.deck)?;
        let proof: StandardShuffleProof = deserialize(&packet.proof)?;
        let verified = self
            .shuffle
            .verify_shuffle(aggregate_key.0, &previous.deck, deck, proof, &self.context)
            .ok_or(MentalPokerError::InvalidShuffleProof)?;
        Ok(VerifiedDeck {
            deck: verified,
            encoded: packet.deck.clone(),
        })
    }

    pub fn create_reveal_share(
        &self,
        rng: &mut (impl CryptoRng + RngCore),
        deck: &VerifiedDeck,
        card_index: usize,
        player: &PlayerKeyMaterial,
    ) -> Result<RevealSharePacket, MentalPokerError> {
        let card = deck
            .deck
            .get(card_index)
            .ok_or(MentalPokerError::CardIndexOutOfRange(card_index))?;
        let (token, proof) =
            card.reveal_token(rng, &player.secret_key, player.public_key, &self.context);
        Ok(RevealSharePacket {
            token: serialize(&token)?,
            proof: serialize(&proof)?,
        })
    }

    pub fn verify_reveal_share(
        &self,
        deck: &VerifiedDeck,
        card_index: usize,
        participant_key: VerifiedParticipantKey,
        packet: &RevealSharePacket,
    ) -> Result<VerifiedRevealShare, MentalPokerError> {
        let card = deck
            .deck
            .get(card_index)
            .ok_or(MentalPokerError::CardIndexOutOfRange(card_index))?;
        let token: RevealToken = deserialize(&packet.token)?;
        let proof: RevealTokenProof = deserialize(&packet.proof)?;
        let verified = proof
            .verify(participant_key.0, token, card, &self.context)
            .ok_or(MentalPokerError::InvalidRevealProof)?;
        Ok(VerifiedRevealShare(verified))
    }

    pub fn reveal_card(
        &self,
        deck: &VerifiedDeck,
        card_index: usize,
        shares: &[VerifiedRevealShare],
        participant_count: usize,
    ) -> Result<usize, MentalPokerError> {
        if shares.len() != participant_count {
            return Err(MentalPokerError::MissingRevealShares {
                expected: participant_count,
                actual: shares.len(),
            });
        }
        let card = deck
            .deck
            .get(card_index)
            .ok_or(MentalPokerError::CardIndexOutOfRange(card_index))?;
        let tokens: Vec<_> = shares.iter().map(|share| share.0).collect();
        self.shuffle
            .reveal_card(AggregateRevealToken::new(&tokens), card)
            .ok_or(MentalPokerError::UnableToRevealCard)
    }

    pub fn reveal_holdem_card(
        &self,
        deck: &VerifiedDeck,
        card_index: usize,
        shares: &[VerifiedRevealShare],
        participant_count: usize,
    ) -> Result<Card, MentalPokerError> {
        let revealed = self.reveal_card(deck, card_index, shares, participant_count)?;
        let deck_index =
            u8::try_from(revealed).map_err(|_| MentalPokerError::InvalidRevealedCard(revealed))?;
        Card::from_deck_index(deck_index)
            .map_err(|_| MentalPokerError::InvalidRevealedCard(revealed))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AggregateKey(AggregatePublicKey);

#[derive(Debug, Clone)]
pub struct ProtocolTranscript {
    hasher: blake3::Hasher,
}

impl Default for ProtocolTranscript {
    fn default() -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(TRANSCRIPT_DOMAIN);
        Self { hasher }
    }
}

impl ProtocolTranscript {
    pub fn append_key(&mut self, announcement: &KeyAnnouncement) {
        self.append_parts(
            b"key",
            [
                &announcement.public_key[..],
                &announcement.ownership_proof[..],
            ],
        );
    }

    pub fn append_shuffle(&mut self, packet: &ShufflePacket) {
        self.append_parts(b"shuffle", [&packet.deck[..], &packet.proof[..]]);
    }

    pub fn append_reveal_share(&mut self, packet: &RevealSharePacket) {
        self.append_parts(b"reveal", [&packet.token[..], &packet.proof[..]]);
    }

    pub fn hash(&self) -> [u8; 32] {
        *self.hasher.finalize().as_bytes()
    }

    fn append_parts<'a>(&mut self, event_type: &[u8], parts: impl IntoIterator<Item = &'a [u8]>) {
        self.hasher.update(&(event_type.len() as u32).to_be_bytes());
        self.hasher.update(event_type);
        for part in parts {
            self.hasher.update(&(part.len() as u32).to_be_bytes());
            self.hasher.update(part);
        }
    }
}

fn serialize<T: CanonicalSerialize>(value: &T) -> Result<Vec<u8>, MentalPokerError> {
    let mut bytes = Vec::with_capacity(value.compressed_size());
    value
        .serialize_compressed(&mut bytes)
        .map_err(|error| MentalPokerError::Serialization(error.to_string()))?;
    Ok(bytes)
}

fn deserialize<T: CanonicalDeserialize>(bytes: &[u8]) -> Result<T, MentalPokerError> {
    let mut remaining = bytes;
    let value = T::deserialize_compressed(&mut remaining)
        .map_err(|error| MentalPokerError::Serialization(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(MentalPokerError::TrailingBytes(remaining.len()));
    }
    Ok(value)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MentalPokerError {
    #[error("协议上下文至少需要 16 个字节")]
    ContextTooShort,
    #[error("至少需要两名参与者")]
    NotEnoughParticipants,
    #[error("密码学消息序列化失败：{0}")]
    Serialization(String),
    #[error("密码学消息包含 {0} 个多余字节")]
    TrailingBytes(usize),
    #[error("密钥所有权证明无效")]
    InvalidOwnershipProof,
    #[error("可验证洗牌证明无效")]
    InvalidShuffleProof,
    #[error("解密份额证明无效")]
    InvalidRevealProof,
    #[error("牌索引 {0} 超出范围")]
    CardIndexOutOfRange(usize),
    #[error("解密份额不足：需要 {expected}，实际 {actual}")]
    MissingRevealShares { expected: usize, actual: usize },
    #[error("无法解密该牌，份额可能不匹配")]
    UnableToRevealCard,
    #[error("解密结果 {0} 不是标准 52 张牌中的索引")]
    InvalidRevealedCard(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn 三名玩家可以完成密钥证明可验证洗牌和可验证开牌() {
        let engine = MentalPokerEngine::new(b"match/2026-08-18/table-7/hand-1".to_vec())
            .expect("协议上下文应当有效");
        let mut rng = OsRng;
        let players = [
            PlayerKeyMaterial::generate(&engine, &mut rng).expect("玩家 A 密钥生成失败"),
            PlayerKeyMaterial::generate(&engine, &mut rng).expect("玩家 B 密钥生成失败"),
            PlayerKeyMaterial::generate(&engine, &mut rng).expect("玩家 C 密钥生成失败"),
        ];
        let verified_keys: Vec<_> = players
            .iter()
            .map(|player| {
                engine
                    .verify_key_announcement(player.announcement())
                    .expect("密钥所有权证明应当有效")
            })
            .collect();
        let aggregate = engine
            .aggregate_key(&verified_keys)
            .expect("聚合密钥应当成功");

        let first_packet = engine
            .initial_shuffle(&mut rng, aggregate)
            .expect("首次洗牌应当成功");
        let first_deck = engine
            .verify_initial_shuffle(aggregate, &first_packet)
            .expect("首次洗牌证明应当有效");
        let second_packet = engine
            .reshuffle(&mut rng, aggregate, &first_deck)
            .expect("第二次洗牌应当成功");
        let second_deck = engine
            .verify_reshuffle(aggregate, &first_deck, &second_packet)
            .expect("第二次洗牌证明应当有效");
        let third_packet = engine
            .reshuffle(&mut rng, aggregate, &second_deck)
            .expect("第三次洗牌应当成功");
        assert!(
            [&first_packet, &second_packet, &third_packet,]
                .into_iter()
                .all(|packet| packet.deck.len() + packet.proof.len() <= 16 * 1_024),
            "单次洗牌包必须保持在实时 P2P 消息预算内"
        );
        let final_deck = engine
            .verify_reshuffle(aggregate, &second_deck, &third_packet)
            .expect("第三次洗牌证明应当有效");

        let share_packets: Vec<_> = players
            .iter()
            .map(|player| {
                engine
                    .create_reveal_share(&mut rng, &final_deck, 0, player)
                    .expect("创建解密份额应当成功")
            })
            .collect();
        let shares: Vec<_> = share_packets
            .iter()
            .zip(verified_keys.iter().copied())
            .map(|(packet, key)| {
                engine
                    .verify_reveal_share(&final_deck, 0, key, packet)
                    .expect("解密份额证明应当有效")
            })
            .collect();
        let card = engine
            .reveal_card(&final_deck, 0, &shares, players.len())
            .expect("聚合份额应当可以开牌");

        assert!(card < STANDARD_DECK_SIZE);
        assert!(engine
            .reveal_holdem_card(&final_deck, 0, &shares, players.len())
            .is_ok());
        assert!(final_deck.encoded().expect("牌组应当可编码").len() > 3_000);
    }
}
