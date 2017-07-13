#![feature(conservative_impl_trait, never_type)]

extern crate futures;
extern crate protocol;
extern crate tokio_core;
extern crate tokio_io;

mod stdio;

use std::io::{self, Write};

use futures::{BoxFuture, Future, Sink, Stream, stream};
use tokio_io::AsyncRead;
use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;
use protocol::async::ChatCodec;

use self::stdio::{stdin_lines, stdout_lines};

fn main() {
    // First get the nickname
    let nickname = get_nickname();

    let addr = "127.0.0.1:8080".parse().unwrap();
    let mut core = Core::new().expect("Failed to create tokio core");
    let handle = core.handle();
    let client_future = TcpStream::connect(&addr, &handle)
                                  .and_then(|f| connection(f, &nickname));

    core.run(client_future).unwrap()
}

fn get_nickname() -> String {
    print!("Please enter a nickname: ");
    io::stdout().flush().ok();
    let mut nickname = String::new();
    io::stdin().read_line(&mut nickname).unwrap();
    nickname = nickname.trim().to_string();

    if nickname == "" {
        "<unknown>".to_string()
    } else {
        nickname
    }
}

fn connection(tcp_stream: TcpStream, nickname: &str) -> BoxFuture<(), io::Error> {
    let (sink, incoming) = tcp_stream.framed(ChatCodec).split();

    // Send the nickname and lines from stdin
    let send_nickname = stream::once(Ok(nickname.to_string()));
    let send_lines = stdin_lines();
    let send_all = send_nickname.chain(send_lines).map_err(|_| unreachable!());

    // Print messages received from the server
    let print_msg = stdout_lines().sink_map_err(|_| -> ::std::io::Error { unreachable!() });

    send_all.forward(sink)
        .join(incoming.forward(print_msg))
        .map(|_| ()).boxed()
}
