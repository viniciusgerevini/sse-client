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
type LastIdWrapper = Arc<Mutex<Option<String>>>;

const INITIAL_RECONNECTION_TIME_IN_MS: u64 = 500;

#[derive(Debug, PartialEq, Clone)]
pub enum State {
    Connecting,
    Open,
    Closed
}

#[derive(Debug, PartialEq)]
enum StreamAction {
    Reconnect(String),
    Close(String),
    Move(Url),
    MovePermanently(Url)
}

pub struct EventStream {
    url: Arc<Url>,
    stream: StreamWrapper,
    state: StateWrapper,
    on_open_listener: CallbackNoArgs,
    on_message_listener: Callback,
    on_error_listener: Callback,
    last_event_id: LastIdWrapper
}

impl EventStream {
    pub fn new(url: Url) -> Result<EventStream, Error> {
        let event_stream = EventStream {
            url: Arc::new(url),
            stream: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(State::Connecting)),
            on_open_listener: Arc::new(Mutex::new(None)),
            on_message_listener: Arc::new(Mutex::new(None)),
            on_error_listener: Arc::new(Mutex::new(None)),
            last_event_id: Arc::new(Mutex::new(None))
        };

        event_stream.listen();

        Ok(event_stream)
    }

    fn listen(&self) {
        listen_stream(
            Arc::clone(&self.url),
            Arc::clone(&self.url),
            Arc::clone(&self.stream),
            Arc::clone(&self.state),
            Arc::clone(&self.on_open_listener),
            Arc::clone(&self.on_message_listener),
            Arc::clone(&self.on_error_listener),
            Arc::new(Mutex::new(0)),
            Arc::clone(&self.last_event_id)
        );
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        *state = State::Closed;
        if let Some(ref st) = *self.stream.lock().unwrap() {
            st.shutdown(Shutdown::Both).unwrap();
        }
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

    pub fn set_last_id(&self, id: String) {
        let mut last_id = self.last_event_id.lock().unwrap();
        *last_id = Some(id);
    }
}

fn listen_stream(
    url: Arc<Url>,
    connection_url: Arc<Url>,
    stream: StreamWrapper,
    state: StateWrapper,
    on_open: CallbackNoArgs,
    on_message: Callback,
    on_error: Callback,
    failed_attempts: Arc<Mutex<u32>>,
    last_event_id: LastIdWrapper
) {
    thread::spawn(move || {
        let action = match connect_event_stream(&connection_url, &stream, &last_event_id) {
            Ok(stream) => read_stream(stream, &state, &on_open, &on_message, &failed_attempts),
            Err(error) => Err(StreamAction::Reconnect(error.to_string()))
        };

        if let Err(stream_action) = action  {
            match stream_action {
                StreamAction::Reconnect(ref error) => {
                    let mut state_lock = state.lock().unwrap();
                    *state_lock = State::Connecting;
                    handle_error(error.to_string(),  &on_error);
                    reconnect_stream(url, stream, Arc::clone(&state), on_open, on_message, on_error, failed_attempts, last_event_id);
                },
                StreamAction::Close(ref error) => {
                    let mut state_lock = state.lock().unwrap();
                    *state_lock = State::Closed;
                    handle_error(error.to_string(), &on_error);
                },
                StreamAction::Move(redirect_url) => {
                    let mut state_lock = state.lock().unwrap();
                    *state_lock = State::Connecting;

                    listen_stream(url, Arc::new(redirect_url), stream, Arc::clone(&state), on_open, on_message, on_error, failed_attempts, last_event_id);
                },
                StreamAction::MovePermanently(redirect_url) => {
                    let mut state_lock = state.lock().unwrap();
                    *state_lock = State::Connecting;

                    listen_stream(Arc::new(redirect_url.clone()), Arc::new(redirect_url), stream, Arc::clone(&state), on_open, on_message, on_error, failed_attempts, last_event_id);
                }
            };
        }
    });
}

fn connect_event_stream(url: &Url, stream: &StreamWrapper, last_event_id: &LastIdWrapper) -> Result<TcpStream, Error> {
    let connection_stream = event_stream_handshake(url, last_event_id)?;
    let mut stream = stream.lock().unwrap();
    *stream = Some(connection_stream.try_clone().unwrap());

    Ok(connection_stream)
}

fn event_stream_handshake(url: &Url, last_event_id: &LastIdWrapper) -> Result<TcpStream, Error> {
    let path = url.path();
    let host = get_host(&url);
    let host = host.as_str();

    let mut stream = TcpStream::connect(host)?;

    let extra_headers = match *(last_event_id.lock().unwrap()) {
        Some(ref last_id) => format!("Last-Event-ID: {}\r\n", last_id),
        None => String::from("")
    };

    let request = format!(
        "GET {} HTTP/1.1\r\nAccept: text/event-stream\r\nHost: {}\r\n{}\r\n",
        path,
        host,
        extra_headers
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
    failed_attempts: &Arc<Mutex<u32>>
) -> Result<(), StreamAction> {
    let reader = BufReader::new(connection_stream);
    let mut previous_line = String::from("nothing");

    for line in reader.lines() {
        let mut state = state.lock().unwrap();

        let line = line.map_err(|error| {
            StreamAction::Reconnect(error.to_string())
        })?;

        match *state {
            State::Connecting => handle_headers(line.clone(), &mut state, &on_open, &previous_line, failed_attempts)?,
            _ => handle_messages(line.clone(), &on_message)
        }
        previous_line = line;
    }

    match *(state.lock().unwrap()) {
        State::Closed => Ok(()),
        _ => Err(StreamAction::Reconnect(String::from("connection closed by server")))
    }
}

fn handle_headers(
    line: String,
    state: &mut State,
    on_open: &CallbackNoArgs,
    previous_line: &str,
    failed_attempts: &Arc<Mutex<u32>>
) -> Result<(), StreamAction> {
    if line == "" {
        handle_open_connection(state, on_open, failed_attempts)
    } else if line.starts_with("Content-Type") {
        validate_content_type(line)
    } else if line.starts_with("HTTP/1.1 ") {
        validate_status_code(line)
    } else if line.starts_with("Location:") {
        handle_new_location(line, previous_line)
    } else {
        Ok(())
    }
}

fn handle_open_connection(state: &mut State, on_open: &CallbackNoArgs, failed_attempts: &Arc<Mutex<u32>>) -> Result<(), StreamAction> {
    *state = State::Open;
    let on_open = on_open.lock().unwrap();
    if let Some(ref f) = *on_open {
        f();
    }
    let mut failed_attempts = failed_attempts.lock().unwrap();
    *failed_attempts = 0;
    Ok(())
}

fn validate_content_type(line: String) -> Result<(), StreamAction> {
    if line.contains("text/event-stream") {
        Ok(())
    } else {
        Err(StreamAction::Close(String::from("Wrong Content-Type")))
    }
}

fn validate_status_code(line: String) -> Result<(), StreamAction> {
    let status = &line[9..];
    let status_code: i32 = status[..3].parse().unwrap();

    match status_code {
        200 | 301 | 302 | 303 | 307 => Ok(()),
        204 => Err(StreamAction::Close(status.to_string())),
        200 ... 299 => Err(StreamAction::Reconnect(status.to_string())),
        _ => Err(StreamAction::Close(status.to_string()))
    }
}

fn handle_new_location(line: String, previous_line: &str) -> Result<(), StreamAction> {
    let status_code = &previous_line[9..12];
    let location = &line[10..];

    match status_code {
        "301" => Err(StreamAction::MovePermanently(Url::parse(location).unwrap())),
        "302" | "303" | "307" => Err(StreamAction::Move(Url::parse(location).unwrap())),
        _ => Ok(())
    }
}

fn handle_messages(line: String, on_message: &Callback) {
    let on_message = on_message.lock().unwrap();
    if let Some(ref f) = *on_message {
        f(line);
    }
}

fn handle_error(message: String, on_error: &Callback) {
    let on_error = on_error.lock().unwrap();
    if let Some(ref f) = *on_error {
        f(message);
    }
}

fn reconnect_stream(
    url: Arc<Url>,
    stream: StreamWrapper,
    state: StateWrapper,
    on_open: CallbackNoArgs,
    on_message: Callback,
    on_error: Callback,
    failed_attempts: Arc<Mutex<u32>>,
    last_event_id: LastIdWrapper
) {
    let mut attempts = failed_attempts.lock().unwrap();
    let base: u64 = 2;
    let reconnection_time = INITIAL_RECONNECTION_TIME_IN_MS + (15 * (base.pow(*attempts) - 1));
    *attempts += 1;

    thread::sleep(Duration::from_millis(reconnection_time));
    listen_stream(url.clone(), url, stream, Arc::clone(&state), on_open, on_message, on_error, Arc::clone(&failed_attempts), last_event_id);
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
        assert_eq!(state, State::Connecting);

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_open_after_connection_stabilished() {
        let (event_stream, fake_server) = setup();

        fake_server.send("\n");
        thread::sleep(Duration::from_millis(200));
        let state = event_stream.state();
        assert_eq!(state, State::Open);

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_closed_after_closing_connection() {
        let (event_stream, fake_server) = setup();

        event_stream.close();
        thread::sleep(Duration::from_millis(200));

        let state = event_stream.state();
        assert_eq!(state, State::Closed);

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

        fake_server.break_current_connection();

        loop {
            thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS));
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

        thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS));

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

        thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS));

        let state = event_stream.state();
        assert_eq!(state, State::Closed);

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

    #[test]
    fn should_close_connection_when_returned_any_status_not_handled_in_privous_scenarios() {
        let (error_tx, error_rx) = mpsc::channel();
        let (mut event_stream, fake_server) = setup();

        event_stream.on_error(move |message| {
            error_tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 500 Internal Server Error\n");

        let message = error_rx.recv().unwrap();
        assert_eq!(message, "500 Internal Server Error");

        thread::sleep(Duration::from_millis(1000));

        let state = event_stream.state();
        assert_eq!(state, State::Closed);

        fake_server.close();
    }

    #[test]
    fn should_connect_to_provided_host_when_status_302() {
        let (event_stream, fake_server) = setup();
        let mut fake_server_2 = FakeServer::create("localhost:65444");

        let (redirect_server_tx, redirect_server_rx) = mpsc::channel();

        fake_server.send("HTTP/1.1 302 Found\n");
        fake_server.send("Location: http://localhost:65444/sub\n");

        thread::sleep(Duration::from_millis(200));

        fake_server_2.on_client_message(move |message| {
            if message.starts_with("GET") {
                redirect_server_tx.send("connection open with second server").unwrap();
            }
        });

        assert_eq!(redirect_server_rx.recv().unwrap(), "connection open with second server");

        event_stream.close();
        fake_server.close();
        fake_server_2.close();
    }

    #[test]
    fn should_reconnect_to_original_host_when_connection_to_redirected_host_is_lost() {
        let (mut event_stream, fake_server) = setup();

        let (tx, rx) = mpsc::channel();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 302 Found\n");
        fake_server.send("Location: http://localhost:6544/sub\n");

        thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS + 100));

        fake_server.send("HTTP/1.1 200 Ok\n\n");
        fake_server.send("data: some message\n\n");

        let message = rx.recv().unwrap();
        assert_eq!(message, "data: some message");

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_reconnect_to_new_host_when_connection_lost_after_moved_permanently() {
        let (mut event_stream, fake_server) = setup();
        let fake_server_2 = FakeServer::create("localhost:60444");

        let (tx, rx) = mpsc::channel();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 301 Moved Permanently\n");
        fake_server.send("Location: http://localhost:60444/sub\n");

        thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS + 100));

        fake_server_2.close();

        loop {
            thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS));
            fake_server_2.send("\ndata: from server 2\n\n");
            if let Ok(message) = rx.try_recv() {
                assert_eq!(message, "data: from server 2");
                break;
            }
        }

        event_stream.close();
        fake_server_2.close();
        fake_server.close();
    }

    #[test]
    fn should_connect_to_new_host_when_status_303() {
        let (mut event_stream, fake_server) = setup();
        let fake_server_2 = FakeServer::create("localhost:60445");

        let (tx, rx) = mpsc::channel();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 303 See Other\n");
        fake_server.send("Location: http://localhost:60445/sub\n");

        loop {
            thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS));
            fake_server_2.send("\ndata: from server 2\n\n");
            if let Ok(message) = rx.try_recv() {
                assert_eq!(message, "data: from server 2");
                break;
            }
        }

        event_stream.close();
        fake_server_2.close();
        fake_server.close();
    }

    #[test]
    fn should_connect_to_new_host_when_status_307() {
        let (mut event_stream, fake_server) = setup();
        let fake_server_2 = FakeServer::create("localhost:60446");

        let (tx, rx) = mpsc::channel();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        fake_server.send("HTTP/1.1 307 Temporary Redirect\n");
        fake_server.send("Location: http://localhost:60446/sub\n");

        loop {
            thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS));
            fake_server_2.send("\ndata: from server 2\n\n");
            if let Ok(message) = rx.try_recv() {
                assert_eq!(message, "data: from server 2");
                break;
            }
        }

        event_stream.close();
        fake_server_2.close();
        fake_server.close();
    }

    #[test]
    fn should_stop_reconnection_when_status_204() {
        let (event_stream, fake_server) = setup();

        fake_server.send("HTTP/1.1 202 Accepted\n");

        assert_eq!(event_stream.state(), State::Connecting);

        thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS + 100));
        fake_server.send("HTTP/1.1 204 No Content\n");

        thread::sleep(Duration::from_millis(100));
        assert_eq!(event_stream.state(), State::Closed);

        event_stream.close();
        fake_server.close();
    }


    #[test]
    fn should_try_to_reconnect_with_an_exponential_backoff() {
        let (event_stream, mut fake_server) = setup();

        let number_of_retries = Arc::new(Mutex::new(0));
        let l = Arc::clone(&number_of_retries);

        fake_server.on_client_message(move |message| {
            if message.starts_with("GET") {
                let mut number_of_retries = l.lock().unwrap();
                *number_of_retries += 1;
            }
        });

        for _ in 0 .. 20 {
            fake_server.send("HTTP/1.1 202 Accepted\n");
            thread::sleep(Duration::from_millis(100));
        }

        let retries_in_first_two_seconds;
        {
            retries_in_first_two_seconds = *(number_of_retries.lock().unwrap());
        }

        for _ in 0 .. 20 {
            fake_server.send("HTTP/1.1 202 Accepted\n");
            thread::sleep(Duration::from_millis(100));
        }

        let retries_in_the_next_two_seconds;
        {
            retries_in_the_next_two_seconds = *(number_of_retries.lock().unwrap()) - retries_in_first_two_seconds;
        }

        assert!(retries_in_first_two_seconds > retries_in_the_next_two_seconds);

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_reset_exponential_backoff_after_success_connection() {
        let (event_stream, mut fake_server) = setup();

        let number_of_retries = Arc::new(Mutex::new(0));
        let l = Arc::clone(&number_of_retries);

        fake_server.on_client_message(move |message| {
            if message.starts_with("GET") {
                let mut number_of_retries = l.lock().unwrap();
                *number_of_retries += 1;
            }
        });

        for _ in 0 .. 20 {
            fake_server.send("HTTP/1.1 202 Accepted\n");
            thread::sleep(Duration::from_millis(100));
        }

        let retries_in_first_two_seconds;
        {
            retries_in_first_two_seconds = *(number_of_retries.lock().unwrap());
        }

        while event_stream.state() != State::Open {
            fake_server.send("HTTP/1.1 200 Ok\n\n");
            thread::sleep(Duration::from_millis(100));
        }

        fake_server.break_current_connection();

        for _ in 0 .. 20 {
            fake_server.send("HTTP/1.1 202 Accepted\n");
            thread::sleep(Duration::from_millis(100));
        }

        let retries_in_the_next_two_seconds;
        {
            retries_in_the_next_two_seconds = *(number_of_retries.lock().unwrap()) - retries_in_first_two_seconds;
        }

        assert_eq!(retries_in_first_two_seconds, retries_in_the_next_two_seconds);

        event_stream.close();
        fake_server.close();
    }

    #[test]
    fn should_send_last_event_id_on_reconnection() {
        let (event_stream, mut fake_server) = setup();
        let expected_header = Arc::new(Mutex::new(None));

        event_stream.set_last_id(String::from("123abc"));

        let header = Arc::clone(&expected_header);
        fake_server.on_client_message(move |message| {
            if message.starts_with("Last-Event-ID") {
                let mut header = header.lock().unwrap();
                *header = Some(message);
            }
        });

        fake_server.send("HTTP/1.1 202 Accepted\n");
        thread::sleep(Duration::from_millis(INITIAL_RECONNECTION_TIME_IN_MS + 100));

        let expected_header = expected_header.lock().unwrap();

        if let Some(ref header) = *expected_header {
            assert_eq!(header, "Last-Event-ID: 123abc");
        } else {
            panic!("should contain last id header");
        }

        event_stream.close();
        fake_server.close();
    }
}

