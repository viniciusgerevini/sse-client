use std::io::prelude::*;
use std::io::Error;
use std::net::{Shutdown, TcpStream};
use url::Url;
use std::thread;
use std::sync::Arc;
use std::sync::Mutex;
use std::io::BufReader;
use std::time::Duration;

#[derive(Debug, PartialEq, Clone)]
pub enum State {
    CONNECTING,
    OPEN,
    CLOSED
}

#[derive(Debug, PartialEq)]
enum StreamAction {
    CONTINUE,
    RECONNECT
}

pub struct EventStream {
    url: Arc<Url>,
    stream: Arc<Mutex<Option<TcpStream>>>,
    state: Arc<Mutex<State>>,
    on_open_listener: Arc<Mutex<Option<Box<Fn() + Send>>>>,
    on_message_listener: Arc<Mutex<Option<Box<Fn(String) + Send>>>>,
    on_error_listener: Arc<Mutex<Option<Box<Fn(String) + Send>>>>
}

impl EventStream {
    pub fn new(url: Url) -> Result<EventStream, Error> {
        let event_stream = EventStream {
            url: Arc::new(url),
            stream: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(State::CONNECTING)),
            on_open_listener: Arc::new(Mutex::new(None)),
            on_message_listener: Arc::new(Mutex::new(None)),
            on_error_listener: Arc::new(Mutex::new(None))
        };

        event_stream.listen();

        Ok(event_stream)
    }

    fn listen(&self) {
        listen_stream(
            Arc::clone(&self.url),
            Arc::clone(&self.stream),
            Arc::clone(&self.state),
            Arc::clone(&self.on_open_listener),
            Arc::clone(&self.on_message_listener),
            Arc::clone(&self.on_error_listener)
        );
    }

    pub fn close(&self) {
        if let Some(ref st) = *self.stream.lock().unwrap() {
            st.shutdown(Shutdown::Both).unwrap();
        }
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

fn listen_stream(
    url: Arc<Url>,
    stream: Arc<Mutex<Option<TcpStream>>>,
    state: Arc<Mutex<State>>,
    on_open: Arc<Mutex<Option<Box<Fn() + Send>>>>,
    on_message: Arc<Mutex<Option<Box<Fn(String) + Send>>>>,
    on_error: Arc<Mutex<Option<Box<Fn(String) + Send>>>>
) {
    thread::spawn(move || {
        let action = match connect_event_stream(&url, &stream) {
            Ok(stream) => read_stream(stream, &state, &on_open, &on_message, &on_error),
            _ => StreamAction::RECONNECT
        };

        if action == StreamAction::RECONNECT {
             reconnect_stream(url, stream, state, on_open, on_message, on_error)
        }
    });
}

fn connect_event_stream(url: &Url, stream: &Arc<Mutex<Option<TcpStream>>>) -> Result<TcpStream, Error> {
    let connection_stream = event_stream_handshake(url)?;

    let mut stream_lock = stream.lock().unwrap();
    *stream_lock = Some(connection_stream.try_clone().unwrap());

    Ok(connection_stream)
}

fn event_stream_handshake(url: &Url) -> Result<TcpStream, Error> {
    let path = url.path();
    let host = get_host(&url);
    let host = host.as_str();

    let mut stream = TcpStream::connect(host)?;

    let request = format!(
        "GET {} HTTP/1.1\r\nAccept: text/event-stream\r\nHost: {}\r\n\r\n",
        path,
        host
    );

    stream.write(request.as_bytes())?;
    stream.flush()?;

    Ok(stream)
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

fn read_stream(
    connection_stream: TcpStream,
    state: &Arc<Mutex<State>>,
    on_open: &Arc<Mutex<Option<Box<Fn() + Send>>>>,
    on_message: &Arc<Mutex<Option<Box<Fn(String) + Send>>>>,
    on_error: &Arc<Mutex<Option<Box<Fn(String) + Send>>>>
) -> StreamAction {

    let reader = BufReader::new(connection_stream);

    for line in reader.lines() {
        let mut state = state.lock().unwrap();

        let line = match line {
            Ok(l) => l,
            Err(error) => {
                let mut on_error = on_error.lock().unwrap();
                if let Some(ref f) = *on_error {
                    f(error.to_string());
                }
                *state = State::CLOSED;
                return StreamAction::RECONNECT;
            }
        };

        match *state {
            State::CONNECTING => {
                if let StreamAction::RECONNECT = handle_headers(
                    line, &mut state, &on_open, &on_error) {
                    return StreamAction::RECONNECT;
                }
            }
            _ => handle_messages(line, &on_message)
        }
    }

    StreamAction::CONTINUE
}

fn handle_headers(
    line: String,
    state: &mut State,
    on_open: &Arc<Mutex<Option<Box<Fn() + Send>>>>,
    on_error: &Arc<Mutex<Option<Box<Fn(String) + Send>>>>
) -> StreamAction  {
    if line == "" {
        *state = State::OPEN;
        let on_open = on_open.lock().unwrap();
        if let Some(ref f) = *on_open {
            f();
        }
        return StreamAction::CONTINUE
    }
    if !line.starts_with("HTTP/1.1 ") {
        return StreamAction::CONTINUE
    }
    let status = &line[9..];
    if !status.starts_with("200") {
        let on_error = on_error.lock().unwrap();
        if let Some(ref f) = *on_error {
            f(status.to_string());
        }
        *state = State::CLOSED;
        return StreamAction::RECONNECT
    } else {
        return StreamAction::CONTINUE
    }

}

fn handle_messages(line: String, on_message: &Arc<Mutex<Option<Box<Fn(String) + Send>>>>) {
    let on_message = on_message.lock().unwrap();
    if let Some(ref f) = *on_message {
        f(line);
    }
}

fn reconnect_stream(
    url: Arc<Url>,
    stream: Arc<Mutex<Option<TcpStream>>>,
    state: Arc<Mutex<State>>,
    on_open: Arc<Mutex<Option<Box<Fn() + Send>>>>,
    on_message: Arc<Mutex<Option<Box<Fn(String) + Send>>>>,
    on_error: Arc<Mutex<Option<Box<Fn(String) + Send>>>>
) {
    thread::sleep(Duration::from_millis(500));

    let mut state_lock = state.lock().unwrap();
    *state_lock = State::CONNECTING;

    listen_stream(url, stream, Arc::clone(&state), on_open, on_message, on_error);
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

        let _ = rx.recv().unwrap();

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

        let _ = rx.recv().unwrap();

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

    #[test]
    fn should_reconnect_when_connection_closed_by_server() {
        let (tx, rx) = mpsc::channel();
        let (error_tx, error_rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_error(move |message| {
            error_tx.send(message).unwrap();
        });

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.close();

        let _ = error_rx.recv().unwrap();

        let fake_server = fake_server.reconnect();

        fake_server.send("\ndata: some message\n\n");

        let message = rx.recv().unwrap();
        assert_eq!(message, "data: some message");

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_reconnect_when_first_connection_fails() {
        let url = Url::parse("http://localhost:7763/sub").unwrap();
        let mut event_stream = EventStream::new(url).unwrap();


        let (tx, rx) = mpsc::channel();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });


        let fake_server = FakeServer::create("localhost:7763");

        fake_server.send("\ndata: some message\n\n");

        let message = rx.recv().unwrap();
        assert_eq!(message, "data: some message");

        event_stream.close();
        fake_server.close();
    }
}

