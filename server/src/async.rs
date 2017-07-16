use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use futures::{future, sink, stream, AsyncSink, Future, Sink, Stream};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::{TcpListener, TcpStream};
use tokio_io::{codec, AsyncRead};

use protocol::async::ChatCodec;

type MsgSink = sink::Buffer<stream::SplitSink<codec::Framed<TcpStream, ChatCodec>>>;

pub fn run() {
    let mut core = Core::new().expect("Unable to initialize tokio core");
    start_server(&core.handle());
    core.run(future::empty::<(), ()>()).expect("Run failed");
}

pub fn start_server(handle: &Handle) {
    let broadcaster = Rc::new(Broadcaster::default());

    // Setup the listener
    let addr = "127.0.0.1:8080".parse().expect("Unable to parse socket address");
    let listener = TcpListener::bind(&addr, handle).expect("Unable to launch tcp listener").incoming();

    // Accept new connections and specify how incoming messages will be handled
    let owned_handle = handle.clone();
    handle.spawn(listener.for_each(move |(tcp_stream, _)| {
        let (msg_sink, msg_stream) = tcp_stream.framed(ChatCodec).split();
        let client_id = broadcaster.subscribe(msg_sink.buffer(100));

        println!("[debug] new connection ({})", client_id);

        // Setup broadcasting of incoming messages from this client to all other active clients
        let broadcaster = broadcaster.clone();
        owned_handle.spawn(msg_stream.into_future().and_then(move |(opt_nickname, msg_stream)| {
            // The first message is the nickname
            let nickname = opt_nickname.unwrap_or(String::new());
            broadcaster.broadcast(client_id, format!("{} has logged in", nickname));

            // All further messages can just be forwarded
            let broadcaster = broadcaster.clone();
            msg_stream.for_each(move |msg| {
                let msg = format!("{}: {}", nickname, msg);
                broadcaster.broadcast(client_id, msg);
                future::ok(())
            }).map_err(move |_| {
                println!("[debug] stream closed ({})", client_id);

                // FIXME: this panic should be removed, but the type system doesn't like it...
                panic!()
            })
        }).map_err(|_| panic!("FIXME: What should we do?")));

        future::ok(())
    }).map_err(|e| panic!("Error listening to TCP connections: {}", e)));
}

#[derive(Default)]
struct Broadcaster {
    next_client_id: Cell<usize>,
    subscribers: RefCell<HashMap<usize, MsgSink>>
}

impl Broadcaster {
    fn subscribe(&self, sink: MsgSink) -> usize {
        // Generate a new id
        let client_id = self.next_client_id.get();
        self.next_client_id.set(client_id + 1);

        // Subscribe to broadcasts
        self.subscribers.borrow_mut().insert(client_id, sink);
        client_id
    }

    fn broadcast(&self, ignore_id: usize, msg: String) {
        let mut subscribers = self.subscribers.borrow_mut();

        // Prepare the message to be sent
        println!("[message] {}", msg);

        // When a send fails, we will store the index of the client in this
        // vector. This way we can remove clients that have disconnected
        let mut closed_connections = Vec::new();

        for (&client_id, client) in subscribers.iter_mut() {
            // Don't send messages to yourself
            if client_id == ignore_id {
                continue;
            }

            match client.start_send(msg.clone()).expect("unexpected error") {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(_) => println!("[debug] sink full, dropping message: {}", msg)
            }

            // We need `poll_complete` to flush the sink
            // FIXME: is this correct? This is the code I am least confident with in this crate
            if let Err(_) = client.poll_complete() {
                println!("[debug] connection closed");
                closed_connections.push(client_id);
            }
        }

        // Remove connections that are closed so we don't broadcast to them in the future
        for id in closed_connections {
            println!("[debug] broadcast failed, removing socket from broadcaster");
            subscribers.remove(&id);
        }
    }
}
