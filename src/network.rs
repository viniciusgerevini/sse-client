use std::io::prelude::*;
use std::io::Error;
use std::net::{Shutdown, TcpStream};
use url::Url;
use std::thread;
use std::sync::Arc;
use std::sync::Mutex;
use std::io::BufReader;
use std::time::Duration;

type Callback = Arc<Mutex<Option<Box<Fn(String) + Send>>>>;
type CallbackNoArgs = Arc<Mutex<Option<Box<Fn() + Send>>>>;
type StreamWrapper = Arc<Mutex<Option<TcpStream>>>;
type StateWrapper = Arc<Mutex<State>>;

#[derive(Debug, PartialEq, Clone)]
pub enum State {
    CONNECTING,
    OPEN,
    CLOSED
}

#[derive(Debug, PartialEq)]
enum StreamAction {
    RECONNECT,
    CLOSE
}

pub struct EventStream {
    url: Arc<Url>,
    stream: StreamWrapper,
    state: StateWrapper,
    on_open_listener: CallbackNoArgs,
    on_message_listener: Callback,
    on_error_listener: Callback
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
    stream: StreamWrapper,
    state: StateWrapper,
    on_open: CallbackNoArgs,
    on_message: Callback,
    on_error: Callback
) {
    thread::spawn(move || {
        let action = match connect_event_stream(&url, &stream) {
            Ok(stream) => read_stream(stream, &state, &on_open, &on_message, &on_error),
            Err(error) => {
                let mut state = state.lock().unwrap();
                handle_error(error.to_string(), &mut state, &on_error);
                Err(StreamAction::RECONNECT)
            }
        };

        if let Err(StreamAction::RECONNECT) = action  {
            reconnect_stream(url, stream, state, on_open, on_message, on_error)
        }
    });
}

fn connect_event_stream(url: &Url, stream: &StreamWrapper) -> Result<TcpStream, Error> {
    let connection_stream = event_stream_handshake(url)?;

    let mut stream = stream.lock().unwrap();
    *stream = Some(connection_stream.try_clone().unwrap());

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
    state: &StateWrapper,
    on_open: &CallbackNoArgs,
    on_message: &Callback,
    on_error: &Callback
) -> Result<(), StreamAction> {
    let reader = BufReader::new(connection_stream);

    for line in reader.lines() {
        let mut state = state.lock().unwrap();

        let line = line.map_err(|error| {
            handle_error(error.to_string(), &mut state, on_error);
            StreamAction::RECONNECT
        })?;

        match *state {
            State::CONNECTING => handle_headers(line, &mut state, &on_open, &on_error)?,
            _ => handle_messages(line, &on_message)
        }
    }

    Ok(())
}

fn handle_headers(
    line: String,
    state: &mut State,
    on_open: &CallbackNoArgs,
    on_error: &Callback
) -> Result<(), StreamAction> {
    if line == "" {
        *state = State::OPEN;
        let on_open = on_open.lock().unwrap();
        if let Some(ref f) = *on_open {
            f();
        }
        return Ok(())
    }

    if line.starts_with("Content-Type") {
        if !line.contains("text/event-stream") {
            handle_error(String::from("Wrong Content-Type"), state, on_error);
            return Err(StreamAction::CLOSE)
        }
    }

    if !line.starts_with("HTTP/1.1 ") {
        return Ok(())
    }
    let status = &line[9..];
    if !status.starts_with("200") {
        handle_error(status.to_string(), state, on_error);
        return Err(StreamAction::RECONNECT)
    } else {
        return Ok(())
    }
}

fn handle_messages(line: String, on_message: &Callback) {
    let on_message = on_message.lock().unwrap();
    if let Some(ref f) = *on_message {
        f(line);
    }
}

fn handle_error(message: String, state: &mut State, on_error: &Callback) {
    let on_error = on_error.lock().unwrap();
    if let Some(ref f) = *on_error {
        f(message);
    }
    *state = State::CLOSED;
}

fn reconnect_stream(
    url: Arc<Url>,
    stream: StreamWrapper,
    state: StateWrapper,
    on_open: CallbackNoArgs,
    on_message: Callback,
    on_error: Callback
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

        thread::sleep(Duration::from_millis(100));

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
        let (mut event_stream, fake_server) = setup();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.close();

        loop {
            thread::sleep(Duration::from_millis(400));
            fake_server.send("\ndata: some message\n\n");
            if let Ok(message) = rx.try_recv() {
                assert_eq!(message, "data: some message");
                break;
            }
        }

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

        thread::sleep(Duration::from_millis(500));

        let fake_server = FakeServer::create("localhost:7763");
        thread::sleep(Duration::from_millis(100));

        fake_server.send("\n");
        fake_server.send("data: some message\n");

        let message = rx.recv().unwrap();
        assert_eq!(message, "data: some message");

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_trigger_error_when_first_connection_fails() {
        let url = Url::parse("http://localhost:7777/sub").unwrap();
        let mut event_stream = EventStream::new(url).unwrap();

        let (tx, rx) = mpsc::channel();

        event_stream.on_error(move |message| {
            tx.send(message).unwrap();
        });

        let message = rx.recv().unwrap();
        assert!(message.contains("Connection refused"));

        event_stream.close();
    }

    #[test]
    fn should_reset_connection_on_status_204() {
        let (error_tx, error_rx) = mpsc::channel();
        let (mut event_stream, mut fake_server) = setup();

        let number_of_retries = Arc::new(Mutex::new(0));
        let l = Arc::clone(&number_of_retries);

        fake_server.on_client_message(move |message| {
            if message.starts_with("GET") {
                let mut number_of_retries = l.lock().unwrap();
                *number_of_retries += 1;
            }
        });

        event_stream.on_error(move |message| {
            error_tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 204 No Content\n");

        let message = error_rx.recv().unwrap();
        assert_eq!(message, "204 No Content");

        loop {
            let number_of_retries = number_of_retries.lock().unwrap();
            if *number_of_retries == 2 {
                break;
            }
        }

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_reset_connection_on_status_205() {
        let (event_stream, mut fake_server) = setup();

        let number_of_retries = Arc::new(Mutex::new(0));
        let l = Arc::clone(&number_of_retries);

        fake_server.on_client_message(move |message| {
            if message.starts_with("GET") {
                let mut number_of_retries = l.lock().unwrap();
                *number_of_retries += 1;
            }
        });

        fake_server.send("HTTP/1.1 205 Reset Content\n");

        loop {
            let number_of_retries = number_of_retries.lock().unwrap();
            if *number_of_retries == 2 {
                break;
            }
        }

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_close_connection_when_content_type_is_not_event_stream() {
        let (error_tx, error_rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_error(move |message| {
            error_tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 200 OK\n");
        fake_server.send("Content-Type: text/html\n");

        let message = error_rx.recv().unwrap();
        assert_eq!(message, "Wrong Content-Type");

        thread::sleep(Duration::from_millis(1000));

        let state = event_stream.state();
        assert_eq!(state, State::CLOSED);

        fake_server.close();
    }

    #[test]
    fn should_reconnect_when_status_in_range_2xx_but_not_200() {
        let (event_stream, mut fake_server) = setup();

        let number_of_retries = Arc::new(Mutex::new(0));
        let l = Arc::clone(&number_of_retries);

        fake_server.on_client_message(move |message| {
            if message.starts_with("GET") {
                let mut number_of_retries = l.lock().unwrap();
                *number_of_retries += 1;
            }
        });

        fake_server.send("HTTP/1.1 202 Accepted\n");

        loop {
            let number_of_retries = number_of_retries.lock().unwrap();
            if *number_of_retries == 2 {
                break;
            }
        }

        event_stream.close();
        fake_server.close();
    }
}

