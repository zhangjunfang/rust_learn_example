#[macro_use]
extern crate futures;

extern crate tokio_core;
extern crate tokio_proto;
extern crate bytes;

use futures::{Future, Poll, Async};
use futures::stream::Stream;

use tokio_core::reactor;
use tokio_core::net::{TcpStream, TcpListener};

use tokio_proto::{TryRead, TryWrite};

use bytes::{Buf};
use bytes::buf::SliceBuf;

use std::io;
use std::net::SocketAddr;

// Echo server connection. Reads packets off of `socket` into `buf` and writes
// it back.
struct Echo {
    socket: TcpStream,
    buf: SliceBuf,
    state: State,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum State {
    Read,
    Write,
}

impl Future for Echo {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        loop {
            match self.state {
                State::Read => {
                    let n = try_ready!(self.socket.try_read_buf(&mut self.buf));

                    if n == 0 {
                        return Ok(Async::Ready(()));
                    }

                    self.state = State::Write;
                }
                State::Write => {
                    try_ready!(self.socket.try_write_buf(&mut self.buf));

                    if !Buf::has_remaining(&self.buf) {
                        self.state = State::Read;
                    }
                }
            };
        }
    }
}

pub fn main() {
    let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    // Initialize a new reactor core
    let mut core = reactor::Core::new().unwrap();

    // Get a core handle, this will be used below to spawn new tasks on the
    // reactor
    let handle = core.handle();

    // Bind the the listener.
    let listener = TcpListener::bind(&addr, &handle).unwrap();

    println!("Running server on {}", addr);

    core.run(listener.incoming().for_each(move |(socket, _)| {
        // Create the echo handler
        let echo = Echo {
            socket: socket,
            buf: SliceBuf::with_capacity(1024),
            state: State::Read,
        };

        handle.spawn(echo.map_err(|err| {
            println!("Oh no! Error {:?}", err);
        }));

        Ok(())
    })).unwrap();
}

