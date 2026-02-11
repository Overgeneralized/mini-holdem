use std::{io::{Read, Write, Result}, net::TcpStream, sync::mpsc::{Receiver, Sender}, thread};

use crate::{events::{ClientBound, ServerBound}, protocol::{decode_client_bound, decode_server_bound, encode_client_bound, encode_server_bound}};

pub fn client_network_loop(stream: &mut TcpStream, tx: Sender<ClientBound>) {
    let mut remaining_packet_size = 0;
    let mut packet_size_received = false;
    let mut packet = Vec::<u8>::new();
    loop {
        let mut buffer = [0u8; 1024];
        let bytes_read = match stream.read(&mut buffer[..]) {
            Err(_) => panic!("real error???"),
            Ok(0) => panic!("another error???"), // peer disconnected
            Ok(n) => n,
        };

        let mut slice = &buffer[..bytes_read];

        while !slice.is_empty() {
            if !packet_size_received {
                let size = slice[0];
                slice = &slice[1..];

                if size > 0 {
                    remaining_packet_size = size as usize;
                    packet_size_received = true;
                    packet.clear();
                }
            } else {
                let to_take = remaining_packet_size.min(slice.len());
                packet.extend_from_slice(&slice[..to_take]);

                slice = &slice[to_take..];
                remaining_packet_size -= to_take;

                if remaining_packet_size == 0 {
                    if let Some(event) = decode_client_bound(&packet) {
                        tx.send(event).expect("Networking failed to send message to client.");
                    }
                    packet_size_received = false;
                }
            }
        }
    }
}

pub fn handle_client(id: u64, mut stream: TcpStream, client_bound_receiver: Receiver<ClientBound>, server_bound_sender: Sender<(u64, ServerBound)>) -> core::result::Result<(), Box<dyn std::error::Error>> {
    stream.set_nonblocking(true)?;

    let mut buf = [0u8; 1024];

    let mut remaining_packet_size = 0;
    let mut received_packet_size = false;
    let mut packet = Vec::<u8>::new();

    loop {
        let received_size = match stream.read(&mut buf) {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {0},
            Ok(0) | Err(_) => {
                server_bound_sender.send((id, ServerBound::Disconnect))?;
                return Ok(());
            },
            Ok(n) => n,
        };
        if received_size != 0 {
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
                        if let Some(event) = decode_server_bound(&packet) {
                            server_bound_sender.send((id, event.clone()))?;
                            if matches!(event, ServerBound::Disconnect) {
                                return Ok(())
                            }
                        }
                        received_packet_size = false;
                        packet.clear();
                    }
                }
            }
        }

        for event in client_bound_receiver.try_iter() {
            let mut packet = encode_client_bound(event);
            let mut msg = vec![packet.len() as u8];
            msg.append(&mut packet);
            if let Err(_) = stream.write_all(&msg) {
                server_bound_sender.send((id, ServerBound::Disconnect))?;
                return Ok(());
            }
        }

        thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub fn send_event(conn: &mut TcpStream, event: ServerBound) -> Result<()> {
    let mut packet = encode_server_bound(event);
    let mut msg = vec![packet.len() as u8];
    msg.append(&mut packet);
    conn.write_all(&msg)?;
    Ok(())
}
