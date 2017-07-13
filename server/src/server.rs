use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;

use futures::{self, future, AsyncSink, Future, Sink, Stream};
use futures::sync::mpsc::UnboundedSender;
use tokio_core;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpListener;
use tokio_io::{self, AsyncRead};

use protocol::async::ChatCodec;

type ActiveClients = Rc<RefCell<Vec<Client>>>;
type MsgSink = futures::sink::Buffer<futures::stream::SplitSink<tokio_io::codec::Framed<tokio_core::net::TcpStream, ChatCodec>>>;
type MsgStream = futures::stream::SplitStream<tokio_io::codec::Framed<tokio_core::net::TcpStream, ChatCodec>>;

struct Client {
    id: u32,
    nickname: Option<String>,
    sink: MsgSink,
}

struct Message {
    from: u32,
    text: String
}

/// Builds a stream that returns a message sink/stream pair for each incoming connection
fn client_stream(handle: &Handle) -> impl Stream<Item=(MsgSink, MsgStream), Error=()> {
    let addr = "127.0.0.1:8080".parse().expect("Unable to parse socket address");
    let listener = TcpListener::bind(&addr, handle).expect("Unable to launch tcp listener").incoming();
    listener.map(move |(stream, _)| {
        let (sink, stream) = stream.framed(ChatCodec).split();
        (sink.buffer(100), stream)
    }).map_err(|e| panic!("Error listening to TCP connections: {}", e))
}

/// Setup the accions that will be taken when a client connects
///
/// client_id
/// msg_sink: a sink to the client that just connected
/// msg_stream: a stream from the client that just connected
/// active_clients: the clients that are active at that moment
/// broadcast_tx: a sink where the messages from this client will be forwarded
fn setup_client<S>(handle: &Handle, client_id: u32, msg_sink: MsgSink, msg_stream: S, active_clients: &ActiveClients, broadcast_tx: UnboundedSender<Message>)
where S: Stream<Item=Message, Error=io::Error> + 'static
{
    println!("[debug] new client");

    // Put incoming clients in the active_clients vector
    active_clients.borrow_mut().push(Client {
        id: client_id,
        nickname: None,
        sink: msg_sink
    });

    // Forward incoming messages to the broadcaster
    let broadcast_tx = broadcast_tx.sink_map_err(|e| -> io::Error { panic!("channel error: {}", e) });
    let forward_messages = msg_stream.forward(broadcast_tx)
                                     .map(|_| ()) // Ignore the result of the future
                                     .map_err(|_| println!("[debug] connection closed")); // Ignore errors

    handle.spawn(forward_messages)
}

/// Broadcast a message to all active clients
fn broadcast_message(msg: Message, active_clients: &ActiveClients) {
    let mut active_clients = active_clients.borrow_mut();

    // Get the nickname of the client
    // If the nickname is None, it means that this message is the nickname!
    let message = {
        let nickname = &mut active_clients.iter_mut().find(|c| c.id == msg.from).expect("Message from inactive client?").nickname;
        match nickname.clone() {
            Some(n) => format!("{}: {}", n, msg.text),
            None => {
                *nickname = Some(msg.text.clone());
                format!("{} has logged in", msg.text)
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
        if client.id == msg.from {
            continue;
        }

        match client.sink.start_send(message.clone()).expect("unexpected error") {
            AsyncSink::Ready => (),
            AsyncSink::NotReady(_) => println!("[debug] sink full, dropping message: {}", message)
        }

        // We need `poll_complete` to flush the sink
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

/// Start the server
///
/// This function will spawn the necessary futures using the `Handle`
pub fn start(handle: &Handle) {
    // The client information corresponding to all open connections are kept in this vector
    let active_clients = Rc::new(RefCell::new(Vec::new()));

    // This channel is used to send incoming messages to the broadcaster future
    let (broadcast_tx, broadcast_rx) = futures::sync::mpsc::unbounded::<Message>();

    // Accept new connections and specify how incoming messages will be handled
    let fresh_client_id = Cell::new(0);
    let (owned_handle, owned_clients) = (handle.clone(), active_clients.clone());
    handle.spawn(client_stream(handle).for_each(move |(msg_sink, msg_stream)| {
        // First, assign a unique id to the client
        let id = fresh_client_id.get();
        fresh_client_id.set(id + 1);

        // Label all messages from this client with its id
        let msg_stream = msg_stream.map(move |text| Message { from: id, text });

        setup_client(&owned_handle, id, msg_sink, msg_stream, &owned_clients, broadcast_tx.clone());
        future::empty()
    }));

    // Broadcast incoming messages to all active clients
    handle.spawn(broadcast_rx.for_each(move |m| {
        broadcast_message(m, &active_clients);
        future::empty()
    }));
}
