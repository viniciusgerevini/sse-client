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

/// Client state
#[derive(Debug, PartialEq, Clone)]
pub enum State {
    /// State when trying to connect or reconnect to stream.
    Connecting,
    /// Stream is open.
    Open,
    /// State when connection is closed and client won't try to reconnect.
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
    stream.set_read_timeout(Some(Duration::from_millis(60000)))?;

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

    if let Some(port) = url.port_or_known_default() {
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
    let mut reader = BufReader::new(connection_stream);

    let mut request_header = String::new();
    reader.read_line(&mut request_header).unwrap();

    let status_code = validate_status_code(request_header)?;

    for line in reader.lines() {
        let mut state = state.lock().unwrap();

        let line = line.map_err(|error| {
            StreamAction::Reconnect(error.to_string())
        })?;

        match *state {
            State::Connecting => handle_headers(line.clone(), &mut state, &on_open, status_code, failed_attempts)?,
            _ => handle_messages(line.clone(), &on_message)
        }
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
    status_code: i32,
    failed_attempts: &Arc<Mutex<u32>>
) -> Result<(), StreamAction> {
    if line == "" {
        handle_open_connection(state, on_open, failed_attempts)
    } else if line.starts_with("Content-Type") {
        validate_content_type(line)
    } else if line.starts_with("Location:") {
        handle_new_location(line, status_code)
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

fn validate_status_code(line: String) -> Result<(i32), StreamAction> {
    let status = &line[9..].trim_right();
    let status_code: i32 = status[..3].parse().unwrap();

    match status_code {
        200 | 301 | 302 | 303 | 307 => Ok(status_code),
        204 => Err(StreamAction::Close(status.to_string())),
        200 ... 299 => Err(StreamAction::Reconnect(status.to_string())),
        _ => Err(StreamAction::Close(status.to_string()))
    }
}

fn handle_new_location(line: String, status_code: i32) -> Result<(), StreamAction> {
    let location = &line[10..];

    match status_code {
        301 => Err(StreamAction::MovePermanently(Url::parse(location).unwrap())),
        302 | 303 | 307 => Err(StreamAction::Move(Url::parse(location).unwrap())),
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
    use std::sync::mpsc;
    use std::time::Duration;
    use http_test_server::{ TestServer, Resource };
    use http_test_server::http::Status;

    fn setup() -> (TestServer, Resource, Url) {
        let server = TestServer::new().unwrap();
        let resource = server.create_resource("/sub");
        resource.header("Content-Type", "text/event-stream").stream();
        let address = format!("http://localhost:{}/sub", server.port());
        let url = Url::parse(address.as_str()).unwrap();
        (server, resource, url)
    }


    #[test]
    fn should_create_stream_object() {
        let (_server, _stream_endpoint, address) = setup();
        let event_stream = EventStream::new(address).unwrap();
        event_stream.close();
    }

    #[test]
    fn should_trigger_on_open_listener() {
        let (tx, rx) = mpsc::channel();

        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.send("Date: Thu, 24 May 2018 12:26:38 GMT\n");

        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_open(move || {
            tx.send("open").unwrap();
        });

        let _ = rx.recv().unwrap();

        event_stream.close();
    }

    #[test]
    fn should_have_status_connecting_while_opening_connection() {
        let (_server, _stream_endpoint, address) = setup();
        let event_stream = EventStream::new(address).unwrap();

        let state = event_stream.state();
        assert_eq!(state, State::Connecting);

        event_stream.close();
    }

    #[test]
    fn should_have_status_open_after_connection_stabilished() {
        let (_server, _stream_endpoint, address) = setup();
        let event_stream = EventStream::new(address).unwrap();

        thread::sleep(Duration::from_millis(100));
        let state = event_stream.state();
        assert_eq!(state, State::Open);

        event_stream.close();
    }

    #[test]
    fn should_have_status_closed_after_closing_connection() {
        let (_server, _stream_endpoint, address) = setup();
        let event_stream = EventStream::new(address).unwrap();

        event_stream.close();

        let state = event_stream.state();
        assert_eq!(state, State::Closed);
    }

    #[test]
    fn should_trigger_listeners_when_message_received() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        while event_stream.state() == State::Connecting {}

        stream_endpoint.send("data: some message\n\n");

        let message = rx.recv().unwrap();

        assert_eq!(message, "data: some message");

        event_stream.close();
    }

    #[test]
    fn should_trigger_on_error_when_connection_closed_by_server() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_error(move |message| {
            println!("{}", message);
            tx.send(message).unwrap();
        });

        while event_stream.state() == State::Connecting {};

        stream_endpoint.close_open_connections();

        let message = rx.recv().unwrap();

        assert!(message.contains("connection closed by server"));
    }

    #[test]
    fn should_trigger_error_when_status_code_is_not_success() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::InternalServerError);
        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_error(move |message| {
            tx.send(message).unwrap();
        });

        let message = rx.recv().unwrap();

        assert_eq!(message, "500 Internal Server Error");

        event_stream.close();
    }

    #[test]
    fn should_reconnect_when_connection_closed_by_server() {
        let (tx, rx) = mpsc::channel();
        let (server, stream_endpoint, address) = setup();
        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        stream_endpoint.close_open_connections();

        server.requests().recv().unwrap();

        while event_stream.state() != State::Open {}

        stream_endpoint.send("data: some message\n\n");

        assert_eq!(rx.recv().unwrap(), "data: some message");

        event_stream.close();
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
        let (server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::ResetContent);

        let event_stream = EventStream::new(address).unwrap();

        server.requests().recv().unwrap();
        server.requests().recv().unwrap();

        assert!(stream_endpoint.request_count() >= 2);

        event_stream.close();
    }

    #[test]
    fn should_close_connection_when_content_type_is_not_event_stream() {
        let (error_tx, error_rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.header("Content-Type", "application/json");
        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_error(move |message| {
            error_tx.send(message).unwrap();
        });


        let message = error_rx.recv().unwrap();
        assert_eq!(message, "Wrong Content-Type");

        let state = event_stream.state();
        assert_eq!(state, State::Closed);
    }

    #[test]
    fn should_reconnect_when_status_in_range_2xx_but_not_200() {
        let (server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::Accepted);
        let event_stream = EventStream::new(address).unwrap();

        server.requests().recv().unwrap();
        server.requests().recv().unwrap();

        assert!(stream_endpoint.request_count() >= 2);

        event_stream.close();
    }

    #[test]
    fn should_close_connection_when_returned_any_status_not_handled_in_previous_scenarios() {
        let (error_tx, error_rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::InternalServerError);
        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_error(move |message| {
            error_tx.send(message).unwrap();
        });

        let message = error_rx.recv().unwrap();
        assert_eq!(message, "500 Internal Server Error");

        let state = event_stream.state();
        assert_eq!(state, State::Closed);
    }

    #[test]
    fn should_connect_to_provided_host_when_status_302() {
        let (_server, stream_endpoint, address) = setup();
        let (_server2, stream_endpoint2, address2) = setup();
        stream_endpoint
            .status(Status::Found)
            .header("Location", address2.as_str());

        let event_stream = EventStream::new(address).unwrap();

        while stream_endpoint2.open_connections_count() != 1 {
            thread::sleep(Duration::from_millis(200));
        }

        event_stream.close();
    }

    #[test]
    fn should_reconnect_to_original_host_when_connection_to_redirected_host_is_lost() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        let (_server2, stream_endpoint2, address2) = setup();
        stream_endpoint
            .status(Status::Found)
            .header("Location", address2.as_str());

        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        stream_endpoint.status(Status::OK);
        stream_endpoint2.close_open_connections();

        while event_stream.state() != State::Open {}

        stream_endpoint.send("data: from server 1\n\n");
        let message = rx.recv().unwrap();
        assert_eq!(message, "data: from server 1");

        event_stream.close();
    }

    #[test]
    fn should_reconnect_to_new_host_when_connection_lost_after_moved_permanently() {
        let (_server, stream_endpoint, address) = setup();
        let (_server2, stream_endpoint2, address2) = setup();
        stream_endpoint
            .status(Status::MovedPermanently)
            .header("Location", address2.as_str());

        let mut event_stream = EventStream::new(address).unwrap();

        let (tx, rx) = mpsc::channel();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        stream_endpoint2
            .delay(Duration::from_millis(200))
            .close_open_connections();

        while event_stream.state() != State::Connecting {}
        while event_stream.state() != State::Open {}

        stream_endpoint2.send("data: from server 2\n");
        assert_eq!(rx.recv().unwrap(), "data: from server 2");

        event_stream.close();
    }

    #[test]
    fn should_connect_to_new_host_when_status_303() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        let (_server2, stream_endpoint2, address2) = setup();

        stream_endpoint
            .status(Status::SeeOther)
            .header("Location", address2.as_str());

        let mut event_stream = EventStream::new(address).unwrap();

        event_stream.on_message(move |message| {
            tx.send(message).unwrap();
        });

        while stream_endpoint2.open_connections_count() == 0 {
            thread::sleep(Duration::from_millis(100));
        }

        stream_endpoint2.send("data: from server 2\n\n");
        let message = rx.recv().unwrap();
        assert_eq!(message, "data: from server 2");

        event_stream.close();
    }

    #[test]
    fn should_connect_to_new_host_when_status_307() {
        let (_server, stream_endpoint, address) = setup();
        let (_server2, stream_endpoint2, address2) = setup();

        stream_endpoint
            .delay(Duration::from_millis(200))
            .status(Status::TemporaryRedirect)
            .header("Location", address2.as_str());

        let event_stream = EventStream::new(address).unwrap();

        while stream_endpoint2.open_connections_count() == 0 {
            thread::sleep(Duration::from_millis(100));
        }

        event_stream.close();
    }

    #[test]
    fn should_stop_reconnection_when_status_204() {
        let (server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::Accepted);

        let event_stream = EventStream::new(address).unwrap();

        assert_eq!(event_stream.state(), State::Connecting);

        stream_endpoint.status(Status::NoContent);
        server.requests().recv().unwrap();

        assert_eq!(event_stream.state(), State::Closed);

        event_stream.close();
    }


    #[test]
    fn should_try_to_reconnect_with_an_exponential_backoff() {
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::Accepted);
        let event_stream = EventStream::new(address).unwrap();

        for _ in 0 .. 20 {
            thread::sleep(Duration::from_millis(100))
        }

        let retries_in_first_two_seconds = stream_endpoint.request_count();

        for _ in 0 .. 20 {
            thread::sleep(Duration::from_millis(100))
        }

        let retries_in_the_next_two_seconds = stream_endpoint.request_count() - retries_in_first_two_seconds;

        assert!(retries_in_first_two_seconds > retries_in_the_next_two_seconds);

        event_stream.close();
    }

    #[test]
    fn should_reset_exponential_backoff_after_success_connection() {
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.status(Status::Accepted);
        let event_stream = EventStream::new(address).unwrap();

        for _ in 0 .. 20 {
            thread::sleep(Duration::from_millis(100))
        }

        let retries_in_first_two_seconds = stream_endpoint.request_count();

        stream_endpoint.status(Status::OK);

        while event_stream.state() != State::Open {
            thread::sleep(Duration::from_millis(100));
        }

        stream_endpoint.status(Status::Accepted);
        stream_endpoint.close_open_connections();

        for _ in 0 .. 20 {
            thread::sleep(Duration::from_millis(100))
        }

        let retries_in_the_next_two_seconds = stream_endpoint.request_count() - retries_in_first_two_seconds;

        assert_eq!(retries_in_first_two_seconds, retries_in_the_next_two_seconds);

        event_stream.close();
    }

    #[test]
    fn should_use_port_80_as_default_when_no_port_provided() {
        let url = Url::parse("http://localhost").unwrap();
        let host = get_host(&url);

        assert_eq!(host, String::from("localhost:80"));
    }

    #[test]
    fn should_use_port_443_as_default_when_https_and_no_port_provided() {
        let url = Url::parse("https://localhost").unwrap();
        let host = get_host(&url);

        assert_eq!(host, String::from("localhost:443"));
    }
}

