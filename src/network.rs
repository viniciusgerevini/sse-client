use std::io::prelude::*;
use std::io::Error;
use std::net::{Shutdown, TcpStream};
use url::Url;
use std::thread;
use std::sync::Arc;
use std::sync::Mutex;
use std::io::BufReader;

#[derive(Debug, PartialEq, Clone)]
pub enum State {
    CONNECTING,
    OPEN,
    CLOSED
}

pub struct EventStream {
    stream: TcpStream,
    state: Arc<Mutex<State>>,
    on_open_listener: Arc<Mutex<Option<Box<Fn() + Send>>>>,
    on_message_listener: Arc<Mutex<Option<Box<Fn(String) + Send>>>>,
    on_error_listener: Arc<Mutex<Option<Box<Fn(String) + Send>>>>
}

impl EventStream {
    pub fn new(url: Url) -> Result<EventStream, Error> {
        let path = url.path();
        let host = get_host(&url);
        let host = host.as_str();

        let mut stream = TcpStream::connect(host)?;
        let state = Arc::new(Mutex::new(State::CONNECTING));
        let on_open_listener = Arc::new(Mutex::new(None));
        let on_message_listener = Arc::new(Mutex::new(None));
        let on_error_listener = Arc::new(Mutex::new(None));

        let response = format!(
            "GET {} HTTP/1.1\r\nAccept: text/event-stream\r\nHost: {}\r\n\r\n",
            path,
            host
        );

        stream.write(response.as_bytes())?;
        stream.flush().unwrap();

        let event_stream = EventStream {
            stream,
            state,
            on_open_listener,
            on_message_listener,
            on_error_listener
        };

        event_stream.listen();

        Ok(event_stream)
    }

    fn listen(&self) {
        let stream = self.stream.try_clone().unwrap();
        let state = Arc::clone(&self.state);
        let on_open_listener = Arc::clone(&self.on_open_listener);
        let on_message_listener = Arc::clone(&self.on_message_listener);
        let on_error_listener = Arc::clone(&self.on_error_listener);

        thread::spawn(move || {
            let reader = BufReader::new(stream);

            for line in reader.lines() {
                let mut state = state.lock().unwrap();

                let line = match line {
                    Ok(l) => l,
                    Err(error) => {
                        let mut on_error_listener = on_error_listener.lock().unwrap();
                        if let Some(ref f) = *on_error_listener {
                            f(error.to_string());
                        }
                        *state = State::CLOSED;
                        break
                    }
                };


                match *state {
                    State::CONNECTING => {
                        if line == "" {
                            *state = State::OPEN;
                            let mut on_open_listener = on_open_listener.lock().unwrap();
                            if let Some(ref f) = *on_open_listener {
                                f();
                            }
                        } else if line.starts_with("HTTP/1.1 ") {
                            let status = &line[9..];
                            if !status.starts_with("200") {
                                let mut on_error_listener = on_error_listener.lock().unwrap();
                                if let Some(ref f) = *on_error_listener {
                                    f(status.to_string());
                                }
                                *state = State::CLOSED;
                                break
                            }
                        }
                    }
                    _ => {
                        let mut on_message_listener = on_message_listener.lock().unwrap();
                        if let Some(ref f) = *on_message_listener {
                            f(line);
                        }
                    }
                }
            }
        });
    }

    pub fn close(&self) {
        self.stream.shutdown(Shutdown::Both).unwrap();
        let mut state = self.state.lock().unwrap();
        *state = State::CLOSED;
    }

    pub fn on_open<F>(&mut self, listener: F) where F: Fn() + Send + 'static {
        let mut on_open_listener = self.on_open_listener.lock().unwrap();
        *on_open_listener = Some(Box::new(listener));
    }

    pub fn on_message<F>(&mut self, listener: F) where F: Fn(String) + Send + 'static {
        let mut on_message_listener = self.on_message_listener.lock().unwrap();
        *on_message_listener = Some(Box::new(listener));
    }

    pub fn on_error<F>(&mut self, listener: F) where F: Fn(String) + Send + 'static {
        let mut on_error_listener = self.on_error_listener.lock().unwrap();
        *on_error_listener = Some(Box::new(listener));
    }

    pub fn state(&self) -> State {
        let state = &self.state.lock().unwrap();
        (*state).clone()
    }
}

fn get_host(url: &Url) -> String {
    let mut host = match url.host_str() {
        Some(h) => String::from(h),
        None => String::from("localhost")
    };

    if let Some(port) = url.port() {
        host = format!("{}:{}", host, port);
    }

    host
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::test_helper::fake_server::FakeServer;
    use std::sync::mpsc;
    use std::time::Duration;

    fn setup() -> (EventStream, FakeServer) {
        let fake_server = FakeServer::new();
        let address = format!("http://{}/sub", fake_server.socket_address());
        let url = Url::parse(address.as_str()).unwrap();
        let event_stream = EventStream::new(url).unwrap();

        (event_stream, fake_server)
    }

    #[test]
    fn should_create_stream_object() {
        let (event_stream, fake_server) = setup();
        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_trigger_on_open_listener() {
        let (tx, rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_open(move || {
            tx.send("open").unwrap();
        });

        fake_server.send("HTTP/1.1 200 OK\n");
        fake_server.send("Date: Thu, 24 May 2018 12:26:38 GMT\n");
        fake_server.send("\n");

        rx.recv().unwrap();

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_connecting_while_opening_connection() {
        let (event_stream, fake_server) = setup();

        let state = event_stream.state();
        assert_eq!(state, State::CONNECTING);

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_open_after_connection_stabilished() {
        let (event_stream, fake_server) = setup();

        fake_server.send("\n");
        thread::sleep(Duration::from_millis(200));

        let state = event_stream.state();
        assert_eq!(state, State::OPEN);

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_closed_after_closing_connection() {
        let (event_stream, fake_server) = setup();

        event_stream.close();
        thread::sleep(Duration::from_millis(200));

        let state = event_stream.state();
        assert_eq!(state, State::CLOSED);

        fake_server.close();
    }

    #[test]
    fn should_trigger_listeners_when_message_received() {
        let (tx, rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.send("\ndata: some message\n\n");

        let message = rx.recv().unwrap();

        assert_eq!(message, "data: some message");

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_trigger_on_error_when_connection_closed_by_server() {
        let (tx, rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_error(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.close();

        let message = rx.recv().unwrap();

        assert!(message.contains("Connection reset by peer"));
    }

    #[test]
    fn should_have_state_closed_when_connection_closed_by_server() {
        let (tx, rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_error(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.close();

        rx.recv().unwrap();

        assert_eq!(event_stream.state(), State::CLOSED);
    }

    #[test]
    fn should_trigger_error_when_status_code_is_not_success() {
        let (tx, rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_error(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 500 Internal Server Error\n");
        fake_server.send("Date: Thu, 24 May 2018 12:26:38 GMT\n");
        fake_server.send("\n");

        let message = rx.recv().unwrap();

        assert_eq!(message, "500 Internal Server Error");

        event_stream.close();
        fake_server.close();
    }
}

