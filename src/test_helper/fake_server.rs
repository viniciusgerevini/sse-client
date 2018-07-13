use std::io::prelude::*;
use std::net::TcpListener;
use std::thread;
use std::sync::mpsc;
use std::mem;

pub struct FakeServer {
    listener: TcpListener,
    address: String,
    client_tx: mpsc::Sender<&'static str>
}

impl FakeServer {
    pub fn new() -> FakeServer {
        listen(TcpListener::bind("localhost:0").unwrap())
    }

    pub fn create(url: &str) -> FakeServer {
        listen(TcpListener::bind(url).unwrap())
    }

    pub fn reconnect(&self) -> FakeServer {
        listen(self.listener.try_clone().unwrap())
    }

    pub fn send(&self, message: &'static str) {
        &self.client_tx.send(message).unwrap();
    }

    pub fn close(&self) {
        &self.client_tx.send("close").unwrap();
    }

    pub fn socket_address(&self) -> String {
        self.address.clone()
    }
}

fn listen(connection: TcpListener) -> FakeServer {
    let (client_tx, server_rx) = mpsc::channel();
    let (server_tx, client_rx) = mpsc::channel();

    let listener = connection.try_clone().unwrap();
    thread::spawn(move || {
        let local_address = format!("localhost:{}", listener.local_addr().unwrap().port());
        let address: &str;

        unsafe {
            address = mem::transmute(&local_address as &str);
            mem::forget(local_address);
        }

        server_tx.send(address).unwrap();

        for stream in listener.incoming() {
            let mut stream = stream.unwrap();

            for received in server_rx {
                match received {
                    "close" => break,
                    _ => {
                        stream.write(received.as_bytes()).unwrap();
                    }
                }
            }
            break;
        }
    });

    let address = String::from(client_rx.recv().unwrap());

    FakeServer {
        listener: connection,
        address,
        client_tx
    }
}
