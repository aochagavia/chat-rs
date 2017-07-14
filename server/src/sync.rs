use std::{io, thread};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use protocol::sync;

pub fn run() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();

    let clients = Arc::new(Mutex::new(Vec::new()));

    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut clients_lock = clients.lock().expect("Failed to acquire lock");
        let nickname = sync::read_message(&mut stream).expect("Failed to read nickname");
        clients_lock.push(stream.try_clone()?);

        // Dummy logging
        println!("{} logged in", nickname);

        // Spawn a thread to receive messages from this client
        let client_index = clients_lock.len() - 1;
        let clients = clients.clone();
        thread::spawn(move || {
            loop {
                // Receive messages from this client
                let msg = sync::read_message(&mut stream).expect("Failed to read message");

                // Append the nickname
                let msg = format!("{}: {}", nickname, msg);

                // Broadcast the message to all clients
                for (i, client) in clients.lock().expect("Failed to acquire lock").iter_mut().enumerate() {
                    // Don't broadcast to the client that sent the message
                    if i != client_index {
                        sync::write_message(&mut *client, &msg).expect("Failed to broadcast message");
                    }
                }
            }
        });
    }

    Ok(())
}
