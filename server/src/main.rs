#![feature(conservative_impl_trait)]

extern crate futures;
extern crate protocol;
extern crate tokio_core;
extern crate tokio_io;

mod async;
mod sync;

use std::env;

fn main() {
    let mode = env::args().nth(1).unwrap_or(String::new());
    if mode == "sync" {
        sync::run().unwrap();
    } else {
        // Default to async
        async::run()
    }
}
