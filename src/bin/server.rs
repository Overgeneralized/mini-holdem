use std::{collections::{HashMap, HashSet}, net::TcpListener, sync::mpsc::{self, Sender}, thread};

use mini_holdem::{events::{ClientBound, GameEvent, GamePlayerAction, ServerBound}, game::{Game, make_game}, networking::handle_client};

type ClientChannels = HashMap<u64, Sender<ClientBound>>;

struct User {
    money: u32,
    username: String,
    ready: bool,
}

struct Lobby {
    players: HashMap<u64, User>,
    network_to_game: HashMap<u64, u8>,
    game_to_network: Vec<u64>,
    default_money: u32,
    game: Option<Game>,
    queued_for_removal: HashSet<u8>,
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:9194").expect("Couldn't bind to 0.0.0.0:9194.");
    listener.set_nonblocking(true)?;
    println!("Listening on 0.0.0.0 to port 9194.");

    let mut client_channels: HashMap<u64, Sender<ClientBound>> = HashMap::new();

    let (server_bound_sender, server_bound_receiver) = mpsc::channel();

    let mut lobby = Lobby { players: HashMap::new(), network_to_game: HashMap::new(), game_to_network: Vec::new(), default_money: 1000, game: None, queued_for_removal: HashSet::new() };
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
            send_player_list_update(lobby, client_channels, None);
            broadcast_event(client_channels, ClientBound::PlayerJoined(name));
        },
        ServerBound::Leave => {
            client_channels.remove(&client);

            if let Some(player) = lobby.players.get(&client) {
                broadcast_event(client_channels, ClientBound::PlayerLeft(player.username.clone()));
            }

            if let Some(&id) = lobby.network_to_game.get(&client) && let Some(game) = &mut lobby.game {
                lobby.queued_for_removal.insert(id);
                if id == game.current_turn {
                    advance_game(GamePlayerAction::Fold, lobby, client_channels);
                } else {
                    game.player(id).has_folded = true;
                }
            } else {
                send_player_list_update(lobby, client_channels, None);
                check_for_game_start(client_channels, lobby);
                lobby.players.remove(&client);
            }

            lobby.network_to_game.remove(&client);
        },
        ServerBound::Ready(ready) => {
            lobby.players.get_mut(&client).unwrap().ready = ready;
            send_player_list_update(lobby, client_channels, None);

            check_for_game_start(client_channels, lobby);
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
        lobby.game_to_network = vec![0; lobby.players.len()];
        for (game_id, (&network_id, user)) in lobby.players.iter().enumerate() {
            list.push((game_id as u8, user.money));
            lobby.network_to_game.insert(network_id, game_id as u8);
            lobby.game_to_network[game_id] = network_id;
        }

        if let Some(game) = make_game(list) {
            for (id, player) in &game.players {
                let channel = client_channels.get(&lobby.game_to_network[*id as usize]).unwrap();
                let _ = channel.send(ClientBound::GameStarted(player.private_cards));
                let _ = channel.send(ClientBound::YourId(*id));
            }

            broadcast_event(client_channels, ClientBound::GameEvent(GameEvent::OwnedMoneyChange(1, game.player(1).money)));
            broadcast_event(client_channels, ClientBound::GameEvent(GameEvent::OwnedMoneyChange(2, game.player(2).money)));

            broadcast_event(client_channels, ClientBound::GameEvent(GameEvent::UpdateCurrentBet(game.current_bet)));
            broadcast_event(client_channels, ClientBound::GameEvent(GameEvent::UpdatePots(game.compute_pots())));
            broadcast_event(client_channels, ClientBound::GameEvent(GameEvent::NextPlayer(game.current_turn)));
            
            lobby.game = Some(game);
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
            for (&id, &player) in &game.players {
                if let Some(user) = lobby.players.get_mut(&lobby.game_to_network[id as usize]) {
                    user.money = player.money;
                }
            }
            for &id in &lobby.queued_for_removal {
                let username = lobby.players.remove(&lobby.game_to_network[id as usize]).unwrap().username;
                broadcast_event(client_channels, ClientBound::PlayerLeft(username));
            }
            lobby.game = None;
            lobby.queued_for_removal.clear();
            lobby.game_to_network.clear();
            lobby.network_to_game.clear();
            send_player_list_update(lobby, client_channels, None);
        }
    }
}

fn send_player_list_update(lobby: &Lobby, client_channels: &ClientChannels, private_id: Option<u64>) {
    let mut list = Vec::new();
    for (id, user) in &lobby.players {
        if let Some(game) = &lobby.game {
            let player = game.player(*lobby.network_to_game.get(&id).unwrap());
            list.push((false, player.has_folded, player.money, user.username.clone()));
        } else {
            list.push((user.ready, false, user.money, user.username.clone()));
        }
    }

    if let Some(id) = private_id {
        let _ = client_channels.get(&id).unwrap().send(ClientBound::UpdatePlayerList(list));
    } else {
        broadcast_event(client_channels, ClientBound::UpdatePlayerList(list));
    }
}

fn broadcast_event(client_channels: &ClientChannels, event: ClientBound) {
    for channel in client_channels.values() {
        let _ = channel.send(event.clone());
    }
}
