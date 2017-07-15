use std::cell::RefCell;
use std::rc::Rc;

use futures::{future, sink, stream, AsyncSink, Future, Sink, Stream};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::{TcpListener, TcpStream};
use tokio_io::{codec, AsyncRead};

use protocol::async::ChatCodec;

type ActiveClients = Rc<RefCell<Vec<Client>>>;
type MsgSink = sink::Buffer<stream::SplitSink<codec::Framed<TcpStream, ChatCodec>>>;

struct Client {
    id: u32,
    nickname: Option<String>,
    sink: MsgSink,
}

pub fn run() {
    let mut core = Core::new().expect("Unable to initialize tokio core");
    start_server(&core.handle());
    core.run(future::empty::<(), ()>()).expect("Run failed");
}

pub fn start_server(handle: &Handle) {
    // The client information corresponding to all open connections will be kept in this vector
    let active_clients = Rc::new(RefCell::new(Vec::new()));

    // Setup the listener
    let addr = "127.0.0.1:8080".parse().expect("Unable to parse socket address");
    let listener = TcpListener::bind(&addr, handle).expect("Unable to launch tcp listener").incoming();

    // Accept new connections and specify how incoming messages will be handled
    let client_ids = stream::iter((0..).map(Ok));
    let owned_handle = handle.clone();
    handle.spawn(client_ids.zip(listener).for_each(move |(client_id, tcp_stream)| {
        println!("[debug] new client");

        let (msg_sink, msg_stream) = tcp_stream.0.framed(ChatCodec).split();
        let msg_sink = msg_sink.buffer(100);

        // Track active clients
        active_clients.borrow_mut().push(Client {
            id: client_id,
            nickname: None,
            sink: msg_sink
        });

        // Setup broadcasting of incoming messages from this client to all other active clients
        let active_clients = active_clients.clone();
        owned_handle.spawn(msg_stream.for_each(move |msg| {
            broadcast_message(&active_clients, client_id, msg);
            future::ok(())
        }).map_err(|_| println!("[debug] connection closed")));

        future::ok(())
    }).map_err(|e| panic!("Error listening to TCP connections: {}", e)));
}

/// Broadcast a message to all active clients
fn broadcast_message(active_clients: &ActiveClients, client_id: u32, msg: String) {
    let mut active_clients = active_clients.borrow_mut();

    // Get the nickname of the client
    // If the nickname is None, it means that this message is the nickname!
    let message = {
        let nickname = &mut active_clients.iter_mut().find(|c| c.id == client_id).expect("Message from inactive client?").nickname;
        match nickname.clone() {
            Some(n) => format!("{}: {}", n, msg),
            None => {
                *nickname = Some(msg.clone());
                format!("{} has logged in", msg)
            }
        }
    };

    // Prepare the message to be sent
    println!("[message] {}", message);

    // When a send fails, we will store the index of the client in this
    // vector. This way we can remove clients that have disconnected
    let mut disconnected = Vec::new();

    for (i, client) in active_clients.iter_mut().enumerate() {
        // Don't send messages to clients that haven't provided a nickname
        if client.nickname.is_none() {
            continue;
        }

        // Don't send messages to yourself
        if client.id == client_id {
            continue;
        }

        match client.sink.start_send(message.clone()).expect("unexpected error") {
            AsyncSink::Ready => (),
            AsyncSink::NotReady(_) => println!("[debug] sink full, dropping message: {}", message)
        }

        // We need `poll_complete` to flush the sink
        // FIXME: is this correct? This is the code I am least confident with in this crate
        if let Err(_) = client.sink.poll_complete() {
            println!("[debug] connection closed");
            disconnected.push(i);
        }
    }

    // Remove closed connections from our active client list
    let mut deleted_count = 0;
    for i in disconnected {
        active_clients.swap_remove(i - deleted_count);
        deleted_count += 1;
    }
}
