use std::cmp::{Ordering, max};
use rand::{seq::SliceRandom, thread_rng};

use crate::{cards::{Card, HandRank, ShowdownDecidingFactor, compare_hand_ranks, get_best_hand_rank}, events::{GameEvent, GamePlayerAction, ShowdownInfo}};

#[derive(Debug, Clone)]
pub struct Pot {
    pub money: u32,
    pub eligible_players: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct Player {
    pub id: u8,
    pub money: u32,
    total_contribution: u32,
    pub private_cards: [Card; 2],
    pub has_folded: bool,
}

pub struct Game {
    pub players: Vec<Player>,
    pub current_bet: u32,
    current_phase: u8, // 0 - 4, preflop, flop, turn, river, showdown
    pub current_turn: u8,
    last_bettor: u8,
    public_cards: [Card; 5],
}

#[derive(Debug, Clone)]
pub struct ShowdownStep {
    pub winners: Vec<u8>,
    pub winnings: u32,
    pub pot_start_index: u8, // players can win multiple pots next to each other at once, both of those are inclusive
    pub pot_end_index: u8,
    pub eligible_players: Vec<u8>,
    pub win_reason: Option<(ShowdownDecidingFactor, u8)>, // only used if there's one winner
}

impl Game {
    pub fn advance_game(&mut self, action: GamePlayerAction) -> Option<Vec<GameEvent>> { // none means illegal action
        if self.current_phase == 4 { return None }
        let player = self.players.get_mut(self.current_turn as usize).unwrap();
        let mut events = Vec::<GameEvent>::new();
        match action {
            GamePlayerAction::AddMoney(money) => {
                if money == 0 {
                    return None
                }
                if player.total_contribution + money < self.current_bet && money != player.money { // all-ins are only recognized if the bet money is exactly equal to the player's money
                    return None
                }
                if money > player.money {
                    return None
                }
                
                self.current_bet = max(self.current_bet, player.total_contribution + money); // has to be done so that all-ins dont lower the bet
                events.push(GameEvent::UpdateCurrentBet(self.current_bet));

                self.last_bettor = self.current_turn;

                player.money -= money;
                player.total_contribution += money;
                events.push(GameEvent::OwnedMoneyChange(self.current_turn, player.money));

                events.push(GameEvent::PlayerAction(self.current_turn, GamePlayerAction::AddMoney(money)));

                events.push(GameEvent::UpdatePots(self.compute_pots()));
            },
            GamePlayerAction::Fold => {
                player.has_folded = true;
                events.push(GameEvent::PlayerAction(self.current_turn, GamePlayerAction::Fold))
            },
            GamePlayerAction::Check => {
                if self.current_bet > player.total_contribution && player.money != 0 {
                    return None;
                }
                events.push(GameEvent::PlayerAction(self.current_turn, GamePlayerAction::Check))
            }
        }
        
        if self.players.iter().filter(|&&p| p.money > 0 && !p.has_folded).count() == 1 {
            events.push(GameEvent::Showdown(self.evaluate_showdown()));
            return Some(events);
        }
        
        let player_count = self.players.len() as u8;
        let mut next_turn = (self.current_turn + 1) % player_count;
        while let Some(&p) = self.players.get(next_turn as usize) {
            if !p.has_folded && p.money > 0 {
                break;
            }
            next_turn = (next_turn + 1) % player_count;
        } 

        if self.current_turn == self.last_bettor && matches!(action, GamePlayerAction::Check) {
            match self.current_phase {
                0 => events.push(GameEvent::RevealFlop(self.public_cards[0..3].try_into().unwrap())),
                1 => events.push(GameEvent::RevealTurn(self.public_cards[3])),
                2 => events.push(GameEvent::RevealRiver(self.public_cards[4])),
                3 => events.push(GameEvent::Showdown(self.evaluate_showdown())),
                _ => {} // should never happen
            }
            self.current_phase += 1;
        }

        self.current_turn = next_turn;

        events.push(GameEvent::NextPlayer(next_turn));

        Some(events)
    }

    fn evaluate_showdown(&mut self) -> ShowdownInfo {
        let mut steps = Vec::<ShowdownStep>::new();
        let info = self.get_showdown_info();
        let pots = self.compute_pots();

        let mut i = 0;
        while i < pots.len() {
            let pot = &pots[i];
            let pot_start_index = i;

            let mut eligible_players: Vec<(u8, HandRank)> = info.iter().enumerate().filter(|(id, _)| pot.eligible_players.contains(&(*id as u8))).map(|(id, (_, _, hand_rank))| (id as u8, hand_rank.clone())).collect();
            if eligible_players.is_empty() {
                continue;
            }
            eligible_players.sort_by(|(id1, hand_rank1), (id2, hand_rank2)| hand_rank2.cmp(&hand_rank1).then(id1.cmp(id2)));

            let mut winners = Vec::new();
            let mut players_iter = eligible_players.iter();
            winners.push(players_iter.next().unwrap());
            for player in players_iter {
                if player.1.cmp(&winners[0].1) == Ordering::Equal {
                    winners.push(player);
                } else {
                    break;
                }
            }

            let mut winnings = pot.money;
            while let Some(pot) = pots.get(i + 1) && winners.iter().all(|(id, _)| pot.eligible_players.contains(id)) {
                winnings += pot.money;
                i += 1;
            }

            let player_winnings = winnings / winners.len() as u32;
            let mut remainder = winnings % winners.len() as u32;
            for (winner, _) in winners.iter() {
                self.players[*winner as usize].money += player_winnings;
                if remainder > 0 {
                    self.players[*winner as usize].money += 1;
                    remainder -= 1;
                }
            }

            let win_reason = if winners.len() < eligible_players.len() {
                Some((compare_hand_ranks(&winners[0].1, &eligible_players[winners.len()].1).1, eligible_players[winners.len()].0))
            } else { None };

            steps.push(ShowdownStep {
                winners: winners.iter().map(|(id, _)| *id).collect(),
                winnings,
                pot_start_index: pot_start_index.try_into().unwrap(),
                pot_end_index: i.try_into().unwrap(),
                eligible_players: eligible_players.iter().map(|(id, _)| *id).collect(),
                win_reason
            });

            i += 1;
        }
        
        (info, steps)
    }

    pub fn compute_pots(&self) -> Vec<Pot> {
        let mut contributions: Vec<(u8, Player)> = self.players.iter().enumerate().filter(|(_, p)| p.total_contribution > 0).map(|(id, p)| (id as u8, *p)).collect();
        contributions.sort_by_key(|(_, p)| p.total_contribution);

        let mut pots = Vec::new();

        while !contributions.is_empty() {
            let level = contributions[0].1.total_contribution;
            let portion = level * contributions.len() as u32;

            if portion > 0 {
                pots.push(Pot { money: portion, eligible_players: contributions.iter().filter(|(_, p)| !p.has_folded).map(|(id, _)| *id).collect() });
            }

            for (_, player) in contributions.iter_mut() {
                player.total_contribution -= level;
            }
            contributions.retain(|(_, p)| p.total_contribution > 0);
        }
        
        pots
    }

    fn get_showdown_info(&self) -> Vec<([Card; 2], [Card; 5], HandRank)> {
        let mut showdown_info = Vec::new();
        for p in self.players.iter() {
            let mut all_cards = Vec::new();
            all_cards.extend_from_slice(&self.public_cards);
            all_cards.extend_from_slice(&p.private_cards);
            let (hand, hand_rank) = get_best_hand_rank(all_cards.as_slice().try_into().unwrap());
            showdown_info.push((p.private_cards, hand, hand_rank));
        }
        showdown_info
    }

    pub fn player(&self, id: u8) -> Player {
        self.players[id as usize]
    }

    pub fn player_mut(&mut self, id: u8) -> &mut Player {
        self.players.get_mut(id as usize).unwrap()
    }
}

pub fn make_game(lobby_players: Vec<u32> /* array of money amounts */) -> Option<Game> { // none means cant create game
    if lobby_players.len() < 3 {
        return None
    }
    if !lobby_players.iter().all(|&p| p > 10) {
        return None
    }

    let mut deck = get_shuffled_deck();

    let mut players = Vec::new();
    for (id, &money) in lobby_players.iter().enumerate() {
        players.push(Player {
            id: id as u8,
            money,
            total_contribution: 0,
            private_cards: [deck.pop().unwrap(), deck.pop().unwrap()],
            has_folded: false,
        });
    }

    let public_cards = [deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap()];

    let current_turn = 1;
    Some(Game { players, current_bet: 0, current_phase: 0, current_turn, last_bettor: 0, public_cards })
}

pub fn get_shuffled_deck() -> Vec<Card> {
    let mut deck = Vec::<Card>::new();
    for suit in 0..4 {
        for rank in 0..13 {
            deck.push(Card { rank, suit });
        }
    }

    deck.shuffle(&mut thread_rng());

    deck
}
