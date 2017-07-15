use std::{io, thread};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use protocol::sync;

pub fn run() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    let broadcaster = Arc::new(Broadcaster::default());

    for stream in listener.incoming() {
        let mut stream = stream?;
        let nickname = sync::read_message(&mut stream).expect("Failed to read nickname");
        let client_index = broadcaster.subscribe(&stream).expect("Unable to subscribe to broadcaster");

        broadcaster.broadcast(client_index, &format!("{} logged in", nickname));

        // Spawn a thread to receive messages from this client
        let broadcaster = broadcaster.clone();
        thread::spawn(move || {
            loop {
                // Receive and broadcast messages from this client
                let msg = sync::read_message(&mut stream).expect("Failed to read message");
                let msg = format!("{}: {}", nickname, msg);
                broadcaster.broadcast(client_index, &msg);
            }
        });
    }

    Ok(())
}

#[derive(Default)]
struct Broadcaster {
    next_client_id: AtomicUsize,
    subscribers: Mutex<HashMap<usize, TcpStream>>
}

impl Broadcaster {
    pub fn subscribe(&self, stream: &TcpStream) -> io::Result<usize> {
        let id = self.next_client_id.fetch_add(1, Ordering::SeqCst);
        let mut subscribers = self.subscribers.lock().expect("Failed to acquire lock");
        subscribers.insert(id, stream.try_clone()?);
        Ok(id)
    }

    pub fn broadcast(&self, ignore_id: usize, msg: &str) {
        println!("[message] {}", msg);

        let mut closed_connections = Vec::new();

        // Broadcast the message to all subscribers
        let mut subscribers = self.subscribers.lock().expect("Failed to acquire lock");
        for (&id, client) in subscribers.iter_mut() {
            // Don't broadcast to the client that sent the message
            if id != ignore_id {
                let result = sync::write_message(&mut *client, &msg);

                if result.is_err() {
                    // Unable to write message to socket. This means the connection is closed.
                    closed_connections.push(id);
                }
            }
        }

        // Remove connections that are closed so we don't broadcast to them in the future
        for id in closed_connections {
            println!("[debug] broadcast failed, removing socket from broadcaster");
            subscribers.remove(&id);
        }
    }
}
