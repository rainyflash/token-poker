use crate::{Chips, PlayerId, SignedChips, StakeLevel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const PUBLIC_BETTING_STATE_DOMAIN: &[u8] = b"token-holdem/public-betting-state/v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Card {
    rank: u8,
    suit: Suit,
}

impl Card {
    pub fn new(rank: u8, suit: Suit) -> Result<Self, HoldemError> {
        if !(2..=14).contains(&rank) {
            return Err(HoldemError::InvalidCardRank(rank));
        }
        Ok(Self { rank, suit })
    }

    pub fn from_deck_index(index: u8) -> Result<Self, HoldemError> {
        if index >= 52 {
            return Err(HoldemError::InvalidDeckIndex(index));
        }
        let suit = match index / 13 {
            0 => Suit::Clubs,
            1 => Suit::Diamonds,
            2 => Suit::Hearts,
            _ => Suit::Spades,
        };
        Ok(Self {
            rank: index % 13 + 2,
            suit,
        })
    }

    pub const fn rank(self) -> u8 {
        self.rank
    }

    pub const fn suit(self) -> Suit {
        self.suit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HandCategory {
    HighCard,
    OnePair,
    TwoPair,
    ThreeOfAKind,
    Straight,
    Flush,
    FullHouse,
    FourOfAKind,
    StraightFlush,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HandRank {
    pub category: HandCategory,
    pub kickers: [u8; 5],
}

pub fn evaluate_seven(cards: [Card; 7]) -> HandRank {
    let mut best = HandRank {
        category: HandCategory::HighCard,
        kickers: [0; 5],
    };
    for first in 0..3 {
        for second in (first + 1)..4 {
            for third in (second + 1)..5 {
                for fourth in (third + 1)..6 {
                    for fifth in (fourth + 1)..7 {
                        best = best.max(evaluate_five([
                            cards[first],
                            cards[second],
                            cards[third],
                            cards[fourth],
                            cards[fifth],
                        ]));
                    }
                }
            }
        }
    }
    best
}

fn evaluate_five(cards: [Card; 5]) -> HandRank {
    let flush = cards.iter().all(|card| card.suit == cards[0].suit);
    let mut unique_ranks = cards.iter().map(|card| card.rank).collect::<Vec<_>>();
    unique_ranks.sort_unstable();
    unique_ranks.dedup();
    let straight_high = straight_high(&unique_ranks);
    if let (true, Some(high)) = (flush, straight_high) {
        return rank(HandCategory::StraightFlush, &[high]);
    }

    let mut counts = BTreeMap::<u8, u8>::new();
    for card in cards {
        *counts.entry(card.rank).or_default() += 1;
    }
    let mut groups = counts
        .into_iter()
        .map(|(rank, count)| (count, rank))
        .collect::<Vec<_>>();
    groups.sort_unstable_by(|left, right| right.cmp(left));

    if groups[0].0 == 4 {
        return rank(HandCategory::FourOfAKind, &[groups[0].1, groups[1].1]);
    }
    if groups[0].0 == 3 && groups[1].0 == 2 {
        return rank(HandCategory::FullHouse, &[groups[0].1, groups[1].1]);
    }
    let mut descending = cards.iter().map(|card| card.rank).collect::<Vec<_>>();
    descending.sort_unstable_by(|left, right| right.cmp(left));
    if flush {
        return rank(HandCategory::Flush, &descending);
    }
    if let Some(high) = straight_high {
        return rank(HandCategory::Straight, &[high]);
    }
    if groups[0].0 == 3 {
        let mut kickers = groups
            .iter()
            .filter(|(count, _)| *count == 1)
            .map(|(_, rank)| *rank)
            .collect::<Vec<_>>();
        kickers.sort_unstable_by(|left, right| right.cmp(left));
        return rank(
            HandCategory::ThreeOfAKind,
            &[groups[0].1, kickers[0], kickers[1]],
        );
    }
    let pairs = groups
        .iter()
        .filter(|(count, _)| *count == 2)
        .map(|(_, rank)| *rank)
        .collect::<Vec<_>>();
    if pairs.len() == 2 {
        let high_pair = pairs[0].max(pairs[1]);
        let low_pair = pairs[0].min(pairs[1]);
        let kicker = groups
            .iter()
            .find(|(count, _)| *count == 1)
            .map(|(_, rank)| *rank)
            .unwrap();
        return rank(HandCategory::TwoPair, &[high_pair, low_pair, kicker]);
    }
    if pairs.len() == 1 {
        let mut kickers = groups
            .iter()
            .filter(|(count, _)| *count == 1)
            .map(|(_, rank)| *rank)
            .collect::<Vec<_>>();
        kickers.sort_unstable_by(|left, right| right.cmp(left));
        return rank(
            HandCategory::OnePair,
            &[pairs[0], kickers[0], kickers[1], kickers[2]],
        );
    }
    rank(HandCategory::HighCard, &descending)
}

fn rank(category: HandCategory, values: &[u8]) -> HandRank {
    let mut kickers = [0; 5];
    kickers[..values.len()].copy_from_slice(values);
    HandRank { category, kickers }
}

fn straight_high(sorted_unique: &[u8]) -> Option<u8> {
    if sorted_unique.len() != 5 {
        return None;
    }
    if sorted_unique == [2, 3, 4, 5, 14] {
        return Some(5);
    }
    (sorted_unique[4] - sorted_unique[0] == 4).then_some(sorted_unique[4])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatStatus {
    Active,
    Folded,
    AllIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerAction {
    Fold,
    Check,
    Call,
    RaiseTo(Chips),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatState {
    player_id: PlayerId,
    starting_stack: Chips,
    stack: Chips,
    total_committed: Chips,
    street_committed: Chips,
    status: SeatStatus,
}

impl SeatState {
    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    pub const fn stack(&self) -> Chips {
        self.stack
    }

    pub const fn starting_stack(&self) -> Chips {
        self.starting_stack
    }

    pub const fn total_committed(&self) -> Chips {
        self.total_committed
    }

    pub const fn street_committed(&self) -> Chips {
        self.street_committed
    }

    pub const fn status(&self) -> SeatStatus {
        self.status
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSettlement {
    pub player_id: PlayerId,
    pub starting_stack: Chips,
    pub ending_stack: Chips,
    pub delta: SignedChips,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldemSettlement {
    pub players: Vec<PlayerSettlement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    WaitingFor(PlayerId),
    StreetAdvanced(Street, Option<PlayerId>),
    ShowdownReady,
    HandComplete(HoldemSettlement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicBettingStateHash([u8; 32]);

impl PublicBettingStateHash {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub struct HoldemHand {
    level: StakeLevel,
    dealer_index: usize,
    street: Street,
    seats: Vec<SeatState>,
    pending: BTreeSet<PlayerId>,
    raise_rights: BTreeSet<PlayerId>,
    current_bet: Chips,
    last_full_raise: Chips,
    next_to_act: Option<usize>,
}

impl HoldemHand {
    pub fn start(
        level: StakeLevel,
        seats: Vec<(PlayerId, Chips)>,
        dealer_index: usize,
    ) -> Result<Self, HoldemError> {
        if seats.len() < usize::from(level.minimum_players())
            || seats.len() > usize::from(level.maximum_players())
        {
            return Err(HoldemError::InvalidSeatCount);
        }
        if dealer_index >= seats.len() {
            return Err(HoldemError::InvalidDealerIndex);
        }
        let unique = seats
            .iter()
            .map(|(player, _)| *player)
            .collect::<BTreeSet<_>>();
        if unique.len() != seats.len() {
            return Err(HoldemError::DuplicatePlayer);
        }
        if seats
            .iter()
            .any(|(_, stack)| *stack < level.minimum_buy_in() || *stack > level.maximum_buy_in())
        {
            return Err(HoldemError::StackOutsideBuyInRange);
        }

        let mut hand = Self {
            level: level.clone(),
            dealer_index,
            street: Street::Preflop,
            seats: seats
                .into_iter()
                .map(|(player_id, stack)| SeatState {
                    player_id,
                    starting_stack: stack,
                    stack,
                    total_committed: Chips::ZERO,
                    street_committed: Chips::ZERO,
                    status: SeatStatus::Active,
                })
                .collect(),
            pending: BTreeSet::new(),
            raise_rights: BTreeSet::new(),
            current_bet: level.big_blind(),
            last_full_raise: level.big_blind(),
            next_to_act: None,
        };
        let (small_blind_index, big_blind_index, first_to_act) = hand.blind_positions();
        hand.commit(small_blind_index, level.small_blind())?;
        hand.commit(big_blind_index, level.big_blind())?;
        hand.reset_pending_for_street();
        hand.next_to_act = hand.next_pending_from(first_to_act);
        Ok(hand)
    }

    pub const fn street(&self) -> Street {
        self.street
    }

    pub fn seats(&self) -> &[SeatState] {
        &self.seats
    }

    pub fn next_player(&self) -> Option<PlayerId> {
        self.next_to_act.map(|index| self.seats[index].player_id)
    }

    pub const fn current_bet(&self) -> Chips {
        self.current_bet
    }

    pub fn pot(&self) -> Result<Chips, HoldemError> {
        self.seats.iter().try_fold(Chips::ZERO, |pot, seat| {
            pot.checked_add(seat.total_committed)
                .ok_or(HoldemError::ChipOverflow)
        })
    }

    pub fn amount_to_call(&self, player_id: PlayerId) -> Result<Chips, HoldemError> {
        let seat = self
            .seats
            .iter()
            .find(|seat| seat.player_id == player_id)
            .ok_or(HoldemError::UnknownPlayer)?;
        Ok(Chips::new(
            self.current_bet
                .value()
                .saturating_sub(seat.street_committed.value()),
        ))
    }

    pub fn minimum_raise_to(&self) -> Result<Chips, HoldemError> {
        self.current_bet
            .checked_add(self.last_full_raise)
            .ok_or(HoldemError::ChipOverflow)
    }

    pub fn public_state_hash(&self) -> PublicBettingStateHash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(PUBLIC_BETTING_STATE_DOMAIN);
        append_bytes(&mut hasher, self.level.id().as_bytes());
        hasher.update(&self.level.small_blind().value().to_be_bytes());
        hasher.update(&self.level.big_blind().value().to_be_bytes());
        hasher.update(&[self.dealer_index as u8, street_tag(self.street)]);
        hasher.update(&[self.seats.len() as u8]);
        for seat in &self.seats {
            hasher.update(seat.player_id.as_bytes());
            hasher.update(&seat.starting_stack.value().to_be_bytes());
            hasher.update(&seat.stack.value().to_be_bytes());
            hasher.update(&seat.total_committed.value().to_be_bytes());
            hasher.update(&seat.street_committed.value().to_be_bytes());
            hasher.update(&[seat_status_tag(seat.status)]);
        }
        append_player_set(&mut hasher, &self.pending);
        append_player_set(&mut hasher, &self.raise_rights);
        hasher.update(&self.current_bet.value().to_be_bytes());
        hasher.update(&self.last_full_raise.value().to_be_bytes());
        match self.next_to_act {
            Some(index) => hasher.update(&[1, index as u8]),
            None => hasher.update(&[0, 0]),
        };
        PublicBettingStateHash(*hasher.finalize().as_bytes())
    }

    pub fn act(
        &mut self,
        player_id: PlayerId,
        action: PlayerAction,
    ) -> Result<ActionOutcome, HoldemError> {
        let actor_index = self.next_to_act.ok_or(HoldemError::NoActionExpected)?;
        if self.seats[actor_index].player_id != player_id {
            return Err(HoldemError::OutOfTurn);
        }
        match action {
            PlayerAction::Fold => {
                self.seats[actor_index].status = SeatStatus::Folded;
                self.consume_action(player_id);
            }
            PlayerAction::Check => {
                if self.seats[actor_index].street_committed != self.current_bet {
                    return Err(HoldemError::CannotCheck);
                }
                self.consume_action(player_id);
            }
            PlayerAction::Call => {
                let needed = self
                    .current_bet
                    .value()
                    .saturating_sub(self.seats[actor_index].street_committed.value());
                if needed == 0 {
                    return Err(HoldemError::NothingToCall);
                }
                let paid = Chips::new(needed.min(self.seats[actor_index].stack.value()));
                self.commit(actor_index, paid)?;
                self.consume_action(player_id);
            }
            PlayerAction::RaiseTo(target) => self.raise_to(actor_index, target)?,
        }

        let contenders = self.remaining_contenders();
        if contenders.len() == 1 {
            let settlement = self.finish_uncontested(contenders[0])?;
            return Ok(ActionOutcome::HandComplete(settlement));
        }
        if self.pending.is_empty() {
            return self.advance_street();
        }
        self.next_to_act = self.next_pending_from((actor_index + 1) % self.seats.len());
        Ok(ActionOutcome::WaitingFor(
            self.next_player().ok_or(HoldemError::StateInvariant)?,
        ))
    }

    pub fn settle_showdown(
        &mut self,
        board: [Card; 5],
        hole_cards: BTreeMap<PlayerId, [Card; 2]>,
    ) -> Result<HoldemSettlement, HoldemError> {
        if self.street != Street::Showdown {
            return Err(HoldemError::ShowdownNotReady);
        }
        let contenders = self.remaining_contenders();
        if contenders
            .iter()
            .any(|player| !hole_cards.contains_key(player))
        {
            return Err(HoldemError::MissingHoleCards);
        }
        let mut seen_cards = BTreeSet::new();
        for card in board {
            if !seen_cards.insert(card) {
                return Err(HoldemError::DuplicateCard);
            }
        }
        for player in &contenders {
            for card in hole_cards[player] {
                if !seen_cards.insert(card) {
                    return Err(HoldemError::DuplicateCard);
                }
            }
        }

        let ranks = contenders
            .iter()
            .map(|player| {
                let hole = hole_cards[player];
                (
                    *player,
                    evaluate_seven([
                        hole[0], hole[1], board[0], board[1], board[2], board[3], board[4],
                    ]),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let thresholds = self
            .seats
            .iter()
            .map(|seat| seat.total_committed.value())
            .filter(|value| *value > 0)
            .collect::<BTreeSet<_>>();
        let mut previous = 0_u64;
        for threshold in thresholds {
            let contributors = self
                .seats
                .iter()
                .filter(|seat| seat.total_committed.value() >= threshold)
                .count() as u64;
            let pot = (threshold - previous)
                .checked_mul(contributors)
                .ok_or(HoldemError::ChipOverflow)?;
            previous = threshold;
            let eligible = contenders
                .iter()
                .copied()
                .filter(|player| {
                    self.seat(*player)
                        .is_some_and(|seat| seat.total_committed.value() >= threshold)
                })
                .collect::<Vec<_>>();
            if eligible.is_empty() {
                let sole_contributor = self
                    .seats
                    .iter()
                    .filter(|seat| seat.total_committed.value() >= threshold)
                    .map(|seat| seat.player_id)
                    .collect::<Vec<_>>();
                if sole_contributor.len() != 1 {
                    return Err(HoldemError::StateInvariant);
                }
                self.award(sole_contributor[0], pot)?;
                continue;
            }
            let best = eligible
                .iter()
                .map(|player| ranks[player])
                .max()
                .ok_or(HoldemError::StateInvariant)?;
            let winners = eligible
                .into_iter()
                .filter(|player| ranks[player] == best)
                .collect::<Vec<_>>();
            self.award_split(pot, &winners)?;
        }
        self.street = Street::Complete;
        self.build_settlement()
    }

    fn raise_to(&mut self, actor_index: usize, target: Chips) -> Result<(), HoldemError> {
        let player = self.seats[actor_index].player_id;
        if !self.raise_rights.contains(&player) {
            return Err(HoldemError::RaiseNotReopened);
        }
        if target <= self.current_bet || target <= self.seats[actor_index].street_committed {
            return Err(HoldemError::RaiseMustIncreaseBet);
        }
        let additional = target
            .checked_sub(self.seats[actor_index].street_committed)
            .ok_or(HoldemError::StateInvariant)?;
        if additional > self.seats[actor_index].stack {
            return Err(HoldemError::InsufficientStack);
        }
        let minimum_target = if self.current_bet == Chips::ZERO {
            self.level.big_blind()
        } else {
            self.current_bet
                .checked_add(self.last_full_raise)
                .ok_or(HoldemError::ChipOverflow)?
        };
        let is_all_in = additional == self.seats[actor_index].stack;
        if target < minimum_target && !is_all_in {
            return Err(HoldemError::RaiseBelowMinimum { minimum_target });
        }
        let previous_bet = self.current_bet;
        self.commit(actor_index, additional)?;
        self.current_bet = target;
        self.pending.remove(&player);
        self.raise_rights.remove(&player);

        if target >= minimum_target {
            self.last_full_raise = target
                .checked_sub(previous_bet)
                .ok_or(HoldemError::StateInvariant)?;
            self.pending = self.actionable_players(Some(player));
            self.raise_rights = self.pending.clone();
        } else {
            for seat in &self.seats {
                if seat.status == SeatStatus::Active && seat.street_committed < self.current_bet {
                    self.pending.insert(seat.player_id);
                    if self.current_bet.value() - seat.street_committed.value()
                        >= self.last_full_raise.value()
                    {
                        self.raise_rights.insert(seat.player_id);
                    }
                }
            }
        }
        Ok(())
    }

    fn advance_street(&mut self) -> Result<ActionOutcome, HoldemError> {
        self.street = match self.street {
            Street::Preflop => Street::Flop,
            Street::Flop => Street::Turn,
            Street::Turn => Street::River,
            Street::River => Street::Showdown,
            _ => return Err(HoldemError::StateInvariant),
        };
        if self.street == Street::Showdown {
            self.next_to_act = None;
            return Ok(ActionOutcome::ShowdownReady);
        }
        for seat in &mut self.seats {
            seat.street_committed = Chips::ZERO;
        }
        self.current_bet = Chips::ZERO;
        self.last_full_raise = self.level.big_blind();
        self.reset_pending_for_street();
        self.next_to_act = self.next_pending_from((self.dealer_index + 1) % self.seats.len());
        if self.pending.len() < 2 {
            self.pending.clear();
            return self.advance_street();
        }
        Ok(ActionOutcome::StreetAdvanced(
            self.street,
            self.next_player(),
        ))
    }

    fn finish_uncontested(&mut self, winner: PlayerId) -> Result<HoldemSettlement, HoldemError> {
        let pot = self
            .seats
            .iter()
            .try_fold(0_u64, |total, seat| {
                total.checked_add(seat.total_committed.value())
            })
            .ok_or(HoldemError::ChipOverflow)?;
        self.award(winner, pot)?;
        self.street = Street::Complete;
        self.build_settlement()
    }

    fn award_split(&mut self, pot: u64, winners: &[PlayerId]) -> Result<(), HoldemError> {
        let share = pot / winners.len() as u64;
        let remainder = pot % winners.len() as u64;
        for winner in winners {
            self.award(*winner, share)?;
        }
        let winner_set = winners.iter().copied().collect::<BTreeSet<_>>();
        let ordered = (1..=self.seats.len())
            .map(|offset| (self.dealer_index + offset) % self.seats.len())
            .map(|index| self.seats[index].player_id)
            .filter(|player| winner_set.contains(player))
            .collect::<Vec<_>>();
        for winner in ordered.into_iter().take(remainder as usize) {
            self.award(winner, 1)?;
        }
        Ok(())
    }

    fn award(&mut self, player: PlayerId, amount: u64) -> Result<(), HoldemError> {
        let seat = self.seat_mut(player).ok_or(HoldemError::UnknownPlayer)?;
        seat.stack = seat
            .stack
            .checked_add(Chips::new(amount))
            .ok_or(HoldemError::ChipOverflow)?;
        Ok(())
    }

    fn build_settlement(&self) -> Result<HoldemSettlement, HoldemError> {
        let players = self
            .seats
            .iter()
            .map(|seat| PlayerSettlement {
                player_id: seat.player_id,
                starting_stack: seat.starting_stack,
                ending_stack: seat.stack,
                delta: SignedChips::new(
                    i128::from(seat.stack.value()) - i128::from(seat.starting_stack.value()),
                ),
            })
            .collect::<Vec<_>>();
        let total_delta = players
            .iter()
            .map(|player| player.delta.value())
            .sum::<i128>();
        if total_delta != 0 {
            return Err(HoldemError::StateInvariant);
        }
        Ok(HoldemSettlement { players })
    }

    fn blind_positions(&self) -> (usize, usize, usize) {
        if self.seats.len() == 2 {
            let big = (self.dealer_index + 1) % 2;
            (self.dealer_index, big, self.dealer_index)
        } else {
            let small = (self.dealer_index + 1) % self.seats.len();
            let big = (self.dealer_index + 2) % self.seats.len();
            let first = (self.dealer_index + 3) % self.seats.len();
            (small, big, first)
        }
    }

    fn commit(&mut self, index: usize, requested: Chips) -> Result<(), HoldemError> {
        let amount = Chips::new(requested.value().min(self.seats[index].stack.value()));
        self.seats[index].stack = self.seats[index]
            .stack
            .checked_sub(amount)
            .ok_or(HoldemError::StateInvariant)?;
        self.seats[index].total_committed = self.seats[index]
            .total_committed
            .checked_add(amount)
            .ok_or(HoldemError::ChipOverflow)?;
        self.seats[index].street_committed = self.seats[index]
            .street_committed
            .checked_add(amount)
            .ok_or(HoldemError::ChipOverflow)?;
        if self.seats[index].stack == Chips::ZERO {
            self.seats[index].status = SeatStatus::AllIn;
        }
        Ok(())
    }

    fn reset_pending_for_street(&mut self) {
        self.pending = self.actionable_players(None);
        self.raise_rights = self.pending.clone();
    }

    fn consume_action(&mut self, player: PlayerId) {
        self.pending.remove(&player);
        self.raise_rights.remove(&player);
    }

    fn actionable_players(&self, excluded: Option<PlayerId>) -> BTreeSet<PlayerId> {
        self.seats
            .iter()
            .filter(|seat| seat.status == SeatStatus::Active && Some(seat.player_id) != excluded)
            .map(|seat| seat.player_id)
            .collect()
    }

    fn next_pending_from(&self, start: usize) -> Option<usize> {
        (0..self.seats.len())
            .map(|offset| (start + offset) % self.seats.len())
            .find(|index| self.pending.contains(&self.seats[*index].player_id))
    }

    fn remaining_contenders(&self) -> Vec<PlayerId> {
        self.seats
            .iter()
            .filter(|seat| seat.status != SeatStatus::Folded)
            .map(|seat| seat.player_id)
            .collect()
    }

    fn seat(&self, player: PlayerId) -> Option<&SeatState> {
        self.seats.iter().find(|seat| seat.player_id == player)
    }

    fn seat_mut(&mut self, player: PlayerId) -> Option<&mut SeatState> {
        self.seats.iter_mut().find(|seat| seat.player_id == player)
    }
}

fn append_bytes(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn append_player_set(hasher: &mut blake3::Hasher, players: &BTreeSet<PlayerId>) {
    hasher.update(&[players.len() as u8]);
    for player in players {
        hasher.update(player.as_bytes());
    }
}

const fn street_tag(street: Street) -> u8 {
    match street {
        Street::Preflop => 0,
        Street::Flop => 1,
        Street::Turn => 2,
        Street::River => 3,
        Street::Showdown => 4,
        Street::Complete => 5,
    }
}

const fn seat_status_tag(status: SeatStatus) -> u8 {
    match status {
        SeatStatus::Active => 0,
        SeatStatus::Folded => 1,
        SeatStatus::AllIn => 2,
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HoldemError {
    #[error("牌面点数必须在 2 到 14 之间，实际为 {0}")]
    InvalidCardRank(u8),
    #[error("牌组索引必须在 0 到 51 之间，实际为 {0}")]
    InvalidDeckIndex(u8),
    #[error("座位数不符合牌桌级别")]
    InvalidSeatCount,
    #[error("庄家座位索引无效")]
    InvalidDealerIndex,
    #[error("同一玩家不能占用多个座位")]
    DuplicatePlayer,
    #[error("玩家筹码不在牌桌买入范围内")]
    StackOutsideBuyInRange,
    #[error("当前没有玩家需要行动")]
    NoActionExpected,
    #[error("玩家没有轮到行动")]
    OutOfTurn,
    #[error("面对下注时不能过牌")]
    CannotCheck,
    #[error("当前没有需要跟注的金额")]
    NothingToCall,
    #[error("加注必须提高当前下注额")]
    RaiseMustIncreaseBet,
    #[error("此前的非完整全下加注没有重新开放加注权")]
    RaiseNotReopened,
    #[error("筹码不足以完成该行动")]
    InsufficientStack,
    #[error("加注低于最小值，至少应加到 {minimum_target}")]
    RaiseBelowMinimum { minimum_target: Chips },
    #[error("尚未进入摊牌阶段")]
    ShowdownNotReady,
    #[error("缺少仍在牌局中的玩家底牌")]
    MissingHoleCards,
    #[error("摊牌数据包含重复牌")]
    DuplicateCard,
    #[error("未知玩家")]
    UnknownPlayer,
    #[error("筹码计算溢出")]
    ChipOverflow,
    #[error("牌局状态不变量被破坏")]
    StateInvariant,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(rank: u8, suit: Suit) -> Card {
        Card::new(rank, suit).unwrap()
    }

    fn level() -> StakeLevel {
        StakeLevel::new(
            "1-2",
            Chips::new(1),
            Chips::new(2),
            Chips::new(40),
            Chips::new(200),
            2,
            6,
        )
        .unwrap()
    }

    #[test]
    fn 七张牌评估器正确识别轮盘同花顺() {
        let evaluated = evaluate_seven([
            card(14, Suit::Spades),
            card(2, Suit::Spades),
            card(3, Suit::Spades),
            card(4, Suit::Spades),
            card(5, Suit::Spades),
            card(13, Suit::Hearts),
            card(13, Suit::Clubs),
        ]);
        assert_eq!(evaluated.category, HandCategory::StraightFlush);
        assert_eq!(evaluated.kickers[0], 5);
    }

    #[test]
    fn 单挑弃牌后赢家拿走底池且结算零和() {
        let first = PlayerId::new([1; 32]);
        let second = PlayerId::new([2; 32]);
        let mut hand = HoldemHand::start(
            level(),
            vec![(first, Chips::new(100)), (second, Chips::new(100))],
            0,
        )
        .unwrap();
        let result = hand.act(first, PlayerAction::Fold).unwrap();
        let ActionOutcome::HandComplete(settlement) = result else {
            panic!("应当直接完成牌局")
        };
        assert_eq!(settlement.players[0].ending_stack, Chips::new(99));
        assert_eq!(settlement.players[1].ending_stack, Chips::new(101));
        assert_eq!(
            settlement
                .players
                .iter()
                .map(|player| player.delta.value())
                .sum::<i128>(),
            0
        );
    }

    #[test]
    fn 非全下加注必须满足最小加注额() {
        let first = PlayerId::new([1; 32]);
        let second = PlayerId::new([2; 32]);
        let mut hand = HoldemHand::start(
            level(),
            vec![(first, Chips::new(100)), (second, Chips::new(100))],
            0,
        )
        .unwrap();
        assert_eq!(
            hand.act(first, PlayerAction::RaiseTo(Chips::new(3))),
            Err(HoldemError::RaiseBelowMinimum {
                minimum_target: Chips::new(4)
            })
        );
    }

    #[test]
    fn 连续短额全下累计达到完整加注后重新开放加注权() {
        let players = [1, 2, 3, 4].map(|id| PlayerId::new([id; 32]));
        let mut hand = HoldemHand::start(
            level(),
            players
                .into_iter()
                .zip([200, 90, 120, 200].map(Chips::new))
                .collect(),
            0,
        )
        .unwrap();
        hand.act(players[3], PlayerAction::RaiseTo(Chips::new(60)))
            .unwrap();
        hand.act(players[0], PlayerAction::Call).unwrap();
        hand.act(players[1], PlayerAction::RaiseTo(Chips::new(90)))
            .unwrap();
        hand.act(players[2], PlayerAction::RaiseTo(Chips::new(120)))
            .unwrap();
        assert_eq!(hand.minimum_raise_to().unwrap(), Chips::new(178));
        assert!(hand
            .act(players[3], PlayerAction::RaiseTo(Chips::new(178)))
            .is_ok());
    }

    #[test]
    fn 跟过中间短额全下的玩家必须单独累计面对的加注额() {
        let players = [1, 2, 3, 4, 5].map(|id| PlayerId::new([id; 32]));
        let mut hand = HoldemHand::start(
            level(),
            players
                .into_iter()
                .zip([200, 120, 200, 200, 90].map(Chips::new))
                .collect(),
            0,
        )
        .unwrap();
        hand.act(players[3], PlayerAction::RaiseTo(Chips::new(60)))
            .unwrap();
        hand.act(players[4], PlayerAction::RaiseTo(Chips::new(90)))
            .unwrap();
        hand.act(players[0], PlayerAction::Call).unwrap();
        hand.act(players[1], PlayerAction::RaiseTo(Chips::new(120)))
            .unwrap();
        hand.act(players[2], PlayerAction::Call).unwrap();
        hand.act(players[3], PlayerAction::Call).unwrap();
        assert_eq!(
            hand.act(players[0], PlayerAction::RaiseTo(Chips::new(178))),
            Err(HoldemError::RaiseNotReopened)
        );
        assert!(hand.act(players[0], PlayerAction::Call).is_ok());
    }

    #[test]
    fn 公共下注摘要对相同状态稳定且随合法动作变化() {
        let first = PlayerId::new([1; 32]);
        let second = PlayerId::new([2; 32]);
        let seats = vec![(first, Chips::new(100)), (second, Chips::new(100))];
        let mut first_view = HoldemHand::start(level(), seats.clone(), 0).unwrap();
        let second_view = HoldemHand::start(level(), seats, 0).unwrap();

        assert_eq!(
            first_view.public_state_hash(),
            second_view.public_state_hash()
        );
        let before = first_view.public_state_hash();
        first_view.act(first, PlayerAction::Call).unwrap();
        assert_ne!(first_view.public_state_hash(), before);
    }

    #[test]
    fn 三人全下能够按主池和边池分别结算() {
        let short = PlayerId::new([1; 32]);
        let middle = PlayerId::new([2; 32]);
        let deep = PlayerId::new([3; 32]);
        let mut hand = HoldemHand::start(
            level(),
            vec![
                (short, Chips::new(40)),
                (middle, Chips::new(100)),
                (deep, Chips::new(200)),
            ],
            0,
        )
        .unwrap();

        assert_eq!(hand.next_player(), Some(short));
        hand.act(short, PlayerAction::RaiseTo(Chips::new(40)))
            .unwrap();
        hand.act(middle, PlayerAction::Call).unwrap();
        hand.act(deep, PlayerAction::RaiseTo(Chips::new(100)))
            .unwrap();
        assert_eq!(
            hand.act(middle, PlayerAction::Call).unwrap(),
            ActionOutcome::ShowdownReady
        );

        let board = [
            card(2, Suit::Clubs),
            card(3, Suit::Diamonds),
            card(4, Suit::Hearts),
            card(9, Suit::Spades),
            card(13, Suit::Diamonds),
        ];
        let settlement = hand
            .settle_showdown(
                board,
                BTreeMap::from([
                    (short, [card(14, Suit::Spades), card(5, Suit::Spades)]),
                    (middle, [card(13, Suit::Hearts), card(12, Suit::Hearts)]),
                    (deep, [card(12, Suit::Clubs), card(11, Suit::Clubs)]),
                ]),
            )
            .unwrap();

        assert_eq!(settlement.players[0].ending_stack, Chips::new(120));
        assert_eq!(settlement.players[1].ending_stack, Chips::new(120));
        assert_eq!(settlement.players[2].ending_stack, Chips::new(100));
    }
}
