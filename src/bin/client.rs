use std::{
    io::{self, Result, Write}, net::TcpStream, sync::mpsc::{self, Sender}, thread::{self, sleep}, time::Duration
};

use mini_holdem::{cards::Card, events::{ClientBound, GameEvent, GamePlayerAction, ServerBound}, game::Pot, networking::client_network_loop, protocol::encode_server_bound};

struct Player {
    username: String,
    money: u32,
    is_ready: bool,
    is_folded: bool,
}

struct InGameInfo {
    current_turn: u8,
    current_bet: u32,
    private_cards: [Card; 2],
    public_cards: Vec<Card>,
    pot_data: Vec<Pot>,
}

struct ClientData {
    player_list: Vec<Player>,
    last_player_list_size: usize,
    player_id: Option<u8>,
    notifs: Vec<String>,
    conn: TcpStream,
    in_game_info: Option<InGameInfo>,
}

fn main() -> Result<()> {
    let conn: TcpStream;
    loop {
        println!("Enter the server ip address.");
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        buf = buf.trim_end().to_string();
        if buf.eq("lh") {
            buf = "0.0.0.0".to_string();
        }
        buf.push_str(":9194");
        if let Ok(c) = TcpStream::connect(buf) {
            conn = c;
            break;
        } else {
            println!("Failed to connect to this address.")
        }
    }

    sleep(Duration::from_millis(100));
    
    println!("\x1b[?1049h");
    print!("\x1b[100A");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_continuously(tx));
    
    let mut client_data = ClientData { player_list: Vec::new(), last_player_list_size: 2, player_id: None, notifs: Vec::new(), conn, in_game_info: None };
    
    send_event(&mut client_data.conn, ServerBound::GetPlayerList)?;
    
    let mut notif_cooldown = 0; // ms
    
    let (tx, received_events) = mpsc::channel();
    let mut cloned = client_data.conn.try_clone().expect("Failed to clone stream.");
    thread::spawn(move || client_network_loop(&mut cloned, tx));
    

    let mut do_render = false;
    loop {
        while let Ok(event) = received_events.try_recv() {
            handle_event(event, &mut client_data);
            do_render = true;
        }

        if do_render {
            render(&mut client_data);
        }
        do_render = false;

        if let Ok(str) = rx.try_recv() {
            if str.eq("exit") { break }
            let parts: Vec<String> = str.split(" ").map(|s| s.to_string()).collect();
            if parts.len() >= 1 {
                let cmd = &parts[0];
                let args = &parts.get(1..).unwrap_or(&[]).to_vec();
                handle_command(cmd.to_string(), args.to_vec(), &mut client_data)?;
            }
        }

        if notif_cooldown > 0 {
            notif_cooldown -= 1;
        }
        if notif_cooldown == 0 && !client_data.notifs.is_empty() {
            print!("\x1b[2A\x1b[2K{}\x1b[2B\x1b[0G", client_data.notifs.pop().unwrap());
            notif_cooldown = 2000;
        }

        std::io::stdout().flush().unwrap();
        sleep(Duration::from_millis(1));
    }
    println!("\x1b[?1049l");
    Ok(())
}

fn handle_event(event: ClientBound, client_data: &mut ClientData) {
    match event {
        ClientBound::UpdatePlayerList(players) => {
            client_data.player_list.clear();
            for (is_ready, is_folded, money, username) in players {
                client_data.player_list.push(Player { username, money, is_ready, is_folded });
            }
        },
        ClientBound::YourId(id) => client_data.player_id = Some(id),
        ClientBound::PlayerLeft(player) => client_data.notifs.push(player.to_owned()+&" left the game.".to_owned()),
        ClientBound::PlayerJoined(player) => client_data.notifs.push(player.to_owned()+&" joined the game.".to_owned()),
        ClientBound::GameStarted(cards) => {
            for player in client_data.player_list.iter_mut() {
                player.is_ready = false;
            }
            client_data.in_game_info = Some(InGameInfo { current_turn: 0, current_bet: 0, private_cards: cards, public_cards: Vec::new(), pot_data: Vec::new() });
        },
        ClientBound::GameEvent(game_event) => {
            if let Some(game_info) = client_data.in_game_info.as_mut() {
                match game_event {
                    GameEvent::NextPlayer(player) => game_info.current_turn = player,
                    GameEvent::OwnedMoneyChange(player, money) => client_data.player_list[player as usize].money = money,
                    GameEvent::PlayerAction(player, action) => {
                        let username = &client_data.player_list[player as usize].username;
                        match action {
                            GamePlayerAction::Check => client_data.notifs.push(username.to_owned()+" checked."),
                            GamePlayerAction::AddMoney(money) => client_data.notifs.push(username.to_owned()+" added "+&money.to_string()),
                            GamePlayerAction::Fold => {
                                client_data.notifs.push(username.to_owned()+" folded.");
                                client_data.player_list[player as usize].is_folded = true;
                            }
                        }
                    },
                    GameEvent::UpdateCurrentBet(money) => game_info.current_bet = money,
                    GameEvent::UpdatePots(pots) => {
                        game_info.pot_data.clear();
                        for pot in pots {
                            game_info.pot_data.push(pot);
                        }
                    },
                    GameEvent::RevealFlop(cards) => game_info.public_cards.extend(cards),
                    GameEvent::RevealTurn(card) | GameEvent::RevealRiver(card) => game_info.public_cards.push(card),
                    GameEvent::Showdown(_map) => todo!(),
                    GameEvent::ShowdownSteps(_steps) => todo!()
                }
            }
        }
    }
}

fn handle_command(cmd: String, args: Vec<String>, client_data: &mut ClientData) -> Result<()> {
    match cmd.as_str() {
        "join" => {
            if let Some(username) = args.get(0) {
                if username == "" {
                    return Ok(());
                }
                if !username.is_ascii() {
                    client_data.notifs.push("Usernames can only contain ASCII characters!".to_string());
                    return Ok(());
                }
                if username.len() < 3 {
                    client_data.notifs.push("Usernames have to have at least 3 characters!".to_string());
                    return Ok(());
                }
                if username.len() > 16 {
                    client_data.notifs.push("Usernames can't have more than 16 characters!".to_string());
                    return Ok(());
                }
                if client_data.player_list.iter().any(|p| p.username == *username) {
                    client_data.notifs.push("This username is already taken!".to_string());
                    return Ok(());
                }
                send_event(&mut client_data.conn, ServerBound::Login(username.to_string()))?;
            } else {
                client_data.notifs.push("Usage: join <username>".to_string());
            }
        }
        "ready" => send_event(&mut client_data.conn, ServerBound::Ready(true))?,
        "notready" => send_event(&mut client_data.conn, ServerBound::Ready(false))?,
        "check" => send_event(&mut client_data.conn, ServerBound::GameAction(GamePlayerAction::Check))?,
        "addmoney" => {
            if args.len() == 1 && let Ok(money) = args[0].parse::<u32>() {
                send_event(&mut client_data.conn, ServerBound::GameAction(GamePlayerAction::AddMoney(money)))?;
            }
        },
        "fold" => send_event(&mut client_data.conn, ServerBound::GameAction(GamePlayerAction::Fold))?,
        _ => {}
    };
    Ok(())
}

fn render(client_data: &mut ClientData) {
    for _ in 0..client_data.last_player_list_size {
        print!("\x1b[1A\x1b[0G\x1b[2K");
    }

    if let Some(game_info) = &client_data.in_game_info {
        for (i, pot) in game_info.pot_data.iter().enumerate() {
            print!("Pot {}: ${} ({})\x1b[1B\x1b[0G", i+1, pot.money, if pot.eligible_players.contains(&client_data.player_id.unwrap()) {
                "eligible"
            } else {
                "not eligible"
            });
        }

        print!("\x1b[1B\x1b[0G");
        
        let mut public_cards_display = String::new();
        for card in &game_info.public_cards {
            public_cards_display.push_str(&card.to_string());
            public_cards_display.push(' ');
        }
        if game_info.public_cards.is_empty() {
            public_cards_display = String::from("No cards yet");
        }
        print!("Public cards: {}\x1b[1B\x1b[0G", public_cards_display);
        print!("Private cards: {} {}\x1b[1B\x1b[0G", game_info.private_cards[0], game_info.private_cards[1]);

        print!("\x1b[1B\x1b[0G");
    }

    if client_data.player_list.is_empty() {
        print!("The player list is empty!\x1b[1B\x1b[0G");
    } else {
        print!("id |username        |money\x1b[1B\x1b[0G");
    }
    
    for (i, player) in client_data.player_list.iter().enumerate() {
        let username_padding = " ".repeat(16 - player.username.len());
        let money_padding = " ".repeat(11-player.money.to_string().len());
        let username_display = if let Some(player_id) = client_data.player_id && player_id == i as u8 {
            &("\x1b[32m".to_owned()+&player.username+&"\x1b[0m")
        } else {
            &player.username
        };
        let extra = if player.is_ready {
            "ready!"
        } else if player.is_folded {
            "folded"
        } else if let Some(game_info) = &client_data.in_game_info && game_info.current_turn == i as u8 {
            "current turn"
        } else {
            ""
        };
        print!("{}.  {}{} ${}{}{}\x1b[1B\x1b[0G", i+1, username_display, username_padding, player.money, money_padding, extra);
    }
    print!("\x1b[3B\x1b[0G");

    client_data.last_player_list_size = 4;
    client_data.last_player_list_size += client_data.player_list.len();
    if let Some(game_info) = &client_data.in_game_info {
        client_data.last_player_list_size += 4;
        client_data.last_player_list_size += game_info.pot_data.len();
    }
}

fn send_event(conn: &mut TcpStream, event: ServerBound) -> Result<()> {
    let mut packet = encode_server_bound(event);
    let mut msg = vec![packet.len() as u8];
    msg.append(&mut packet);
    conn.write_all(&msg)?;
    Ok(())
}

fn read_continuously(tx: Sender<String>) {
    loop {
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).expect("Failed to read input line.");
        print!("\x1b[1A\x1b[2K\x1b[0G"); // clear what the user has entered
        tx.send(buf.trim_end().to_string()).expect("Failed to send read input line to command parser.");
    }
}
