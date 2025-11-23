use std::{cmp::Ordering, collections::HashMap, time::{SystemTime, UNIX_EPOCH}};
use crate::cards::{Card, HandRank, ShowdownDecidingFactor, compare_hand_ranks, get_best_hand_rank};

#[derive(Clone, Copy)]
pub struct Player {
    pub money: u32,
    total_contribution: u32,
    pub private_cards: [Card; 2],
    pub has_folded: bool,
}

pub struct Pot {
    pub money: u32,
    pub eligible_players: Vec<u8>,
}

pub struct ShowdownStep {
    pub winners: Vec<u8>,
    pub winnings: u32,
    pub pot_start_index: u8, // players can win multiple pots next to each other at once, both of those are inclusive
    pub pot_end_index: u8,
    pub eligible_players: Vec<u8>,
    pub win_reason: ShowdownDecidingFactor, // only used if it wasnt a tie
}

pub struct Game {
    pub players: HashMap<u8, Player>,
    current_bet: u32,
    current_phase: u8, // 0 - 4, preflop, flop, turn, river, showdown
    pub current_turn: u8,
    last_bettor: u8,
    public_cards: [Card; 5],
}

// the client is able to tell when something is a check, call, bet, raise or an all-in
pub enum PlayerAction {
    Fold,
    Check,
    AddMoney(u32), // can be anything: call, bet, raise, all-in
}

pub enum Event {
    PlayerAction(u8, PlayerAction),
    OwnedMoneyChange(u8, u32),
    NextPlayer(u8),
    UpdateCurrentBet(u32),
    UpdatePots(Vec<Pot>),
    RevealFlop([Card; 3]),
    RevealTurn(Card),
    RevealRiver(Card),
    Showdown(HashMap<u8, ([Card; 2], HandRank)>),
}

impl Game {
    pub fn advance_game(&mut self, action: PlayerAction) -> Vec<Event> {
        let player = self.players.get_mut(&self.current_turn).unwrap();
        let mut events = Vec::<Event>::new();
        match action {
            PlayerAction::AddMoney(money) => {
                assert!(money > 0);
                if player.total_contribution + money < self.current_bet {
                    return events;
                }
                if self.current_bet < money + player.total_contribution && money > player.money {
                    return events;
                }

                let real_money = if money > player.money {
                    player.money
                } else {
                    money
                };
                
                if player.total_contribution + money > self.current_bet {
                    self.current_bet = player.total_contribution + money;
                    self.last_bettor = self.current_turn;
                    events.push(Event::UpdateCurrentBet(self.current_bet));
                }

                player.money -= real_money;
                player.total_contribution += real_money;
                events.push(Event::OwnedMoneyChange(self.current_turn, player.money));

                events.push(Event::PlayerAction(self.current_turn, PlayerAction::AddMoney(real_money)));
            },
            PlayerAction::Fold => {
                player.has_folded = true;
                events.push(Event::PlayerAction(self.current_turn, PlayerAction::Fold))
            },
            PlayerAction::Check => {
                if self.current_bet > player.total_contribution {
                    return events;
                }
                events.push(Event::PlayerAction(self.current_turn, PlayerAction::Check))
            }
        }

        events.push(Event::UpdatePots(self.compute_pots()));
        
        if self.players.values().filter(|&&p| p.money > 0 && !p.has_folded).count() == 1 {
            events.push(Event::Showdown(self.get_showdown_info()));
            return events;
        }
        
        let player_count = self.players.len() as u8;
        let mut next_turn = (self.current_turn + 1) % player_count;
        while let Some(&p) = self.players.get(&next_turn) {
            if !p.has_folded && p.money > 0 {
                break;
            }
            next_turn = (next_turn + 1) % player_count;
        } 

        if next_turn == self.last_bettor {
            self.current_bet += 1;
            match self.current_phase {
                1 => events.push(Event::RevealFlop(self.public_cards[0..3].try_into().unwrap())),
                2 => events.push(Event::RevealTurn(self.public_cards[3])),
                3 => events.push(Event::RevealRiver(self.public_cards[4])),
                4 => events.push(Event::Showdown(self.get_showdown_info())),
                _ => {} // should never happen
            }
            self.last_bettor = next_turn;
            self.current_phase += 1;
        }

        events.push(Event::NextPlayer(next_turn));

        events
    }

    pub fn evaluate_showdown(&mut self) -> Vec<ShowdownStep> {
        assert!(self.current_phase == 4);
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

    fn compute_pots(&self) -> Vec<Pot> {
        let mut contributions: Vec<(u8, Player)> = self.players.iter().filter(|p| p.1.total_contribution > 0).map(|p| (*p.0, *p.1)).collect();
        contributions.sort_by_key(|p| p.1.total_contribution);

        let mut pots = Vec::new();
        let mut previous_level = 0;

        while !contributions.is_empty() {
            let level = contributions[0].1.total_contribution;
            let portion = (level - previous_level) * contributions.len() as u32;

            pots.push(Pot { money: portion, eligible_players: contributions.iter().filter(|p| !p.1.has_folded).map(|p| p.0).collect() });

            for contrib in contributions.iter_mut() {
                contrib.1.total_contribution -= level;
            }
            contributions.retain(|p| p.1.total_contribution > 0);

            previous_level = level;
        }
        
        pots
    }

    fn get_showdown_info(&self) -> HashMap<u8, ([Card; 2], HandRank)>{
        let mut showdown_info = HashMap::new();
        for p in self.players.clone() {
            let mut all_cards = Vec::with_capacity(7);
            all_cards.extend_from_slice(&p.1.private_cards);
            all_cards.extend_from_slice(&self.public_cards);
            showdown_info.insert(p.0, (p.1.private_cards, get_best_hand_rank(all_cards.as_slice().try_into().unwrap())));
        }
        showdown_info
    }
}

pub fn make_game(lobby_players: Vec<(u8, u32)> /* player id (turn order at the table) and money */) -> Game {
    assert!(lobby_players.len() >= 3);
    assert!(lobby_players.iter().all(|p| p.1 > 10));

    let mut deck = get_shuffled_deck();

    let mut players = HashMap::new();
    for player in lobby_players {
        players.insert(player.0, Player {
            money: player.1,
            total_contribution: 0,
            private_cards: [deck.pop().unwrap(), deck.pop().unwrap()],
            has_folded: false,
        });
    }

    let public_cards = [deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap(), deck.pop().unwrap()];

    players.get_mut(&1).unwrap().money -= 5;
    players.get_mut(&2).unwrap().money -= 10;
    let current_turn = 3 % players.len() as u8;
    Game { players, current_bet: 0, current_phase: 0, current_turn, last_bettor: 2, public_cards }
}

fn get_shuffled_deck() -> Vec<Card> {
    let mut deck = Vec::<Card>::new();
    for suit in 0..4 {
        for rank in 0..13 {
            deck.push(Card { rank, suit });
        }
    }

    let mut rng = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() ^ std::process::id() as u128;
    for i in (1..deck.len()).rev() {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = rng as usize % (i + 1);
        deck.swap(i, j);
    }

    deck
}
