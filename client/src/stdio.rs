use std::io::{self, BufRead};
use std::thread;
use futures::{self, Future, Sink, Stream};

// Based on https://github.com/tokio-rs/tokio-core/blob/c13e7f35337ca37ae3f4207e861375fc250f1c05/examples/connect.rs#L120
pub fn stdin_lines() -> impl Stream<Item=String, Error=!> {
    let (mut tx, rx) = futures::sync::mpsc::channel(0);
    thread::spawn(move || {
        let stdin = io::stdin();
        let lock = stdin.lock();
        for line in lock.lines() {
            tx = tx.send(line.unwrap()).wait().unwrap();
        }
    });

    rx.map_err(|_| unreachable!())
}

pub fn stdout_lines() -> impl Sink<SinkItem=String, SinkError=!> {
    let (tx, rx) = futures::sync::mpsc::channel(0);
    thread::spawn(move || {
        rx.for_each(|msg| { println!("{}", msg); Ok(()) }).wait()
    });

    tx.sink_map_err(|e| panic!("{}", e))
}