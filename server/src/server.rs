use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

use futures::{self, future, Async, AsyncSink, Future, Poll, Sink, Stream};
use futures::sync::mpsc::UnboundedSender;
use tokio_core;
use tokio_core::reactor::Handle;
use tokio_core::net::{Incoming, TcpListener, TcpStream};
use tokio_io::{self, AsyncRead};

use protocol::async::ChatCodec;

type ActiveClients = Rc<RefCell<Vec<Client>>>;
type MsgStream = futures::stream::SplitStream<tokio_io::codec::Framed<tokio_core::net::TcpStream, ChatCodec>>;
type MsgSink = futures::sink::Buffer<futures::stream::SplitSink<tokio_io::codec::Framed<tokio_core::net::TcpStream, ChatCodec>>>;

pub struct Client {
    nickname: Option<String>,
    sink: MsgSink,
}

/// Builds a stream that returns a message sink/stream pair for each incoming connection
fn client_stream(handle: &Handle) -> impl Stream<Item=(MsgSink, MsgStream), Error=()> {
    let addr = "127.0.0.1:8080".parse().expect("Unable to parse socket address");
    let listener = TcpListener::bind(&addr, handle).expect("Unable to launch tcp listener").incoming();
    listener.map(|(stream, _)| {
        let (sink, stream) = stream.framed(ChatCodec).split();
        (sink.buffer(100), stream)
    }).map_err(|e| panic!("Error listening to TCP connections: {}", e))
}

/// Setup the accions that will be taken when a client connects
///
/// client: the client that just connected
/// active_clients: the clients that are active at that moment
/// sink: a sink where the messages from this client will be forwarded
fn setup_client(handle: &Handle, msg_sink: MsgSink, msg_stream: MsgStream, active_clients: &ActiveClients, broadcast_tx: UnboundedSender<String>) {
    println!("New client!");

    // Put incoming clients in the active_clients vector
    active_clients.borrow_mut().push(Client {
        nickname: None,
        sink: msg_sink
    });

    // Forward incoming messages to the broadcaster
    let broadcast_tx = broadcast_tx.sink_map_err(|e| -> ::std::io::Error { panic!("channel error: {}", e) });
    let forward_messages = msg_stream.forward(broadcast_tx)
                                     .map(|_| ()) // Ignore the result of the future
                                     .map_err(|_| println!("Connection closed")); // Ignore errors

    handle.spawn(forward_messages)
}

/// Broadcast a message to all active clients
fn broadcast_message(handle: &Handle, m: String, active_clients: &ActiveClients) {
    println!("Message received: {}", m);

    let mut active_clients = active_clients.borrow_mut();
    let mut deleted_indices = Vec::new();
    for (i, client) in active_clients.iter_mut().enumerate() {
        // Don't send messages to clients that haven't provided a nickname
        if client.nickname.is_none() {
            continue;
        }

        // FIXME: we are cloning too much... We should RC the string.
        match client.sink.start_send(m.clone()) {
            Ok(AsyncSink::Ready) => (),
            Ok(AsyncSink::NotReady(_)) => {
                // The sink is full. If this happens we just drop the message
                println!("Sink full, dropping message: {}", m)
            }
            Err(e) => {
                // Buffer full or connection closed
                println!("Buffer full or connection closed: {:?}", e)
            }
        }

        // Async flushing
        match client.sink.poll_complete() {
            Err(e) => {
                println!("Connection closed");
                deleted_indices.push(i);
            }
            _ => {}
        }
    }

    // Remove closed connections from our active client list
    let mut deleted_count = 0;
    for i in deleted_indices {
        active_clients.swap_remove(i - deleted_count);
        deleted_count += 1;
    }
}

/// Start the server
///
/// This function will spawn the necessary futures using the `Handle`
pub fn start(handle: &Handle) {
    // The sinks corresponding to all open connections are kept in this vector
    let active_clients = Rc::new(RefCell::new(Vec::new()));

    // This channel is used to send incoming messages to the broadcaster future
    let (broadcast_tx, broadcast_rx) = futures::sync::mpsc::unbounded();

    // Accept new connections and specify how incoming messages will be handled
    let (owned_handle, owned_clients) = (handle.clone(), active_clients.clone());
    handle.spawn(client_stream(handle).for_each(move |(msg_sink, msg_stream)| {
        setup_client(&owned_handle, msg_sink, msg_stream, &owned_clients, broadcast_tx.clone());
        future::empty()
    }));

    // Broadcast incoming messages to all active clients
    let owned_handle = handle.clone();
    handle.spawn(broadcast_rx.for_each(move |m| {
        broadcast_message(&owned_handle, m, &active_clients);
        future::empty()
    }));
}
