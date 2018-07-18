use std::io::prelude::*;
use std::net::TcpListener;
use std::thread;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::mem;

pub struct FakeServer {
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

        let client = Arc::new(Mutex::new(None));
        let c = Arc::clone(&client);

        thread::spawn(move|| {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut client = c.lock().unwrap();
                *client = Some(stream.try_clone().unwrap());
            }
        });

        for received in &server_rx {
            let mut client = client.lock().unwrap();
            match received {
                "close" => {
                    *client = None;
                },
                _ => {
                    if let Some(ref mut stream) = *client {
                        stream.write(received.as_bytes()).unwrap();
                    }
                }
            }
        }
    });

    let address = String::from(client_rx.recv().unwrap());

    FakeServer {
        address,
        client_tx
    }
}
