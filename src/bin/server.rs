use std::{collections::{HashMap, HashSet}, net::{SocketAddr, TcpListener}, sync::mpsc::{self, Sender}, thread};

use mini_holdem::{events::{ClientBound, GameEvent, GamePlayerAction, PlayerState, ServerBound}, game::{Game, make_game}, networking::handle_client};

type ClientChannels = HashMap<u64, Sender<ClientBound>>;

struct User {
    money: u32,
    username: String,
    ready: bool,
}

struct Lobby {
    players: HashMap<u64, User>,
    player_order: Vec<u64>,
    network_to_game: HashMap<u64, u8>,
    default_money: u32,
    game: Option<Game>,
    queued_for_removal: HashSet<u8>,
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 9194))).expect("Couldn't bind to 0.0.0.0:9194.");
    listener.set_nonblocking(true)?;
    println!("Bound to 0.0.0.0 with port 9194.");

    let mut client_channels: HashMap<u64, Sender<ClientBound>> = HashMap::new();

    let (server_bound_sender, server_bound_receiver) = mpsc::channel();

    let mut lobby = Lobby { players: HashMap::new(), player_order: Vec::new(), network_to_game: HashMap::new(), default_money: 1000, game: None, queued_for_removal: HashSet::new() };
    let mut next_id: u64 = 0;

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let id = next_id;
                next_id += 1;
                let (tx, rx) = mpsc::channel();
                client_channels.insert(id, tx.clone());
                let cloned = server_bound_sender.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_client(id, stream, rx, cloned) {
                        println!("Error handling client id {}: {}", id, e);
                    }
                });
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {},
            Err(e) => return Err(e),
        }

        for (client_id, event) in server_bound_receiver.try_iter() {
            handle_event(event, client_id, &mut lobby, &mut client_channels);
        }

        thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn handle_event(event: ServerBound, client: u64, lobby: &mut Lobby, client_channels: &mut ClientChannels) {
    match event {
        ServerBound::Login(name) => {
            if !name.is_ascii() || name.len() > 16 || name.len() < 3 || name.contains(" ") || lobby.players.values().any(|n| n.username.eq_ignore_ascii_case(&name)) {
                return;
            }
            lobby.players.insert(client, User { money: lobby.default_money, username: name.clone(), ready: false });
            lobby.player_order.push(client);
            send_player_list_update(lobby, client_channels, None);
            broadcast_event(client_channels, ClientBound::PlayerJoined(name));
        },
        ServerBound::Disconnect => {
            client_channels.remove(&client);

            if let Some(player) = lobby.players.get(&client) {
                broadcast_event(client_channels, ClientBound::PlayerLeft(player.username.clone()));
            }

            if let Some(&id) = lobby.network_to_game.get(&client) && let Some(game) = &mut lobby.game {
                lobby.queued_for_removal.insert(id);
                broadcast_event(client_channels, ClientBound::GameEvent(GameEvent::InGamePlayerLeave(id)));
                if id == game.current_turn {
                    advance_game(GamePlayerAction::Fold, lobby, client_channels);
                } else {
                    (*game.player_mut(id)).has_folded = true;
                }
            } else {
                lobby.players.remove(&client);
                lobby.player_order.retain(|&p| p != client);
                send_player_list_update(lobby, client_channels, None);
                check_for_game_start(client_channels, lobby);
            }

            lobby.network_to_game.remove(&client);
        },
        ServerBound::Ready(ready) => {
            if let Some(user) = lobby.players.get_mut(&client) {
                user.ready = ready;
                send_player_list_update(lobby, client_channels, None);
                check_for_game_start(client_channels, lobby);
            }

        },
        ServerBound::GameAction(action) => {
            if let Some(game) = lobby.game.as_ref() && let Some(&id) = lobby.network_to_game.get(&client) && game.current_turn == id {
                advance_game(action, lobby, client_channels);
            }
        },
        ServerBound::GetPlayerList => {
            send_player_list_update(lobby, client_channels, Some(client));
        }
    }
}

fn check_for_game_start(client_channels: &ClientChannels, lobby: &mut Lobby) {
    if lobby.players.iter().all(|(_, user)| user.ready) && lobby.players.len() >= 3 {
        let mut list = Vec::new();
        for (game_id, &network_id) in lobby.player_order.iter().enumerate() {
            let player = lobby.players.get(&network_id).unwrap();
            list.push(player.money);
            lobby.network_to_game.insert(network_id, game_id as u8);
        }

        if let Some(game) = make_game(list) {
            for (id, player) in game.players.iter().enumerate() {
                let _ = client_channels.get(&lobby.player_order[id]).unwrap().send(ClientBound::GameStarted(player.private_cards));
            }
            
            lobby.game = Some(game);

            // big blind and small blind forced
            advance_game(GamePlayerAction::AddMoney(5), lobby, client_channels);
            advance_game(GamePlayerAction::AddMoney(10), lobby, client_channels);
        }
    }
}

fn advance_game(player_action: GamePlayerAction, lobby: &mut Lobby, client_channels: &ClientChannels) {
    if let Some(game) = lobby.game.as_mut() && let Some(events) = game.advance_game(player_action) {
        for event in &events {
            broadcast_event(client_channels, ClientBound::GameEvent(event.clone()));
        }

        if events.iter().any(|e| matches!(e, GameEvent::Showdown(_))) {
            // cleanup
            for &id in &lobby.queued_for_removal {
                let newtork_id = lobby.player_order[id as usize];
                let username = lobby.players.remove(&newtork_id).unwrap().username;
                broadcast_event(client_channels, ClientBound::PlayerLeft(username));
                lobby.player_order.retain(|c| *c != newtork_id);
            }
            for (id, &player) in game.players.iter().enumerate() {
                if let Some(network_id) = lobby.player_order.get(id) && let Some(user) = lobby.players.get_mut(&*network_id) {
                    user.money = player.money;
                }
            }
            for (_, user) in &mut lobby.players {
                user.ready = false;
            }
            lobby.game = None;
            lobby.queued_for_removal.clear();
            lobby.network_to_game.clear();
            send_player_list_update(lobby, client_channels, None);
        }
    }
}

fn send_player_list_update(lobby: &Lobby, client_channels: &ClientChannels, private_id: Option<u64>) {
    let mut list = Vec::new();
    for network_id in &lobby.player_order {
        let user = lobby.players.get(network_id).unwrap();
        if let Some(game) = &lobby.game {
            let player = game.player(*lobby.network_to_game.get(network_id).unwrap());
            list.push((if lobby.queued_for_removal.contains(&player.id) { PlayerState::Left } else if player.has_folded { PlayerState::Folded } else { PlayerState::InGame }, player.money, user.username.clone()));
        } else {
            list.push((if user.ready { PlayerState::Ready } else { PlayerState::NotReady }, user.money, user.username.clone()));
        }
    }

    if let Some(id) = private_id {
        let _ = client_channels.get(&id).unwrap().send(ClientBound::UpdatePlayerList(list));
    } else {
        broadcast_event(client_channels, ClientBound::UpdatePlayerList(list));
        for (index, network_id) in lobby.player_order.iter().enumerate() {
            if let Some(channel) = client_channels.get(network_id) {
                let _ = channel.send(ClientBound::YourIndex(index as u8));
            }
        }
    }
}

fn broadcast_event(client_channels: &ClientChannels, event: ClientBound) {
    for channel in client_channels.values() {
        let _ = channel.send(event.clone());
    }
}
