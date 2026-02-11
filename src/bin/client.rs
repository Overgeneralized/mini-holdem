use std::{
    io::{self, Result}, net::{IpAddr, SocketAddr, TcpStream}, str::FromStr, sync::mpsc::{self, Sender}, thread::{self, sleep}, time::Duration
};

use crossterm::{cursor::{MoveDown, MoveLeft, MoveRight, MoveUp}, event::{self, Event, KeyCode, KeyEvent, KeyEventKind}, execute, terminal::{self, Clear, ClearType, DisableLineWrap, EnableLineWrap, disable_raw_mode, enable_raw_mode}};
use mini_holdem::{cards::{Card, format_cards}, events::{ClientBound, GameEvent, GamePlayerAction, PlayerState, ServerBound, ShowdownInfo}, game::Pot, networking::{client_network_loop, send_event}};

struct Player {
    username: String,
    money: u32,
    player_state: PlayerState
}

struct InGameInfo {
    current_turn: u8,
    current_bet: u32,
    private_cards: [Card; 2],
    public_cards: Vec<Card>,
    pot_data: Vec<Pot>,
}

#[derive(Debug)]
enum DisplayMode {
    PlayerList,
    ShowdownHandRanks((Vec<String>, ShowdownInfo)),
    ShowdownSteps((Vec<String>, ShowdownInfo, usize))
}

struct ClientData {
    player_list: Vec<Player>,
    player_index: Option<u8>,
    notifs: Vec<String>,
    conn: TcpStream,
    in_game_info: Option<InGameInfo>,
    display_mode: DisplayMode
}

fn main() -> Result<()> {
    let conn: TcpStream;
    loop {
        println!("Enter the server ip address.");
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        buf = buf.trim_end().to_string();
        let conn_attempt;
        if buf.eq("lh") {
            conn_attempt = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], 9194)), Duration::from_secs(5));
        } else if let Ok(addr) = IpAddr::from_str(&buf) {
            conn_attempt = TcpStream::connect_timeout(&SocketAddr::new(addr, 9194), Duration::from_secs(5));
        } else {
            println!("Invalid IP address.");
            continue;
        }
        if let Ok(c) = conn_attempt {
            conn = c;
            break;
        } else {
            println!("Failed to connect to this address.")
        }
    }

    sleep(Duration::from_millis(100));
    
    enable_raw_mode()?;
    execute!(io::stdout(), Clear(ClearType::All))?;
    execute!(io::stdout(), DisableLineWrap)?;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_continuously(tx));
    
    let mut client_data = ClientData { player_list: Vec::new(), player_index: None, notifs: Vec::new(), conn, in_game_info: None, display_mode: DisplayMode::PlayerList };
    
    let mut notif_cooldown = 0; // ms
    
    let (tx, received_events) = mpsc::channel();
    let mut cloned = client_data.conn.try_clone().expect("Failed to clone stream.");
    thread::spawn(move || client_network_loop(&mut cloned, tx));
    
    send_event(&mut client_data.conn, ServerBound::GetPlayerList)?;

    let mut line = String::new();
    let mut last_notif = String::new();
    let mut do_render = false;
    loop {
        while let Ok(event) = received_events.try_recv() {
            handle_event(event, &mut client_data);
            do_render = true;
        }

        if let Ok(key) = rx.try_recv() {
            if matches!(key, KeyCode::Esc) {
                break;
            }
            if handle_key(key, &mut line, &mut client_data)? {
                do_render = true;
            }
        }

        if do_render {
            render(&mut client_data, &line, &last_notif)?;
            do_render = false;
        }


        if notif_cooldown > 0 {
            notif_cooldown -= 1;
        }
        if notif_cooldown == 0 && let Some(notif) = client_data.notifs.pop() {
            last_notif = notif.clone();
            execute!(io::stdout(), MoveUp(2), Clear(ClearType::CurrentLine))?;
            if line.len() != 0 { execute!(io::stdout(), MoveLeft(line.len() as u16))? }
            print!("{}", notif);
            execute!(io::stdout(), MoveDown(2), MoveLeft(notif.len() as u16))?;
            if line.len() != 0 { execute!(io::stdout(), MoveRight(line.len() as u16))? }
            notif_cooldown = 2000;
        }

        sleep(Duration::from_millis(1));
    }

    disable_raw_mode()?;
    execute!(io::stdout(), EnableLineWrap)?;
    Ok(())
}

fn handle_event(event: ClientBound, client_data: &mut ClientData) {
    match event {
        ClientBound::UpdatePlayerList(players) => {
            client_data.player_list.clear();
            for (player_state, money, username) in players {
                client_data.player_list.push(Player { username, money, player_state });
            }
        },
        ClientBound::YourIndex(idx) => client_data.player_index = Some(idx),
        ClientBound::PlayerLeft(player) => client_data.notifs.push(player+" left the game."),
        ClientBound::PlayerJoined(player) => client_data.notifs.push(player+" joined the game."),
        ClientBound::GameStarted(cards) => {
            for player in client_data.player_list.iter_mut() {
                player.player_state = PlayerState::InGame;
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
                            GamePlayerAction::Check => client_data.notifs.push(username.clone()+" checked."),
                            GamePlayerAction::AddMoney(money) => client_data.notifs.push(username.clone()+" added "+&money.to_string()),
                            GamePlayerAction::Fold => {
                                client_data.notifs.push(username.to_owned()+" folded.");
                                client_data.player_list[player as usize].player_state = PlayerState::Folded;
                            }
                        }
                    },
                    GameEvent::InGamePlayerLeave(player) => client_data.player_list[player as usize].player_state = PlayerState::Left,
                    GameEvent::UpdateCurrentBet(money) => game_info.current_bet = money,
                    GameEvent::UpdatePots(pots) => {
                        game_info.pot_data.clear();
                        for pot in pots {
                            game_info.pot_data.push(pot);
                        }
                    },
                    GameEvent::RevealFlop(cards) => game_info.public_cards.extend(cards),
                    GameEvent::RevealTurn(card) | GameEvent::RevealRiver(card) => game_info.public_cards.push(card),
                    GameEvent::Showdown(info) => {
                        client_data.display_mode = DisplayMode::ShowdownHandRanks((client_data.player_list.iter().map(|p| p.username.clone()).collect(), info))
                    }
                }
            }
        }
    }
}

fn handle_command(cmd: String, args: Vec<String>, client_data: &mut ClientData) -> Result<bool> {
    match cmd.as_str() {
        "join" => {
            if let Some(username) = args.get(0) {
                if username == "" {
                    return Ok(false);
                }
                if !username.is_ascii() {
                    client_data.notifs.push("Usernames can only contain ASCII characters!".to_string());
                    return Ok(false);
                }
                if username.len() < 3 {
                    client_data.notifs.push("Usernames have to have at least 3 characters!".to_string());
                    return Ok(false);
                }
                if username.len() > 16 {
                    client_data.notifs.push("Usernames can't have more than 16 characters!".to_string());
                    return Ok(false);
                }
                if client_data.player_list.iter().any(|p| p.username == *username) {
                    client_data.notifs.push("This username is already taken!".to_string());
                    return Ok(false);
                }
                send_event(&mut client_data.conn, ServerBound::Login(username.clone()))?;
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
        "next" => {
            if let DisplayMode::ShowdownSteps((players, info, idx)) = &client_data.display_mode {
                client_data.display_mode = DisplayMode::ShowdownSteps((players.clone(), info.clone(), idx + 1))
            }
            if let DisplayMode::ShowdownHandRanks((players, info)) = &client_data.display_mode {
                client_data.display_mode = DisplayMode::ShowdownSteps((players.clone(), info.clone(), 0));
            }
            if let DisplayMode::ShowdownSteps((_, (_, steps), idx)) = &client_data.display_mode && steps.len() == *idx {
                client_data.display_mode = DisplayMode::PlayerList;
                client_data.in_game_info = None;
            }
        }
        _ => return Ok(false)
    };
    Ok(true)
}

fn render(client_data: &ClientData, line: &String, notif: &String) -> Result<()> {
    execute!(io::stdout(), Clear(ClearType::All), MoveLeft(line.len() as u16))?;

    if let Some(game_info) = &client_data.in_game_info {
        for (i, pot) in game_info.pot_data.iter().enumerate() {
            let eligibility = if let Some(id) = client_data.player_index {
                if pot.eligible_players.contains(&id) {
                    "(eligible)"
                } else {
                    "(not eligible)"
                }
            } else {
                ""
            };
            println!("Pot {}: ${} {}\r", i+1, pot.money, eligibility);
        }

        print!("\nCurrent bet: {}\r\n\n", game_info.current_bet);
        
        let public_cards_display = if game_info.public_cards.is_empty() {
            String::from("No cards yet")
        } else {
            format_cards(&game_info.public_cards)
        };
        println!("Public cards: {}\r", public_cards_display);
        println!("Private cards: {} {}\r\n", game_info.private_cards[0], game_info.private_cards[1]);
    }

    if let DisplayMode::ShowdownHandRanks((players, (hand_ranks, _))) = &client_data.display_mode {
        print!("SHOWDOWN!\r\n\n");
        for (i, player) in players.iter().enumerate() {
            if let Some(hand_rank) = hand_ranks.get(i) {
                println!("{}{}: {} | {}     {}\r", player, " ".repeat(16-player.len()), format_cards(&hand_rank.0), format_cards(&hand_rank.1), hand_rank.2.to_string());
            }
        }
        print!("\nUse the command \"next\" to go to showdown steps.\r\n\n");
    }

    if let DisplayMode::ShowdownSteps((players, (_hand_ranks, steps), idx)) = &client_data.display_mode {
        print!("Showdown step {} of {}\r\n\n", idx+1, steps.len());
        let step = &steps[*idx];
        if step.pot_start_index == step.pot_start_index {
            print!("Fighting for pot {} worth {} money\r\n\n", step.pot_start_index+1, step.winnings);
        } else {
            print!("This step was for pots from {} to {} worth {} money in total\r\n\n", step.pot_start_index+1, step.pot_end_index+1, step.winnings);
        }
        if step.eligible_players.len() == 0 || step.winners.len() == 0 { 
            // do nothing, illegal state
        } else if step.eligible_players.len() == 1 {
            if let Some(name) = players.get(step.eligible_players[0] as usize) {
                print!("There was only one eligible player for these winnings: {}\r\n\n", name);
            }
        } else {
            if step.winners.len() == step.eligible_players.len() {
                println!("All {} players who were eligible for these winnings have tied", step.eligible_players.len());
            } else {
                let mut username_list = Vec::new();
                for winner in &step.winners {
                    if let Some(username) = players.get(*winner as usize) {
                        username_list.push(username.clone());
                    }
                }
                if username_list.len() == 1 {
                    print!("Out of the {} eligible players for these winnings, {} won all the money\r\n\n", step.eligible_players.len(), username_list[0]);
                } else {
                    print!("There were {} eligible players for these winnings, and {} of them have tied to receive a split amount: {}\r\n\n", step.eligible_players.len(), username_list.len(), username_list.join(", "))
                }
                // this is quite a bit of work that i realized may not be that groundbreaking
                // gonna leave this here for now
                //
                // if let Some(winner_name) = players.get(step.winners[0] as usize) && let Some(reason) = &step.win_reason && let Some(compared_name) = players.get(reason.1 as usize) && let Some(winner_hand_rank) = hand_ranks.get(step.winners[0] as usize) && let Some(compared_hand_rank) = hand_ranks.get(reason.1 as usize) {
                //     println!("Comparing the hand of {} with the hand of a player who hasn't won this round: {}\r", winner_name, compared_name);
                //     println!("{}{}: {} | {}     {}\r", winner_name, " ".repeat(16-winner_name.len()), format_cards(&winner_hand_rank.0), format_cards(&winner_hand_rank.1), winner_hand_rank.2.to_string());
                //     println!("{}{}: {} | {}     {}\r", compared_name, " ".repeat(16-compared_name.len()), format_cards(&compared_hand_rank.0), format_cards(&compared_hand_rank.1), compared_hand_rank.2.to_string());
                //     match reason.0 {
                //         ShowdownDecidingFactor::Category =>
                //         ShowdownDecidingFactor::Primary((), ())
                //     }
                // }
            }
        }
        if idx - 1 != steps.len() {
            print!("\nUse the command \"next\" to view the next showdown step.\r\n\n");
        } else {
            print!("\nUse the command \"next\" to exit viewing the showdown steps.\r\n\n");
        }
    }

    if client_data.player_list.is_empty() {
        println!("The player list is empty!\r");
    } else {
        println!("id |username        |money\r");
    }
    
    for (i, player) in client_data.player_list.iter().enumerate() {
        let username_padding = " ".repeat(16 - player.username.len());
        let money_padding = " ".repeat(11-player.money.to_string().len());
        let username_display = if let Some(index) = client_data.player_index && index == i as u8 {
            &("\x1b[32m".to_owned()+&player.username+&"\x1b[0m")
        } else {
            &player.username
        };
        let extra = if matches!(player.player_state, PlayerState::Ready) {
            "ready!"
        } else if matches!(player.player_state, PlayerState::Folded) {
            "folded"
        } else if matches!(player.player_state, PlayerState::Left) {
            "left"
        } else if let Some(game_info) = &client_data.in_game_info && game_info.current_turn == i as u8 {
            "current turn"
        } else {
            ""
        };
        println!("{}.  {}{} ${}{}{}\r", i+1, username_display, username_padding, player.money, money_padding, extra);
    }

    print!("\n");
    println!("{}\r", notif);
    print!("\n");
    print!("{}", line);
    execute!(io::stdout())?;
    Ok(())
}

fn handle_key(key: KeyCode, line: &mut String, client_data: &mut ClientData) -> Result<bool> {
    match key {
        KeyCode::Char(c) => {
            line.push(c);
            print!("{}", c);
            execute!(io::stdout())?;
        },
        KeyCode::Backspace => {
            execute!(io::stdout(), Clear(terminal::ClearType::CurrentLine), MoveLeft(line.len() as u16))?;
            line.pop();
            print!("{}", line);
            execute!(io::stdout())?;
        },
        KeyCode::Enter => {
            let parts: Vec<String> = line.split(" ").map(|s| s.to_string()).collect();
            execute!(io::stdout(), Clear(terminal::ClearType::CurrentLine), MoveLeft(line.len() as u16))?;
            line.clear();
            if parts.len() >= 1 {
                let cmd = &parts[0];
                let args = &parts.get(1..).unwrap_or(&[]).to_vec();
                return Ok(handle_command(cmd.to_string(), args.to_vec(), client_data)?);
            }
        },
        _ => {}
    }
    Ok(false)
}

fn read_continuously(tx: Sender<KeyCode>) {
    loop {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event::read().expect("Failed to ready current input event.") {
            tx.send(code).expect("Failed to send key code to main loop.");
        }
    }
}
