use std::io::prelude::*;
use std::net::TcpListener;
use std::thread;
use std::sync::mpsc;
use std::mem;

pub struct FakeServer {
    address: String,
    client_tx: mpsc::Sender<&'static str>
}

impl FakeServer {
    pub fn new() -> FakeServer {
        let (client_tx, server_rx) = mpsc::channel();
        let (server_tx, client_rx) = mpsc::channel();

        thread::spawn(move || {
            let listener = TcpListener::bind("localhost:0").unwrap();

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
            address,
            client_tx
        }
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
