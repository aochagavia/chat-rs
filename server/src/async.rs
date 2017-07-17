use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use futures::{future, sink, stream, AsyncSink, Future, Sink, Stream};
use tokio_core::reactor::Core;
use tokio_core::net::{Incoming, TcpListener, TcpStream};
use tokio_io::{codec, AsyncRead};

use protocol::async::ChatCodec;

type MsgSink = sink::Buffer<stream::SplitSink<codec::Framed<TcpStream, ChatCodec>>>;

pub fn run() {
    let mut core = Core::new().expect("Unable to initialize tokio core");
    let handle = core.handle();

    let addr = "127.0.0.1:8080".parse().expect("Unable to parse socket address");
    let listener = TcpListener::bind(&addr, &handle).expect("Unable to launch tcp listener");
    let server = gen_server(listener.incoming());
    core.run(server).expect("Run failed");
}

pub fn gen_server(listener: Incoming) -> impl Future<Item=(), Error=()> {
    let listener = listener.map_err(|e| panic!("Listener error: {}", e));
    let broadcaster = Rc::new(Broadcaster::default());

    // Accept new connections and specify how incoming messages will be handled
    listener.for_each(move |(tcp_stream, _)| {
        println!("[debug] new connection");

        let (msg_sink, msg_stream) = tcp_stream.framed(ChatCodec).split();

        // Setup broadcasting of incoming messages from this client to all other active clients
        let broadcaster = broadcaster.clone();
        msg_stream.into_future()
                  .map_err(|_| println!("[debug] stream closed (into_future)"))
                  .and_then(move |(opt_nickname, msg_stream)| {
            // Only subscribe a client after we receive their nickname
            let client_id = broadcaster.subscribe(msg_sink.buffer(100));

            // The nickname could be `None`, but that would mean the connection has been dropped
            // Therefore we can use an empty nickname in that case, as the client will be removed anyway
            let nickname = opt_nickname.unwrap_or(String::new());
            broadcaster.broadcast(client_id, format!("{} has logged in", nickname));

            // After receiving the nickname, messages only need to be forwarded
            let broadcaster = broadcaster.clone();
            msg_stream.for_each(move |msg| {
                let msg = format!("{}: {}", nickname, msg);
                broadcaster.broadcast(client_id, msg);
                future::ok(())
            }).map_err(|_| println!("[debug] stream closed (for_each)"))
        }).then(|_| Ok(()))  // Ignore errors
    })
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
