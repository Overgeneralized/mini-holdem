use std::{cmp::{Ordering, max}, collections::HashMap};
use rand::{seq::SliceRandom, thread_rng};

use crate::{cards::{Card, HandRank, ShowdownDecidingFactor, compare_hand_ranks, get_best_hand_rank}, events::{GameEvent, GamePlayerAction}};

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
    pub players: HashMap<u8, Player>,
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
    pub win_reason: ShowdownDecidingFactor, // only used if it wasnt a tie
}

impl Game {
    pub fn advance_game(&mut self, action: GamePlayerAction) -> Option<Vec<GameEvent>> { // none means illegal action
        if self.current_phase == 4 { return None }
        let player = self.players.get_mut(&self.current_turn).unwrap();
        let mut events = Vec::<GameEvent>::new();
        match action {
            GamePlayerAction::AddMoney(money) => {
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
        
        if self.players.values().filter(|&&p| p.money > 0 && !p.has_folded).count() == 1 {
            events.push(GameEvent::Showdown(self.get_showdown_info()));
            return Some(events);
        }
        
        let player_count = self.players.len() as u8;
        let mut next_turn = (self.current_turn + 1) % player_count;
        while let Some(&p) = self.players.get(&next_turn) {
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
                3 => events.push(GameEvent::Showdown(self.get_showdown_info())),
                _ => {} // should never happen
            }
            self.current_phase += 1;
        }

        self.current_turn = next_turn;

        events.push(GameEvent::NextPlayer(next_turn));

        Some(events)
    }

    pub fn evaluate_showdown(&mut self) -> Vec<ShowdownStep> {
        let mut steps = Vec::<ShowdownStep>::new();
        let info = self.get_showdown_info();
        let pots = self.compute_pots();

        let mut i = 0;
        while i < pots.len() {
            let pot = &pots[i];
            let pot_start_index = i;

            let mut eligible_players: Vec<(&u8, HandRank)> = info.iter().filter(|p| pot.eligible_players.contains(p.0)).map(|p| (p.0, p.1.1.clone())).collect();
            if eligible_players.is_empty() {
                continue;
            }
            eligible_players.sort_by(|p1, p2| p2.1.cmp(&p1.1).then(p1.0.cmp(p2.0)));

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
            while let Some(pot) = pots.get(i + 1) && winners.iter().all(|p| pot.eligible_players.contains(p.0)) {
                winnings += pot.money;
                i += 1;
            }

            let player_winnings = winnings / winners.len() as u32;
            let mut remainder = winnings % winners.len() as u32;
            for winner in winners.iter() {
                self.players.get_mut(winner.0).unwrap().money += player_winnings;
                if remainder > 0 {
                    self.players.get_mut(winner.0).unwrap().money += 1;
                    remainder -= 1;
                }
            }

            let win_reason = if winners.len() == 1 && eligible_players.len() > 1 {
                compare_hand_ranks(&winners[0].1, &eligible_players[1].1).1
            } else {
                ShowdownDecidingFactor::None
            };

            steps.push(ShowdownStep {
                winners: winners.iter().map(|p| *p.0).collect(),
                winnings,
                pot_start_index: pot_start_index.try_into().unwrap(),
                pot_end_index: i.try_into().unwrap(),
                eligible_players: eligible_players.iter().map(|p| *p.0).collect(),
                win_reason
            });

            i += 1;
        }
        
        steps
    }

    pub fn compute_pots(&self) -> Vec<Pot> {
        let mut contributions: Vec<(u8, Player)> = self.players.iter().filter(|(_, p)| p.total_contribution > 0).map(|(id, p)| (*id, *p)).collect();
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

    fn get_showdown_info(&self) -> HashMap<u8, ([Card; 2], HandRank)> {
        let mut showdown_info = HashMap::new();
        for p in self.players.iter() {
            let mut all_cards = Vec::with_capacity(7);
            all_cards.extend_from_slice(&p.1.private_cards);
            all_cards.extend_from_slice(&self.public_cards);
            showdown_info.insert(*p.0, (p.1.private_cards, get_best_hand_rank(all_cards.as_slice().try_into().unwrap())));
        }
        showdown_info
    }

    pub fn player(&self, id: u8) -> Player {
        *self.players.get(&id).unwrap()
    }

    pub fn player_mut(&self, id: u8) -> &Player {
        self.players.get(&id).unwrap()
    }
}

pub fn make_game(lobby_players: Vec<(u8, u32)> /* player id (turn order at the table) and money */) -> Option<Game> { // none means cant create game
    if lobby_players.len() < 3 {
        return None
    }
    if !lobby_players.iter().all(|p| p.1 > 10) {
        return None
    }

    let mut deck = get_shuffled_deck();

    let mut players = HashMap::new();
    for (id, money) in lobby_players {
        players.insert(id, Player {
            id,
            money,
            total_contribution: 0,
            private_cards: [deck.pop().unwrap(), deck.pop().unwrap()],
            has_folded: false,
        });
    }

    let public_cards = [deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap()];

    players.get_mut(&1).unwrap().money -= 5;
    players.get_mut(&1).unwrap().total_contribution += 5;
    players.get_mut(&2).unwrap().money -= 10;
    players.get_mut(&2).unwrap().total_contribution += 10;
    let current_turn = 3 % players.len() as u8;
    Some(Game { players, current_bet: 10, current_phase: 0, current_turn, last_bettor: 2, public_cards })
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
