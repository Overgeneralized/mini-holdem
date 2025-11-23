use std::{collections::{HashMap, HashSet}, io::{ErrorKind, Read, Write}, net::{TcpListener, TcpStream}, sync::{Arc, Mutex, MutexGuard}, thread};

use mini_holdem::{cards::{Card, ShowdownDecidingFactor}, game::{Event, Game, PlayerAction, ShowdownStep, make_game}};

struct GameState {
    broadcast_events: Vec<Vec<u8>>,
    private_events: HashMap<u64, Vec<Vec<u8>>>, // s to c
    client_events: HashMap<u64, Vec<Vec<u8>>>, // c to s
}

type SharedState = Arc<Mutex<GameState>>;

struct User {
    money: u32,
    username: String,
    ready: bool,
}

struct Lobby {
    players: HashMap<u64, User>,
    id_list: Vec<u64>, // basically a Hashmap<u8, u64>
    default_money: u32,
    game: Option<Game>,
    queued_for_removal: HashSet<u8>,
}

fn main() -> std::io::Result<()> {
    let listener = match TcpListener::bind("0.0.0.0:9194") {
        Ok(conn) => conn,
        Err(err) => panic!("Couldn't to bind to 0.0.0.0:9194 {}", err),
    };
    println!("Listening on 0.0.0.0 to port 9194.");
    listener.set_nonblocking(false)?;

    let state = Arc::new(Mutex::new(GameState { broadcast_events: Vec::new(), private_events: HashMap::new(), client_events: HashMap::new() }));

    let temp_state = Arc::clone(&state);
    thread::spawn(move || server_run(temp_state));

    let mut next_id: u64 = 0;
    for stream in listener.incoming() {
        let id = next_id;
        next_id += 1;
        let stream = stream?;
        stream.set_nonblocking(true)?;
        let state = Arc::clone(&state);
        thread::spawn(move || {
            handle_client(id, stream, state);
        });
    }

    Ok(())
}

// c to s:
// 0 0 name: login
// 0 1 cmd args: cmds
// psuedo: 0 2: leaving
// 0 3 bool: ready / not ready
// 0 4: ping for private player list message
//
// s to c
// 0 0: clear player list
// 0 1 ready money username: add player to player list
// 0 2 id money: update player money, also applies in game
// 0 3 id bool: update ready status
// PRIVATE 0 4 id: tell a players info
// 0 5 id: player left the game

// s to c
// 1 0: starting game!
// PRIVATE 1 1 card card: communicate private cards
//
// c to s
// 1 0: check
// 1 1 money: add money
// 1 2: fold

fn server_run(shared: SharedState) {
    let mut client_event_indexes = HashMap::<u64, usize>::new();
    let mut lobby = Lobby { players: HashMap::new(), id_list: Vec::new(), default_money: 1000, game: None, queued_for_removal: HashSet::new() };
    loop {
        {
            let mut state = shared.lock().unwrap();
            let client_events = state.client_events.clone();
            
            for (client_id, client_messages) in client_events {
                for message in client_messages.iter().skip(*client_event_indexes.entry(client_id).or_insert(0)) {
                    handle_message(message, client_id, &mut lobby, &mut state);
                }
                *client_event_indexes.get_mut(&client_id).unwrap() = client_messages.len();
            }
        }
        thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn handle_message(message: &[u8], client: u64, lobby: &mut Lobby, state: &mut MutexGuard<'_, GameState>) {
    if message.len() < 2 {
        return;
    }
    if message[0] == 0u8 && message[1] == 4u8 {
        send_player_list_update(lobby, state, Some(client));
    }
    if !lobby.id_list.contains(&client) {
        if message.len() > 2 && message.len() <= 18 && message[0] == 0 && message[1] == 0 {
            let mut name = String::new();
            for c in &message[2..] {
                name.push(*c as char);
            }
            if !name.is_ascii() {
                return;
            }
            if name.contains(" ") {
                return;
            }
            if lobby.players.values().any(|n| n.username.eq_ignore_ascii_case(&name)) {
                return;
            }
            lobby.players.insert(client, User { money: lobby.default_money, username: name, ready: false });
            lobby.id_list.push(client);
            send_player_list_update(lobby, state, None);
        }
        return;
    }
    if message[0] == 0u8 {
        if let None = lobby.game && message[1] == 1u8 && lobby.id_list[0] == client {
            // admin commands
            // maybe some add non admin commands???
        } else if message[1] == 2u8 {
            if let Some(p) = find_id_from_client(lobby, client) && !lobby.queued_for_removal.contains(&p.0) {
                state.broadcast_events.push(vec![0u8, 5u8, p.0]);
                if let Some(game) = &mut lobby.game {
                    lobby.queued_for_removal.insert(p.0);
                    if lobby.id_list[game.current_turn as usize] == client {
                        advance_game(PlayerAction::Fold, lobby, state);
                    } else {
                        game.players.get_mut(&p.0).unwrap().has_folded = true;
                    }
                } else {
                    lobby.players.remove(&p.1);
                    lobby.id_list.remove(p.0 as usize);
                    send_player_list_update(lobby, state, None);
                    check_for_game_start(state, lobby);
                }
            }
        } else if message[1] == 3u8 && message.len() >= 3 {
            lobby.players.get_mut(&client).unwrap().ready = message[2] != 0u8;

            let id = find_id_from_client(lobby, client).unwrap().0 as u8;
            state.broadcast_events.push(vec![0u8, 3u8, id, (message[2] != 0u8) as u8]);

            check_for_game_start(state, lobby);
        }
    } else if message[0] == 1u8 {
        if lobby.game.as_ref().unwrap().current_turn != find_id_from_client(lobby, client).unwrap().0 {
            return;
        }
        let player_action = match message[1] {
            0u8 => PlayerAction::Check,
            1u8 => {
                let mut money = 0;
                for (i, &byte) in message.iter().skip(2).enumerate() {
                    money |= (byte as u32) << (i * 8);
                }
                PlayerAction::AddMoney(money)
            },
            2u8 => PlayerAction::Fold,
            _ => return,
        };
        advance_game(player_action, lobby, state);
    }
}

fn check_for_game_start(state: &mut MutexGuard<'_, GameState>, lobby: &mut Lobby) {
    if lobby.players.iter().all(|p| p.1.ready) && lobby.id_list.len() >= 3 {
        lobby.game = Some(make_game(lobby.players.iter().map(|p| (find_id_from_client(lobby, *p.0).unwrap().0, p.1.money)).collect()));
        state.broadcast_events.push(vec![1u8, 0u8]);

        let mut msg = vec![0u8, 2u8, 1u8];
        msg.append(&mut lobby.game.as_ref().unwrap().players.get(&1u8).unwrap().money.to_le_bytes().to_vec());
        state.broadcast_events.push(msg);
        let mut msg = vec![0u8, 2u8, 2u8];
        msg.append(&mut lobby.game.as_ref().unwrap().players.get(&2u8).unwrap().money.to_le_bytes().to_vec());
        state.broadcast_events.push(msg);

        for (&id, &player) in &lobby.game.as_ref().unwrap().players {
            state.private_events.get_mut(&lobby.id_list[id as usize]).unwrap().push(vec![1u8, 1u8, player.private_cards[0].to_byte(), player.private_cards[1].to_byte()]);
        }

        state.broadcast_events.push(vec![1u8, 2u8, lobby.game.as_ref().unwrap().current_turn]);
    }
}

// s to c
/* pub enum Event {
    PlayerAction(u8, PlayerAction), | 1 3 id action (like the ones in c to s) (optional money)
    OwnedMoneyChange(u8, u32),      | 1 4 id money
    NextPlayer(u8),                 | 1 2 id
    UpdateCurrentBet(u32),          | 1 5 money
    UpdatePots(Vec<Pot>),           | 1 6: reset pots
                                    | 1 7 money all bytes of players ids
    RevealFlop([Card; 3]),          | 1 8 card card card
    RevealTurn(Card),               | 1 9 card
    RevealRiver(Card),              | 1 10 card
    Showdown(HashMap<u8, ([Card; 2], HandRank)>), | 1 11 start showdown
                                                  | 1 12 id card card handcategory primarycard secondarycard kickers
                                                  | cards are 255 if None
                                                  | THEN 1 13 showdownstep
} */

fn advance_game(player_action: PlayerAction, lobby: &mut Lobby, state: &mut MutexGuard<'_, GameState>) {
    let events = lobby.game.as_mut().unwrap().advance_game(player_action);
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut reset_player_list = false;
    for event in events {
        match event {
            Event::UpdatePots(pots) => {
                packets.push(vec![1u8, 6u8]);
                for pot in pots {
                    let mut msg = vec![1u8, 7u8];
                    msg.append(&mut pot.money.to_le_bytes().to_vec());
                    for p in pot.eligible_players {
                        msg.push(p);
                    }
                    packets.push(msg);
                }
            },
            Event::Showdown(map) => {
                packets.push(vec![1u8, 11u8]);
                for (id, (cards, hand_rank)) in map {
                    let mut msg = vec![1u8, 12u8, id, cards[0].to_byte(), cards[1].to_byte(), hand_rank.category.to_byte(), option_card_to_byte(hand_rank.primary), option_card_to_byte(hand_rank.secondary)];
                    for kicker in hand_rank.kickers {
                        msg.push(kicker.to_byte());
                    }
                    packets.push(msg);
                }
                packets.append(&mut evaluate_showdown(lobby.game.as_mut().unwrap().evaluate_showdown()));

                // set up for new game
                for (&id, &player) in &lobby.game.as_ref().unwrap().players {
                    lobby.players.get_mut(&lobby.id_list[id as usize]).unwrap().money = player.money;
                }
                for &p in &lobby.queued_for_removal {
                    lobby.players.remove(&lobby.id_list.remove(p as usize));
                    packets.push(vec![0u8, 5u8, p]);
                }
                lobby.game = None;
                lobby.queued_for_removal.clear();
                reset_player_list = true;
            },
            _ => {
                packets.push(match event {
                    Event::PlayerAction(id, action) => {
                        match action {
                            PlayerAction::AddMoney(money) => {
                                let mut msg = vec![1u8, 3u8, id, 1u8];
                                msg.append(&mut money.to_le_bytes().to_vec());
                                msg
                            },
                            PlayerAction::Check => vec![1u8, 3u8, id, 0u8],
                            PlayerAction::Fold => vec![1u8, 3u8, id, 2u8],
                        }
                    },
                    Event::OwnedMoneyChange(id, money) => {
                        let mut msg = vec![1u8, 4u8, id];
                        msg.append(&mut money.to_le_bytes().to_vec());
                        msg
                    },
                    Event::NextPlayer(id) => vec![1u8, 2u8, id],
                    Event::UpdateCurrentBet(money) => {
                        let mut msg = vec![1u8, 5u8];
                        msg.append(&mut money.to_le_bytes().to_vec());
                        msg
                    },
                    Event::RevealFlop(cards) => vec![1u8, 8u8, cards[0].to_byte(), cards[1].to_byte(), cards[2].to_byte()],
                    Event::RevealTurn(card) => vec![1u8, 9u8, card.to_byte()],
                    Event::RevealRiver(card) => vec![1u8, 10u8, card.to_byte()],
                    _ => panic!("this should never occur")
                });
            }
        }
    }
    state.broadcast_events.append(&mut packets);
    if reset_player_list {
        send_player_list_update(lobby, state, None);
    }
}

/* pub struct ShowdownStep {
    pub winners: Vec<u8>, 255 is the list terminator
    pub winnings: u32,
    pub pot_start_index: u8,
    pub pot_end_index: u8,
    pub eligible_players: Vec<u8>, read until 255
    pub win_reason: Option<ShowdownDecidingFactor>, 255 is None
} */
fn evaluate_showdown(showdown_steps: Vec<ShowdownStep>) -> Vec<Vec<u8>> {
    let mut events = Vec::new();
    for step in showdown_steps {
        let mut msg = vec![1u8, 13u8];
        for winner in step.winners {
            msg.push(winner);
        }
        msg.push(255u8);
        msg.append(&mut step.winnings.to_le_bytes().to_vec());
        msg.push(step.pot_start_index);
        msg.push(step.pot_end_index);
        for player in step.eligible_players {
            msg.push(player);
        }
        msg.push(255u8);
        msg.append(&mut match step.win_reason {
            ShowdownDecidingFactor::Category => vec![0u8],
            ShowdownDecidingFactor::Primary(card1, card2) => vec![1u8, card1.to_byte(), card2.to_byte()],
            ShowdownDecidingFactor::Secondary(card1, card2) => vec![2u8, card1.to_byte(), card2.to_byte()],
            ShowdownDecidingFactor::Kicker(card1, card2) => vec![3u8, card1.to_byte(), card2.to_byte()],
            ShowdownDecidingFactor::None => vec![255u8],
        });
        events.push(msg);
    }
    events
}

fn option_card_to_byte(card: Option<Card>) -> u8 {
    match card {
        Some(card) => card.to_byte(),
        None => 255u8,
    }
}

fn find_id_from_client(lobby: &Lobby, client: u64) -> Option<(u8, u64)>{
    lobby.id_list.iter().enumerate().find(|p| *p.1 == client).map(|p| (p.0 as u8, *p.1))
}

fn send_player_list_update(lobby: &Lobby, state: &mut MutexGuard<'_, GameState>, private_id: Option<u64>) {
    let events: &mut Vec<Vec<u8>> = match private_id {
        Some(id) => state.private_events.get_mut(&id).unwrap(),
        None => &mut state.broadcast_events,
    };
    events.push(vec![0u8, 0u8]);
    for network_id in lobby.id_list.iter() {
        let user = lobby.players.get(&network_id).unwrap();
        let mut msg = vec![0u8, 1u8];
        msg.push(user.ready as u8);
        msg.append(&mut user.money.to_le_bytes().to_vec());
        msg.append(&mut user.username.as_bytes().to_vec());
        events.push(msg);
    }
    if private_id.is_none() {
        for (id, network_id) in lobby.id_list.iter().enumerate() {
            state.private_events.get_mut(network_id).unwrap().push(vec![0u8, 4u8, id as u8]);
        }
    }
}

fn handle_client(id: u64, mut stream: TcpStream, shared: SharedState) {
    let mut buf = [0u8; 1024];

    let mut remaining_packet_size = 0;
    let mut received_packet_size = false;
    let mut packet = Vec::<u8>::new();

    let mut broadcast_event_index;
    let mut private_event_index = 0;

    {
        let mut state = shared.lock().unwrap();
        broadcast_event_index = state.broadcast_events.len();
        state.client_events.insert(id, Vec::new());
        state.private_events.insert(id, Vec::new());
    }

    loop {
        {
            let state = shared.lock().unwrap();
            
            let broadcast_events = &state.broadcast_events;
            for event in state.broadcast_events.iter().skip(broadcast_event_index) {
                let mut msg = Vec::<u8>::new();
                msg.push(event.len() as u8);
                msg.append(&mut event.clone());
                match stream.write_all(&msg) {
                    Ok(_) => {},
                    Err(_) => {
                        send_disconnect_message(id, state);
                        return;
                    },
                };
            }
            broadcast_event_index = broadcast_events.len();

            let private_events = &state.private_events.get(&id).unwrap();
            for event in private_events.iter().skip(private_event_index) {
                let mut msg = Vec::<u8>::new();
                msg.push(event.len() as u8);
                msg.append(&mut event.clone());
                match stream.write_all(&msg) {
                    Ok(_) => {},
                    Err(_) => {
                        send_disconnect_message(id, state);
                        return;
                    },
                };
            }
            private_event_index = private_events.len();
        };

        let received_size = match stream.read(&mut buf) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => 0,
            Ok(0) | Err(_) => {
                let state = shared.lock().unwrap();
                send_disconnect_message(id, state);
                return;
            },
            Ok(n) => n,
        };
        if received_size == 0 {
            continue;
        }
        let bytes = &buf[..received_size];
        
        for byte in bytes {
            if !received_packet_size {
                if *byte > 0 {
                    remaining_packet_size = *byte;
                    received_packet_size = true;
                }
            } else {
                packet.push(*byte);
                remaining_packet_size -= 1;
                if remaining_packet_size == 0 {
                    {
                        let mut state = shared.lock().unwrap();
                        state.client_events.get_mut(&id).unwrap().push(packet.clone());
                    }
                    received_packet_size = false;
                    packet.clear();
                }
            }
        }
    }
}

fn send_disconnect_message(id: u64, mut state: MutexGuard<'_, GameState>) {
    state.client_events.get_mut(&id).unwrap().push(vec![0u8, 2u8]);
}
