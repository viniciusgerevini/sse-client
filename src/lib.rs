extern crate url;

use std::io::prelude::*;
use std::io::BufReader;
use std::thread;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use url::{Url, ParseError};
use std::net::{Shutdown, TcpStream};

mod network;

#[cfg(test)]
mod test_helper;

pub struct EventSource {
    ready_state: Arc<Mutex<State>>,
    listeners: Arc<Mutex<HashMap<String, Vec<Box<Fn(Event) + Send>>>>>,
    on_open_listeners: Arc<Mutex<Vec<Box<Fn() + Send>>>>,
    stream: TcpStream
}

#[derive(Debug, Clone)]
pub struct Event {
    type_: String,
    data: String
}

#[derive(Debug, PartialEq, Clone)]
pub enum State {
    CONNECTING,
    OPEN,
    CLOSED
}

impl EventSource {
    pub fn new(url: &str) -> Result<EventSource, ParseError> {
        let stream = network::open_connection(Url::parse(url)?).unwrap();

        let listeners = Arc::new(Mutex::new(HashMap::new()));
        let on_open_listeners = Arc::new(Mutex::new(vec!()));
        let ready_state = Arc::new(Mutex::new(State::CONNECTING));

        listen_to_stream(
            stream.try_clone().unwrap(),
            Arc::clone(&ready_state),
            Arc::clone(&listeners),
            Arc::clone(&on_open_listeners)
        );

        Ok(EventSource{ ready_state, listeners, stream: stream, on_open_listeners })
    }

    pub fn close(&self) {
        self.stream.shutdown(Shutdown::Both).unwrap();
        let mut state = self.ready_state.lock().unwrap();
        *state = State::CLOSED;
    }

    pub fn on_open<F>(&self, listener: F) where F: Fn() + Send + 'static {
        let mut listeners = self.on_open_listeners.lock().unwrap();
        listeners.push(Box::new(listener));
    }

    pub fn on_message<F>(&self, listener: F) where F: Fn(Event) + Send + 'static {
        self.add_event_listener("message", listener);
    }

    pub fn add_event_listener<F>(&self, event_type: &str, listener: F) where F: Fn(Event) + Send + 'static {
        let mut listeners = self.listeners.lock().unwrap();
        let listener = Box::new(listener);

        if listeners.contains_key(event_type) {
            listeners.get_mut(event_type).unwrap().push(listener);
        } else {
            listeners.insert(String::from(event_type), vec!(listener));
        }
    }

    pub fn state(&self) -> State {
        let state = &self.ready_state.lock().unwrap();
        (*state).clone()
    }
}

fn listen_to_stream(
    stream: TcpStream,
    state: Arc<Mutex<State>>,
    listeners: Arc<Mutex<HashMap<String, Vec<Box<Fn(Event) + Send>>>>>,
    on_open_listeners: Arc<Mutex<Vec<Box<Fn() + Send>>>>
) {
    thread::spawn(move || {
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut pending_event: Option<Event> = None;

        for line in reader.lines() {
            let line = line.unwrap();
            let mut state = state.lock().unwrap();

            match *state {
                State::CONNECTING => *state = handle_stream_header(line, &on_open_listeners),
                _ => pending_event = handle_stream_body(pending_event, line, &listeners)
            }
        }
    });
}

fn handle_stream_header(line: String, listeners: &Arc<Mutex<Vec<Box<Fn() + Send>>>>) -> State {
    if line == "" {
        dispatch_open_event(listeners);
        State::OPEN
    } else {
        State::CONNECTING
    }
}

fn handle_stream_body(
    pending_event: Option<Event>,
    line: String,
    listeners: &Arc<Mutex<HashMap<String, Vec<Box<Fn(Event) + Send>>>>>
) -> Option<Event> {
    let mut event = None;

    if line == "" {
        if let Some(e) = pending_event {
            dispatch_event(listeners, &e);
        }
    } else if !line.starts_with(":") {
        event = update_event(pending_event, line);
    }

    event
}

fn dispatch_event(listeners: &Arc<Mutex<HashMap<String, Vec<Box<Fn(Event) + Send>>>>>, event: &Event) {
    let listeners = listeners.lock().unwrap();
    if listeners.contains_key(&event.type_) {
        for listener in listeners.get(&event.type_).unwrap().iter() {
            listener(event.clone())
        }
    }
}

fn dispatch_open_event(listeners: &Arc<Mutex<Vec<Box<Fn() + Send>>>>) {
    let listeners = listeners.lock().unwrap();
    for listener in listeners.iter() {
        listener()
    }
}

fn update_event(pending_event: Option<Event>, message: String) -> Option<Event> {
    let mut event = match pending_event {
        Some(e) => e.clone(),
        None => Event { type_: String::from("message"), data: String::from("") }
    };

    match parse_field(&message) {
        ("event", value) => event.type_ = String::from(value),
        ("data", value) => event.data = String::from(value),
        _ => ()
    }

    Some(event)
}

fn parse_field<'a>(message: &'a String) -> (&'a str, &'a str) {
    let parts: Vec<&str> = message.split(":").collect();
    (parts[0], parts[1].trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::sync::mpsc;

    use test_helper::fake_server;

    fn setup() -> (EventSource, fake_server::FakeServer) {
        let fake_server = fake_server::FakeServer::new();
        let address = format!("http://{}/sub", fake_server.socket_address());
        let event_source = EventSource::new(address.as_str()).unwrap();

        (event_source, fake_server)
    }

    #[test]
    fn should_create_client() {
        let (event_source, fake_server) = setup();
        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_thrown_an_error_when_malformed_url_provided() {
        match EventSource::new("127.0.0.1:1236/sub") {
            Ok(_) => assert!(false, "should had thrown an error"),
            Err(_) => assert!(true)
        }
    }

    #[test]
    fn accept_closure_as_listeners() {
        let (tx, rx) = mpsc::channel();
        let (event_source, fake_server) = setup();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        fake_server.send("\ndata: some message\n\n");

        let message = rx.recv().unwrap();
        assert_eq!(message, "some message");

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_trigger_listeners_when_message_received() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let (event_source, fake_server) = setup();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });


        event_source.on_message(move |message| {
            tx2.send(message.data).unwrap();
        });

        fake_server.send("\ndata: some message\n\n");

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();

        assert_eq!(message, "some message");
        assert_eq!(message2, "some message");

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_not_trigger_listeners_for_comments() {
        let (tx, rx) = mpsc::channel();

        let (event_source, fake_server) = setup();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        fake_server.send("\n");
        fake_server.send("data: message\n\n");
        fake_server.send(":this is a comment\n");
        fake_server.send(":this is another comment\n");
        fake_server.send("data: this is a message\n\n");

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();

        assert_eq!(message, "message");
        assert_eq!(message2, "this is a message");

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn ensure_stream_is_parsed_after_headers() {
        let (tx, rx) = mpsc::channel();

        let (event_source, fake_server) = setup();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        fake_server.send("HTTP/1.1 200 OK\n");
        fake_server.send("Server: nginx/1.10.3\n");
        fake_server.send("Date: Thu, 24 May 2018 12:26:38 GMT\n");
        fake_server.send("Content-Type: text/event-stream; charset=utf-8\n");
        fake_server.send("Connection: keep-alive\n");
        fake_server.send("\n");
        fake_server.send("data: this is a message\n\n");

        let message = rx.recv().unwrap();

        assert_eq!(message, "this is a message");

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn ignore_empty_messages() {
        let (tx, rx) = mpsc::channel();

        let (event_source, fake_server) = setup();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        fake_server.send("\n");
        fake_server.send("data: message\n");
        fake_server.send("\n");
        fake_server.send("\n");
        fake_server.send("data: this is a message\n\n");

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();

        assert_eq!(message, "message");
        assert_eq!(message2, "this is a message");

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn event_trigger_its_defined_listener() {
        let (tx, rx) = mpsc::channel();

        let (event_source, fake_server) = setup();

        event_source.add_event_listener("myEvent", move |event| {
            tx.send(event).unwrap();
        });

        fake_server.send("\n");
        fake_server.send("event: myEvent\n");
        fake_server.send("data: my message\n\n");

        let message = rx.recv().unwrap();

        assert_eq!(message.type_, String::from("myEvent"));
        assert_eq!(message.data, String::from("my message"));

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn dont_trigger_on_message_for_event() {
        let (tx, rx) = mpsc::channel();
        let (event_source, fake_server) = setup();

        event_source.on_message(move |_| {
            tx.send("NOOOOOOOOOOOOOOOOOOO!").unwrap();
        });

        fake_server.send("\n");
        fake_server.send("event: myEvent\n");
        fake_server.send("data: my message\n\n");

        thread::sleep(Duration::from_millis(500));
        assert!(rx.try_recv().is_err());

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_close_connection() {
        let (tx, rx) = mpsc::channel();

        let (event_source, fake_server) = setup();

         event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        fake_server.send("\ndata: some message\n\n");
        rx.recv().unwrap();
        event_source.close();
        fake_server.send("\ndata: some message\n\n");

        thread::sleep(Duration::from_millis(400));
        assert!(rx.try_recv().is_err());

        fake_server.close();
    }

    #[test]
    fn should_trigger_on_open_callback_when_connected() {
        let (tx, rx) = mpsc::channel();
        let (event_source, fake_server) = setup();

        event_source.on_open(move || {
            tx.send("open").unwrap();
        });

        fake_server.send("HTTP/1.1 200 OK\n");
        fake_server.send("Date: Thu, 24 May 2018 12:26:38 GMT\n");
        fake_server.send("\n");

        rx.recv().unwrap();

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_connecting_while_opening_connection() {
        let (event_source, fake_server) = setup();

        let state = event_source.state();
        assert_eq!(state, State::CONNECTING);

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_open_after_connection_stabilished() {
        let (event_source, fake_server) = setup();

        fake_server.send("\n");
        thread::sleep(Duration::from_millis(200));

        let state = event_source.state();
        assert_eq!(state, State::OPEN);

        event_source.close();
        fake_server.close();
    }

    #[test]
    fn should_have_status_closed_after_closing_connection() {
        let (event_source, fake_server) = setup();

        event_source.close();
        thread::sleep(Duration::from_millis(200));

        let state = event_source.state();
        assert_eq!(state, State::CLOSED);

        fake_server.close();
    }
}
