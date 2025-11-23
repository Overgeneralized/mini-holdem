use std::{
    io::{self, ErrorKind, prelude::*}, net::TcpStream, sync::mpsc::{self, Sender}, thread::{self, sleep}, time::Duration
};

struct Player {
    username: String,
    money: u32,
    is_ready: bool,
}

struct ClientData {
    player_list: Vec<Player>,
    last_player_list_size: usize,
    player_id: Option<u8>,
    notifs: Vec<String>,
    conn: TcpStream,

}

fn main() -> std::io::Result<()> {
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
        match TcpStream::connect(buf) {
            Ok(c) => {
                c.set_nonblocking(true)?;
                conn = c;
                break;
            },
            Err(_) => println!("Failed to connect to this ip address.")
        }
    }

    sleep(Duration::from_secs(1));

    // A♠ 7♦ K♣ J♥
    // println!("\x1b[0mJ\x1b[31m♥ \x1b[0m7\x1b[31m♦");
    // println!("\x1b[0mT\x1b[30m♣ \x1b[0mA\x1b[30m♠");
    println!("\x1b[?1049h");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_continuously(tx));

    let mut client_data = ClientData { player_list: Vec::new(), last_player_list_size: 0, player_id: None, notifs: Vec::new(), conn };

    send_packet(&mut client_data.conn, vec![0u8, 4u8])?;

    let mut notif_cooldown = 0; // ms

    let mut remaining_packet_size = 0;
    let mut received_packet_size = false;
    let mut packet = Vec::<u8>::new();
    loop {
        let mut buffer = [0u8; 1024];
        let bytes_read = match client_data.conn.read(&mut buffer[..]) {
            Err(e) if e.kind() == ErrorKind::WouldBlock => 0,
            Err(_) => panic!("real error???"),
            Ok(n) => n,
        };
        let mut update_player_list = false;
        if bytes_read > 0 {
            let bytes = &buffer[..bytes_read];
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
                        if handle_packet(&packet, &mut client_data) {
                            update_player_list = true;
                        }
                        received_packet_size = false;
                        packet.clear();
                    }
                }
            }
        }
        if update_player_list {
            draw_player_list(&mut client_data);
        }
            
        if let Ok(str) = rx.try_recv() {
            print!("\x1b[1A\x1b[2K\x1b[0G"); // clear what the user has entered
            if str.eq("exit") {
                break;
            }
            handle_command(str.split(" ").map(|s| s.to_string()).collect(), &mut client_data)?;
        }

        if notif_cooldown > 0 {
            notif_cooldown -= 1;
        }
        if notif_cooldown == 0 && !client_data.notifs.is_empty() {
            print!("\x1b[1A\x1b[2K{}\x1b[1B\x1b[0G", client_data.notifs.pop().unwrap());
            notif_cooldown = 2000;
        }

        std::io::stdout().flush().unwrap();
        sleep(Duration::from_millis(1));
    }
    println!("\x1b[?1049l");
    Ok(())
}

fn handle_packet(packet: &Vec<u8>, client_data: &mut ClientData) -> bool {
    if packet[0] == 0u8 {
        match packet[1] {
            0u8 => {
                client_data.player_list.clear();
                return true;
            },
            1u8 => {
                let mut username = String::new();
                for byte in &packet[7..] {
                    username.push(*byte as char);
                }
                client_data.player_list.push(Player { username, money: get_money_from_bytes(&packet[3..7]), is_ready: packet[2] != 0u8 });
                return true;
            },
            4u8 => {
                client_data.player_id = Some(packet[2]);
            },
            3u8 => {
                client_data.player_list[packet[2] as usize].is_ready = packet[3] == 1u8;
                return true;
            },
            2u8 => {
                client_data.player_list[packet[2] as usize].money = get_money_from_bytes(&packet[3..7]);
            },
            5u8 => {
                client_data.notifs.push(client_data.player_list[packet[2] as usize].username.to_owned()+&" left the game.".to_owned());
            },
            _ => {}
        }
    }
    return false;
}

fn handle_command(parts: Vec<String>, client_data: &mut ClientData) -> std::io::Result<()> {
    if parts.is_empty() {
        return Ok(());
    }
    if parts[0] == "join" {
        if let Some(username) = parts.get(1) {
            if !username.is_ascii() {
                client_data.notifs.push("Usernames can only contain ASCII characters!".to_string());
                return Ok(());
            }
            if username.contains(" ") {
                client_data.notifs.push("Usernames can't contain spaces!".to_string());
            }
            if username.len() > 16 {
                client_data.notifs.push("Usernames can't have more than 16 characters!".to_string());
                return Ok(());
            }
            if client_data.player_list.iter().any(|p| p.username == *username) {
                client_data.notifs.push("This username is already taken!".to_string());
                return Ok(());
            }
            let mut msg = vec![0u8, 0u8];
            msg.append(&mut username.as_bytes().to_vec());
            send_packet(&mut client_data.conn, msg)?;
        } else {
            client_data.notifs.push("Usage: join <username>".to_string());
        }
    } else if parts[0] == "ready" && !client_data.player_id.is_none() {
        client_data.player_list[client_data.player_id.unwrap() as usize].is_ready = true;
        send_packet(&mut client_data.conn, vec![0u8, 3u8, 1u8])?;
    } else if parts[0] == "notready" && !client_data.player_id.is_none() {
        client_data.player_list[client_data.player_id.unwrap() as usize].is_ready = false;
        send_packet(&mut client_data.conn, vec![0u8, 3u8, 0u8])?;
    } else if parts[0] == "leave" && !client_data.player_id.is_none() {
        client_data.player_id = None;
        send_packet(&mut client_data.conn, vec![0u8, 2u8])?;
    }
    Ok(())
}

fn send_packet(conn: &mut TcpStream, mut packet: Vec<u8>) -> std::io::Result<()> {
    let mut msg = vec![packet.len() as u8];
    msg.append(&mut packet);
    conn.write_all(&msg)?;
    Ok(())
}

fn get_money_from_bytes(bytes: &[u8]) -> u32 {
    let mut money: u32 = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        money |= (byte as u32) << (i * 8);
    }
    money
}

fn draw_player_list(client_data: &mut ClientData) {
    print!("\x1b[{}A", client_data.last_player_list_size + 3);
    for _ in 0..client_data.last_player_list_size+3 {
        print!("\x1b[2K\x1b[1B\x1b[0G");
    }
    if client_data.player_list.is_empty() {
        print!("\x1b[2AThe player list is empty!\x1b[2B\x1b[0G");
        return;
    }
    print!("\x1b[{}Aid |username        |money\x1b[1B\x1b[0G", client_data.player_list.len()+2);
    for (i, player) in client_data.player_list.iter().enumerate() {
        let username_padding = " ".repeat(16 - player.username.len());
        let money_padding = " ".repeat(11-player.money.to_string().len());
        let username_display = if let Some(player_id) = client_data.player_id && player_id == i as u8 {
            &("\x1b[32m".to_owned()+&player.username+&"\x1b[0m")
        } else {
            &player.username
        };
        let ready_display = if player.is_ready {
            "ready!"
        } else {
            ""
        };
        print!("{}.  {}{} ${}{}{}\x1b[1B\x1b[0G", i+1, username_display, username_padding, player.money, money_padding, ready_display);
    }
    client_data.last_player_list_size = client_data.player_list.len();
    print!("\x1b[1B\x1b[0G");
}

fn read_continuously(tx: Sender<String>) {
    loop {
        let mut buf = String::new();
        match io::stdin().read_line(&mut buf) {
            Ok(_) => {},
            Err(e) => panic!("{e}")
        }
        let to_send = buf.trim_end().to_string();
        tx.send(to_send).unwrap();
    }
}
