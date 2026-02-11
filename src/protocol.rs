use crate::{cards::{Card, HandCategory, HandRank, ShowdownDecidingFactor}, events::{ClientBound, GameEvent, GamePlayerAction, PlayerState, ServerBound}, game::{Pot, ShowdownStep}};

pub fn encode_server_bound(event: ServerBound) -> Vec<u8> {
    match event {
        ServerBound::Login(username) => append_username(vec![0], username),
        ServerBound::Disconnect => vec![1],
        ServerBound::Ready(ready) => vec![2, if ready {1} else {0}],
        ServerBound::GetPlayerList => vec![3],
        ServerBound::GameAction(action) => match action {
            GamePlayerAction::Check => vec![4],
            GamePlayerAction::AddMoney(money) => append_money(vec![5], money),
            GamePlayerAction::Fold => vec![6]
        }
    }
}

pub fn decode_server_bound(msg: &Vec<u8>) -> Option<ServerBound> {
    if msg.is_empty() { return None }
    match msg[0] {
        0 => {
            if msg.len() < 3 { return None }
            Some(ServerBound::Login(String::from_utf8(msg[1..].to_vec()).ok()?))
        },
        1 => {
            if msg.len() != 1 { return None }
            Some(ServerBound::Disconnect)
        },
        2 => {
            if msg.len() != 2 { return None }
            Some(ServerBound::Ready(msg[1] != 0))
        }
        3 => {
            if msg.len() != 1 { return None }
            Some(ServerBound::GetPlayerList)
        },
        4 => {
            if msg.len() != 1 { return None }
            Some(ServerBound::GameAction(GamePlayerAction::Check))
        },
        5 => {
            if msg.len() != 5 { return None }
            Some(ServerBound::GameAction(GamePlayerAction::AddMoney(u32::from_le_bytes([msg[1], msg[2], msg[3], msg[4]]))))
        },
        6 => {
            if msg.len() != 1 { return None }
            Some(ServerBound::GameAction(GamePlayerAction::Fold))
        },
        _ => None
    }
}

pub fn encode_client_bound(event: ClientBound) -> Vec<u8> {
    match event {
        ClientBound::UpdatePlayerList(players) => {
            let mut msg = vec![0];
            for (player_state, money, username) in players {
                msg.extend(append_username(append_money(vec![player_state as u8], money), username));
                msg.push(255);
            }
            msg
        },
        ClientBound::YourIndex(id) => vec![1, id],
        ClientBound::PlayerLeft(username) => append_username(vec![2], username),
        ClientBound::PlayerJoined(username) => append_username(vec![3], username),
        ClientBound::GameStarted(cards) => vec![4, cards[0].to_byte(), cards[1].to_byte()],
        ClientBound::GameEvent(game_event) => match game_event {
            GameEvent::PlayerAction(player, action) => match action {
                GamePlayerAction::Check => vec![5, player],
                GamePlayerAction::AddMoney(money) => append_money(vec![6, player], money),
                GamePlayerAction::Fold => vec![7, player]
            },
            GameEvent::OwnedMoneyChange(player, money) => append_money(vec![8, player], money),
            GameEvent::NextPlayer(player) => vec![9, player],
            GameEvent::UpdateCurrentBet(money) => append_money(vec![10], money),
            GameEvent::UpdatePots(pots) => {
                let mut msg = vec![11];
                for mut pot in pots {
                    msg.append(&mut pot.money.to_le_bytes().to_vec());
                    msg.append(&mut pot.eligible_players);
                    msg.push(255);
                }
                msg
            },
            GameEvent::RevealFlop(cards) => vec![12, cards[0].to_byte(), cards[1].to_byte(), cards[2].to_byte()],
            GameEvent::RevealTurn(card) => vec![13, card.to_byte()],
            GameEvent::RevealRiver(card) => vec![14, card.to_byte()],
            GameEvent::Showdown((hand_ranks, steps)) => {
                let mut msg = vec![15];
                for (private_cards, hand_cards, hand_rank) in hand_ranks {
                    msg.push(hand_rank.category as u8);
                    msg.append(&mut private_cards.iter().map(|c| c.to_byte()).collect());
                    msg.append(&mut hand_cards.iter().map(|c| c.to_byte()).collect());
                    msg.append(&mut encode_cards(&hand_rank.primary));
                    msg.append(&mut encode_cards(&hand_rank.secondary));
                    msg.append(&mut encode_cards(&hand_rank.kickers));
                }
                msg.push(255);

                for mut step in steps {
                    msg.append(&mut step.winners);
                    msg.push(255);
                    msg.append(&mut step.winnings.to_le_bytes().to_vec());
                    msg.push(step.pot_start_index);
                    msg.push(step.pot_end_index);
                    msg.append(&mut step.eligible_players);
                    msg.push(255);
                    if let Some((sdf, player)) = step.win_reason {
                        msg.append(&mut match sdf {
                            ShowdownDecidingFactor::Category => vec![0, 255, 255],
                            ShowdownDecidingFactor::Primary(cards1, cards2) => encode_showdown_deciding_factor(1, cards1, cards2),
                            ShowdownDecidingFactor::Secondary(cards1, cards2) => encode_showdown_deciding_factor(2, cards1, cards2),
                            ShowdownDecidingFactor::Kicker(cards1, cards2) => encode_showdown_deciding_factor(3, cards1, cards2),
                            ShowdownDecidingFactor::Tie => vec![4, 255, 255],
                        });
                        msg.push(player);
                    } else {
                        msg.append(&mut vec![255, 255, 255, 255]);
                    }
                }
                msg
            },
            GameEvent::InGamePlayerLeave(id) => vec![16, id]
        }
    }
}

pub fn decode_client_bound(msg: &Vec<u8>) -> Option<ClientBound> {
    if msg.is_empty() { return None }
    match msg[0] {
        0 => {
            let mut players = Vec::new();
            let mut idx = 1;
            while idx < msg.len() {
                if idx + 5 >= msg.len() { return None }
                let player_state = PlayerState::from_byte(msg[idx])?;
                let money = u32::from_le_bytes(msg.get(idx+1..idx+5)?.try_into().ok()?);
                idx += 5;
                let username = String::from_utf8(decode_byte_list(msg, &mut idx)?).ok()?;
                players.push((player_state, money, username));
            }
            Some(ClientBound::UpdatePlayerList(players))
        },
        1 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::YourIndex(msg[1]))
        },
        2 => {
            if msg.len() < 2 { return None }
            Some(ClientBound::PlayerLeft(String::from_utf8(msg[1..].to_vec()).ok()?))
        },
        3 => {
            if msg.len() < 2 { return None }
            Some(ClientBound::PlayerJoined(String::from_utf8(msg[1..].to_vec()).ok()?))
        },
        4 => {
            if msg.len() != 3 { return None }
            Some(ClientBound::GameStarted([Card::from_byte(msg[1])?, Card::from_byte(msg[2])?]))
        },
        5 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::GameEvent(GameEvent::PlayerAction(msg[1], GamePlayerAction::Check)))
        },
        6 => {
            if msg.len() != 6 { return None }
            Some(ClientBound::GameEvent(GameEvent::PlayerAction(msg[1], GamePlayerAction::AddMoney(u32::from_le_bytes(msg.get(2..)?.try_into().ok()?)))))
        },
        7 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::GameEvent(GameEvent::PlayerAction(msg[1], GamePlayerAction::Fold)))
        },
        8 => {
            if msg.len() < 6 { return None }
            let player = msg[1];
            let money = u32::from_le_bytes(msg.get(2..6)?.try_into().ok()?);
            Some(ClientBound::GameEvent(GameEvent::OwnedMoneyChange(player, money)))
        },
        9 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::GameEvent(GameEvent::NextPlayer(msg[1])))
        },
        10 => {
            if msg.len() != 5 { return None }
            Some(ClientBound::GameEvent(GameEvent::UpdateCurrentBet(u32::from_le_bytes(msg.get(1..)?.try_into().ok()?))))
        },
        11 => {
            let mut pots = Vec::new();
            let mut idx = 1;
            while idx < msg.len() {
                if idx + 4 >= msg.len() { return None }
                let money = u32::from_le_bytes([msg[idx], msg[idx+1], msg[idx+2], msg[idx+3]]);
                idx += 4;
                let eligible_players = decode_byte_list(msg, &mut idx)?;
                pots.push(Pot { money, eligible_players });
            }
            Some(ClientBound::GameEvent(GameEvent::UpdatePots(pots)))
        },
        12 => {
            if msg.len() != 4 { return None }
            Some(ClientBound::GameEvent(GameEvent::RevealFlop([Card::from_byte(msg[1])?, Card::from_byte(msg[2])?, Card::from_byte(msg[3])?])))
        },
        13 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::GameEvent(GameEvent::RevealTurn(Card::from_byte(msg[1])?)))
        },
        14 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::GameEvent(GameEvent::RevealRiver(Card::from_byte(msg[1])?)))
        },
        15 => {
            let mut hand_ranks = Vec::new();
            let mut idx = 1;
            while idx < msg.len() && msg[idx] != 255 {
                if idx + 8 >= msg.len() { return None }
                let category = msg[idx];
                let private_cards = [Card::from_byte(msg[idx+1])?, Card::from_byte(msg[idx+2])?];
                let hand_cards = [Card::from_byte(msg[idx+3])?, Card::from_byte(msg[idx+4])?, Card::from_byte(msg[idx+5])?, Card::from_byte(msg[idx+6])?, Card::from_byte(msg[idx+7])?,];
                idx += 8;
                let primary = decode_card_list(msg, &mut idx)?;
                let secondary = decode_card_list(msg, &mut idx)?;
                let kickers = decode_card_list(msg, &mut idx)?;
                let hand_rank = HandRank { category: HandCategory::from_byte(category)?, primary, secondary, kickers };
                hand_ranks.push((private_cards, hand_cards, hand_rank));
            }
            idx += 1;

            let mut steps = Vec::new();
            while idx < msg.len() {
                let winners = decode_byte_list(msg, &mut idx)?;
                if idx + 6 >= msg.len() { return None }
                let winnings = u32::from_le_bytes([msg[idx], msg[idx+1], msg[idx+2], msg[idx+3]]);
                let pot_start_index = msg[idx+4];
                let pot_end_index = msg[idx+5];
                idx += 6;
                let eligible_players = decode_byte_list(msg, &mut idx)?;
                let win_reason;
                match msg[idx] {
                    255 => {win_reason = None; idx += 4}
                    0 => {win_reason = Some((ShowdownDecidingFactor::Category, *msg.get(idx+1)?)); idx += 4},
                    1 => {win_reason = Some((ShowdownDecidingFactor::Primary(decode_card_list(msg, &mut idx)?, decode_card_list(msg, &mut idx)?), *msg.get(idx+1)?)); idx += 1}
                    2 => {win_reason = Some((ShowdownDecidingFactor::Secondary(decode_card_list(msg, &mut idx)?, decode_card_list(msg, &mut idx)?), *msg.get(idx+1)?)); idx += 1}
                    3 => {win_reason = Some((ShowdownDecidingFactor::Kicker(decode_card_list(msg, &mut idx)?, decode_card_list(msg, &mut idx)?), *msg.get(idx+1)?)); idx += 1}
                    4 => {win_reason = Some((ShowdownDecidingFactor::Tie, *msg.get(idx+1)?)); idx += 4}
                    _ => return None,
                };
                steps.push(ShowdownStep { winners, winnings, pot_start_index, pot_end_index, eligible_players, win_reason });
            }
            Some(ClientBound::GameEvent(GameEvent::Showdown((hand_ranks, steps))))
        },
        16 => {
            if msg.len() != 2 { return None }
            Some(ClientBound::GameEvent(GameEvent::InGamePlayerLeave(msg[1])))
        }
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

fn encode_cards(cards: &Vec<Card>) -> Vec<u8> {
    let mut part = Vec::new();
    for card in cards {
        part.push(card.to_byte());
    }
    part.push(255);
    part
}

fn encode_showdown_deciding_factor(id: u8, cards1: Vec<Card>, cards2: Vec<Card>) -> Vec<u8> {
    let mut part = vec![id];
    part.append(&mut encode_cards(&cards1));
    part.append(&mut encode_cards(&cards2));
    part
}

fn decode_byte_list(msg: &Vec<u8>, idx: &mut usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    while *msg.get(*idx)? != 255 {
        bytes.push(msg[*idx]);
        *idx += 1;
    }
    *idx += 1;
    Some(bytes)
}

fn decode_card_list(msg: &Vec<u8>, idx: &mut usize) -> Option<Vec<Card>> {
    let mut list = Vec::new();
    for byte in decode_byte_list(msg, idx)? {
        list.push(Card::from_byte(byte)?);
    }
    Some(list)
}
