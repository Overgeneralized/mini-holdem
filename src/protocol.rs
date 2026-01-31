use std::collections::HashMap;

use crate::{cards::{Card, HandCategory, ShowdownDecidingFactor}, events::{ClientBound, GameEvent, GamePlayerAction, ServerBound}, game::{Pot, ShowdownStep}};

pub fn encode_server_bound(event: ServerBound) -> Vec<u8> {
    match event {
        ServerBound::Login(username) => append_username(vec![0 ,0], username),
        ServerBound::Leave => vec![0, 2],
        ServerBound::Ready(ready) => vec![0, 3, if ready {1} else {0}],
        ServerBound::GetPlayerList => vec![0, 4],
        ServerBound::GameAction(action) => match action {
            GamePlayerAction::Check => vec![1, 0],
            GamePlayerAction::AddMoney(money) => append_money(vec![1, 1], money),
            GamePlayerAction::Fold => vec![1, 2]
        }
    }
}

pub fn decode_server_bound(msg: &Vec<u8>) -> Option<ServerBound> {
    if msg.len() < 2 { return None }
    match msg[0..2] {
        [0, 0] => {
            if msg.len() < 3 { return None }
            let username_bytes = &msg[2..];
            Some(ServerBound::Login(String::from_utf8(username_bytes.to_vec()).ok()?))
        },
        [0, 2] => Some(ServerBound::Leave),
        [0, 3] => {
            if msg.len() < 3 { return None }
            Some(ServerBound::Ready(msg[2] != 0))
        }
        [0, 4] => Some(ServerBound::GetPlayerList),
        [1, 0] => Some(ServerBound::GameAction(GamePlayerAction::Check)),
        [1, 1] => {
            if msg.len() < 6 { return None }
            Some(ServerBound::GameAction(GamePlayerAction::AddMoney(u32::from_le_bytes([msg[2], msg[3], msg[4], msg[5]]))))
        },
        [1, 2] => Some(ServerBound::GameAction(GamePlayerAction::Fold)),
        _ => None
    }
}

pub fn encode_client_bound(event: ClientBound) -> Vec<u8> {
    match event {
        ClientBound::UpdatePlayerList(players) => {
            let mut msg = vec![0, 0];
            for (ready, folded, money, username) in players {
                msg.extend(append_username(append_money(vec![if ready {1} else {0}, if folded {1} else {0}], money), username));
                msg.push(255);
            }
            msg
        },
        ClientBound::YourId(id) => vec![0, 4, id],
        ClientBound::PlayerLeft(username) => append_username(vec![0, 5], username),
        ClientBound::PlayerJoined(username) => append_username(vec![0, 6], username),
        ClientBound::GameStarted(cards) => vec![1, 0, cards[0].to_byte(), cards[1].to_byte()],
        ClientBound::GameEvent(game_event) => match game_event {
            GameEvent::PlayerAction(player, action) => match action {
                GamePlayerAction::Check => vec![1, 3, player, 0],
                GamePlayerAction::AddMoney(money) => append_money(vec![1, 3, player, 1], money),
                GamePlayerAction::Fold => vec![1, 3, player, 2]
            },
            GameEvent::OwnedMoneyChange(player, money) => append_money(vec![0, 2, player], money),
            GameEvent::NextPlayer(player) => vec![1, 2, player],
            GameEvent::UpdateCurrentBet(money) => append_money(vec![1, 5], money),
            GameEvent::UpdatePots(pots) => {
                let mut msg = vec![1, 6];
                for pot in pots {
                    msg.append(&mut pot.money.to_le_bytes().to_vec());
                    for player in pot.eligible_players {
                        msg.push(player);
                    }
                    msg.push(255);
                }
                msg
            },
            GameEvent::RevealFlop(cards) => vec![1, 8, cards[0].to_byte(), cards[1].to_byte(), cards[2].to_byte()],
            GameEvent::RevealTurn(card) => vec![1, 9, card.to_byte()],
            GameEvent::RevealRiver(card) => vec![1, 10, card.to_byte()],
            GameEvent::Showdown(map) => {
                let mut msg = vec![1, 11];
                for (id, (cards, hand_rank)) in map {
                    msg.append(&mut vec![id, cards[0].to_byte(), cards[1].to_byte(), hand_rank.category as u8, option_card_to_byte(hand_rank.primary), option_card_to_byte(hand_rank.secondary)]);
                    for kicker in &hand_rank.kickers {
                        msg.push(kicker.to_byte());
                    }
                    for _ in 0..5-hand_rank.kickers.len() {
                        msg.push(255);
                    }
                }
                msg
            },
            GameEvent::ShowdownSteps(steps) => {
                let mut msg = vec![1, 13];
                for step in steps {
                    for winner in step.winners {
                        msg.push(winner);
                    }
                    msg.push(255);
                    msg.append(&mut step.winnings.to_le_bytes().to_vec());
                    msg.push(step.pot_start_index);
                    msg.push(step.pot_end_index);
                    for player in step.eligible_players {
                        msg.push(player);
                    }
                    msg.push(255);
                    msg.append(&mut match step.win_reason {
                        ShowdownDecidingFactor::Category => vec![0, 255, 255],
                        ShowdownDecidingFactor::Primary(card1, card2) => vec![1, card1.to_byte(), card2.to_byte()],
                        ShowdownDecidingFactor::Secondary(card1, card2) => vec![2, card1.to_byte(), card2.to_byte()],
                        ShowdownDecidingFactor::Kicker(card1, card2) => vec![3, card1.to_byte(), card2.to_byte()],
                        ShowdownDecidingFactor::None => vec![255, 255, 255],
                    });
                }
                msg
            }
        }
    }
}

pub fn decode_client_bound(msg: &Vec<u8>) -> Option<ClientBound> {
    if msg.len() < 2 { return None }
    match msg[0..2] {
        [0, 0] => {
            let mut players = Vec::new();
            let mut idx = 2;
            while idx < msg.len() {
                if idx + 3 > msg.len() { return None }
                let ready = msg[idx] != 0;
                let folded = msg[idx+1] != 0;
                let money_bytes = msg.get(idx+2..idx+6)?;
                let money = u32::from_le_bytes([money_bytes[0], money_bytes[1], money_bytes[2], money_bytes[3]]);
                idx += 6;
                let mut username_bytes = Vec::new();
                while idx < msg.len() && msg[idx] != 255 {
                    username_bytes.push(msg[idx]);
                    idx += 1;
                }
                idx += 1; // skip 255
                let username = String::from_utf8(username_bytes).ok()?;
                players.push((ready, folded, money, username));
            }
            Some(ClientBound::UpdatePlayerList(players))
        },
        [0, 2] => {
            if msg.len() < 6 { return None }
            let player = msg[2];
            let money_bytes = msg.get(3..7)?;
            let money = u32::from_le_bytes([money_bytes[0], money_bytes[1], money_bytes[2], money_bytes[3]]);
            Some(ClientBound::GameEvent(GameEvent::OwnedMoneyChange(player, money)))
        },
        [0, 4] => {
            if msg.len() < 3 { return None }
            Some(ClientBound::YourId(msg[2]))
        },
        [0, 5] => {
            if msg.len() < 3 { return None }
            Some(ClientBound::PlayerLeft(String::from_utf8(msg[2..].to_vec()).ok()?))
        },
        [0, 6] => {
            if msg.len() < 3 { return None }
            Some(ClientBound::PlayerJoined(String::from_utf8(msg[2..].to_vec()).ok()?))
        },
        [1, 0] => {
            if msg.len() < 4 { return None }
            Some(ClientBound::GameStarted([Card::from_byte(msg[2])?, Card::from_byte(msg[3])?]))
        },
        [1, 2] => {
            if msg.len() < 3 { return None }
            Some(ClientBound::GameEvent(GameEvent::NextPlayer(msg[2])))
        },
        [1, 3] => {
            if msg.len() < 4 { return None }
            let player = msg[2];
            match msg[3] {
                0 => Some(ClientBound::GameEvent(GameEvent::PlayerAction(player, GamePlayerAction::Check))),
                1 => {
                    if msg.len() < 8 { return None }
                    let money_bytes = msg.get(4..8)?;
                    let money = u32::from_le_bytes([money_bytes[0], money_bytes[1], money_bytes[2], money_bytes[3]]);
                    Some(ClientBound::GameEvent(GameEvent::PlayerAction(player, GamePlayerAction::AddMoney(money))))
                },
                2 => Some(ClientBound::GameEvent(GameEvent::PlayerAction(player, GamePlayerAction::Fold))),
                _ => None,
            }
        },
        [1, 5] => {
            if msg.len() < 6 { return None }
            let money_bytes = msg.get(2..6)?;
            let money = u32::from_le_bytes([money_bytes[0], money_bytes[1], money_bytes[2], money_bytes[3]]);
            Some(ClientBound::GameEvent(GameEvent::UpdateCurrentBet(money)))
        },
        [1, 6] => {
            let mut pots = Vec::new();
            let mut idx = 2;
            while idx < msg.len() {
                if idx + 4 > msg.len() { return None }
                let money = u32::from_le_bytes([msg[idx], msg[idx+1], msg[idx+2], msg[idx+3]]);
                idx += 4;
                let mut eligible_players = Vec::new();
                while idx < msg.len() && msg[idx] != 255 {
                    eligible_players.push(msg[idx]);
                    idx += 1;
                }
                idx += 1; // skip 255
                pots.push(Pot { money, eligible_players });
            }
            Some(ClientBound::GameEvent(GameEvent::UpdatePots(pots)))
        },
        [1, 8] => {
            if msg.len() < 5 { return None }
            let cards = [Card::from_byte(msg[2])?, Card::from_byte(msg[3])?, Card::from_byte(msg[4])?];
            Some(ClientBound::GameEvent(GameEvent::RevealFlop(cards)))
        },
        [1, 9] => {
            if msg.len() < 3 { return None }
            Some(ClientBound::GameEvent(GameEvent::RevealTurn(Card::from_byte(msg[2])?)))
        },
        [1, 10] => {
            if msg.len() < 3 { return None }
            Some(ClientBound::GameEvent(GameEvent::RevealRiver(Card::from_byte(msg[2])?)))
        },
        [1, 11] => {
            let mut map = HashMap::new();
            let mut idx = 2;
            while idx < msg.len() {
                if idx + 6 > msg.len() { return None }
                let id = msg[idx];
                let card1 = Card::from_byte(msg[idx+1])?;
                let card2 = Card::from_byte(msg[idx+2])?;
                let category = msg[idx+3];
                let primary = if msg[idx+4] == 255 { None } else { Some(Card::from_byte(msg[idx+4])?) };
                let secondary = if msg[idx+5] == 255 { None } else { Some(Card::from_byte(msg[idx+5])?) };
                idx += 6;
                let mut kickers = Vec::new();
                for _ in 0..5 {
                    if idx >= msg.len() { return None }
                    if msg[idx] != 255 {
                        kickers.push(Card::from_byte(msg[idx])?);
                    }
                    idx += 1;
                }
                let hand_rank = crate::cards::HandRank { category: HandCategory::from_byte(category)?, primary, secondary, kickers };
                map.insert(id, ([card1, card2], hand_rank));
            }
            Some(ClientBound::GameEvent(GameEvent::Showdown(map)))
        },
        [1, 13] => {
            let mut steps = Vec::new();
            let mut idx = 2;
            while idx < msg.len() {
                let mut winners = Vec::new();
                while idx < msg.len() && msg[idx] != 255 {
                    winners.push(msg[idx]);
                    idx += 1;
                }
                idx += 1; // skip 255
                if idx + 8 > msg.len() { return None }
                let winnings = u32::from_le_bytes([msg[idx], msg[idx+1], msg[idx+2], msg[idx+3]]);
                let pot_start_index = msg[idx+4];
                let pot_end_index = msg[idx+5];
                idx += 6;
                let mut eligible_players = Vec::new();
                while idx < msg.len() && msg[idx] != 255 {
                    eligible_players.push(msg[idx]);
                    idx += 1;
                }
                idx += 1; // skip 255
                if idx + 3 > msg.len() { return None }
                let win_reason = match msg[idx] {
                    0 => ShowdownDecidingFactor::Category,
                    1 => ShowdownDecidingFactor::Primary(Card::from_byte(msg[idx+1])?, Card::from_byte(msg[idx+2])?),
                    2 => ShowdownDecidingFactor::Secondary(Card::from_byte(msg[idx+1])?, Card::from_byte(msg[idx+2])?),
                    3 => ShowdownDecidingFactor::Kicker(Card::from_byte(msg[idx+1])?, Card::from_byte(msg[idx+2])?),
                    255 => ShowdownDecidingFactor::None,
                    _ => return None,
                };
                idx += 3;
                steps.push(ShowdownStep { winners, winnings, pot_start_index, pot_end_index, eligible_players, win_reason });
            }
            Some(ClientBound::GameEvent(GameEvent::ShowdownSteps(steps)))
        },
        _ => None,
    }
}

fn append_money(mut msg: Vec<u8>, money: u32) -> Vec<u8> {
    msg.append(&mut money.to_le_bytes().to_vec());
    msg
}

fn append_username(mut msg: Vec<u8>, username: String) -> Vec<u8> {
    msg.append(&mut username.as_bytes().to_vec());
    msg
}

fn option_card_to_byte(card: Option<Card>) -> u8 {
    match card {
        Some(card) => card.to_byte(),
        None => 255u8,
    }
}
