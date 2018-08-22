use std::io::prelude::*;
use std::net::TcpListener;
use std::net::{Shutdown, TcpStream};
use std::thread;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::mem;
use std::io::BufReader;
use std::time::Duration;

pub struct FakeServer {
    address: String,
    client_tx: mpsc::Sender<&'static str>,
    client: Arc<Mutex<Option<TcpStream>>>
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

    pub fn break_current_connection(&self) {
        let mut client = self.client.lock().unwrap();
        if let Some(ref s) = *client {
            s.shutdown(Shutdown::Both).unwrap();
        }
        *client = None;
    }

    pub fn socket_address(&self) -> String {
        self.address.clone()
    }

    pub fn on_client_message<F>(&mut self, listener: F) where F: Fn(String) + Send + 'static {
        listen_to_client_message(&self.client, &Arc::new(Mutex::new(listener)));
    }

}

fn listen_to_client_message<F>(client: &Arc<Mutex<Option<TcpStream>>>, listener: &Arc<Mutex<F>>) where F: Fn(String) + Send + 'static {
    let mut stream = None;

    {
        let client = client.lock().unwrap();
        if let Some(ref s) = *client {
            stream = Some(s.try_clone().unwrap());
        }
    }

    if let Some(stream) = stream {
        let listener = Arc::clone(&listener);
        let client = Arc::clone(&client);

        thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                if let Ok(line) = line {
                    let listener = listener.lock().unwrap();
                    listener(line)
                } else {
                    break;
                }
            }

            listen_to_client_message(&client, &listener);
        });
    } else {
        let listener = Arc::clone(&listener);
        let client = Arc::clone(&client);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            listen_to_client_message(&client, &listener);
        });
    }
}

fn listen(connection: TcpListener) -> FakeServer {
    let (client_tx, server_rx) = mpsc::channel();
    let (server_tx, client_rx) = mpsc::channel();
    let client = Arc::new(Mutex::new(None));
    let listener = connection.try_clone().unwrap();

    let current_client = Arc::clone(&client);

    thread::spawn(move || {
        let local_address = format!("localhost:{}", listener.local_addr().unwrap().port());
        let address: &str;

        unsafe {
            address = mem::transmute(&local_address as &str);
            mem::forget(local_address);
        }

        server_tx.send(address).unwrap();

        let c = Arc::clone(&current_client);

        thread::spawn(move|| {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut client = c.lock().unwrap();

                *client = Some(stream.try_clone().unwrap());
            }
        });

        for received in &server_rx {
            let mut client = current_client.lock().unwrap();
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
        client_tx,
        client
    }
}
