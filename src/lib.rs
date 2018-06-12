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


pub struct EventSource {
    pub ready_state: Arc<Mutex<State>>,
    listeners: Arc<Mutex<HashMap<String, Vec<Box<Fn(Event) + Send>>>>>,
    on_open_listeners: Arc<Mutex<Vec<Box<Fn() + Send>>>>,
    stream: TcpStream
}

#[derive(Debug, Clone)]
pub struct Event {
    type_: String,
    data: String
}

#[derive(Debug, PartialEq)]
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
        let event_source = EventSource{
            ready_state: Arc::new(Mutex::new(State::CONNECTING)),
            listeners,
            stream: stream.try_clone().unwrap(),
            on_open_listeners
        };

        event_source.start(stream)?;

        Ok(event_source)
    }

    fn start(&self, stream: TcpStream) -> Result<(), ParseError> {
        let reader = BufReader::new(stream.try_clone().unwrap());
        let on_open_listeners = Arc::clone(&self.on_open_listeners);
        let state = Arc::clone(&self.ready_state);
        let listeners = Arc::clone(&self.listeners);

        thread::spawn(move || {
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

        Ok(())
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

    use std::net::TcpListener;
    use std::thread;
    use std::sync::mpsc;

    fn fake_server(address: String) -> std::sync::mpsc::Sender<&'static str>  {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let listener = TcpListener::bind(address).unwrap();

            for stream in listener.incoming() {
                let mut stream = stream.unwrap();

                for received in rx {
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

        tx
    }

    #[test]
    fn should_create_client() {
        let tx = fake_server(String::from("127.0.0.1:6969"));
        let event_source = EventSource::new("http://127.0.0.1:6969/sub").unwrap();
        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_thrown_an_error_when_malformed_url_provided() {
        match EventSource::new("127.0.0.1:1236/sub") {
            Ok(_) => assert!(false, "should had thrown an error"),
            Err(_) => assert!(true)
        }
    }

    #[test]
    fn should_register_listeners() {
        let tx = fake_server(String::from("127.0.0.1:6961"));
        let event_source = EventSource::new("http://127.0.0.1:6961/sub").unwrap();

        event_source.on_message(|_| {});
        event_source.on_message(|_| {});

        let listeners = event_source.listeners.lock().unwrap();

        if let Some(l) = listeners.get("message") {
            assert_eq!(l.len(), 2)
        } else {
            panic!("should contain listeners")
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn accept_closure_as_listeners() {
        let tx = fake_server(String::from("127.0.0.1:6962"));
        let event_source = EventSource::new("http://127.0.0.1:6962/sub").unwrap();

        let something = "s";

        event_source.on_message(move |_| {
            println!("{}", something);
        });

        let listeners = event_source.listeners.lock().unwrap();

        if let Some(l) = listeners.get("message") {
            assert_eq!(l.len(), 1)
        } else {
            panic!("should contain listeners")
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_trigger_listeners_when_message_received() {
        static mut CALL_COUNT: i32 = 0;
        static mut IS_RIGHT_MESSAGE: bool = false;

        let tx = fake_server(String::from("127.0.0.1:6963"));
        let event_source = EventSource::new("http://127.0.0.1:6963/sub").unwrap();

        event_source.on_message(|message| {
            unsafe {
                CALL_COUNT += 1;
                IS_RIGHT_MESSAGE = message.data == "some message";
            }
        });

        tx.send("\ndata: some message\n\n").unwrap();

        unsafe {
            let mut retry_count = 0;
            while CALL_COUNT == 0 && retry_count < 5 {
              thread::sleep(Duration::from_millis(100));
              retry_count += 1;
            }

            assert_eq!(CALL_COUNT, 1);
            assert!(IS_RIGHT_MESSAGE);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_not_trigger_listeners_for_comments() {
        static mut CALL_COUNT: i32 = 0;

        let tx = fake_server(String::from("127.0.0.1:6964"));
        let event_source = EventSource::new("http://127.0.0.1:6964/sub").unwrap();


        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        tx.send("\n").unwrap();
        tx.send("data: message\n\n").unwrap();
        tx.send(":this is a comment\n").unwrap();
        tx.send(":this is another comment\n").unwrap();
        tx.send("data: this is a message\n\n").unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert_eq!(CALL_COUNT, 2);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn ensure_stream_is_parsed_after_headers() {
        static mut CALL_COUNT: i32 = 0;

        let tx = fake_server(String::from("127.0.0.1:6965"));
        let event_source = EventSource::new("http://127.0.0.1:6965/sub").unwrap();

        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        tx.send("HTTP/1.1 200 OK\n").unwrap();
        tx.send("Server: nginx/1.10.3\n").unwrap();
        tx.send("Date: Thu, 24 May 2018 12:26:38 GMT\n").unwrap();
        tx.send("Content-Type: text/event-stream; charset=utf-8\n").unwrap();
        tx.send("Connection: keep-alive\n").unwrap();
        tx.send("\n").unwrap();
        tx.send("data: this is a message\n\n").unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(300));
            assert_eq!(CALL_COUNT, 1);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn ignore_empty_messages() {
        static mut CALL_COUNT: i32 = 0;

        let tx = fake_server(String::from("127.0.0.1:6966"));
        let event_source = EventSource::new("http://127.0.0.1:6966/sub").unwrap();

        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        tx.send("\n").unwrap();
        tx.send("data: message\n").unwrap();
        tx.send("\n").unwrap();
        tx.send("\n").unwrap();
        tx.send("data: this is a message\n\n").unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert_eq!(CALL_COUNT, 2);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn event_trigger_its_defined_listener() {
        static mut IS_RIGHT_EVENT: bool = false;

        let tx = fake_server(String::from("127.0.0.1:6967"));
        let event_source = EventSource::new("http://127.0.0.1:6967/sub").unwrap();

        event_source.add_event_listener("myEvent", |event| {
            unsafe {
                IS_RIGHT_EVENT = event.type_ == String::from("myEvent");
                IS_RIGHT_EVENT = IS_RIGHT_EVENT && event.data == String::from("my message");
            }
        });

        tx.send("\n").unwrap();
        tx.send("event: myEvent\n").unwrap();
        tx.send("data: my message\n\n").unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert!(IS_RIGHT_EVENT);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn dont_trigger_on_message_for_event() {
        static mut ON_MESSAGE_WAS_CALLED: bool = false;

        let tx = fake_server(String::from("127.0.0.1:6968"));
        let event_source = EventSource::new("http://127.0.0.1:6968/sub").unwrap();

        event_source.on_message(|_| {
            unsafe {
                ON_MESSAGE_WAS_CALLED = true;
            }
        });

        tx.send("\n").unwrap();
        tx.send("event: myEvent\n").unwrap();
        tx.send("data: my message\n\n").unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert!(!ON_MESSAGE_WAS_CALLED);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_close_connection() {
        static mut CALL_COUNT: i32 = 0;

        let tx = fake_server(String::from("127.0.0.1:6970"));
        let event_source = EventSource::new("http://127.0.0.1:6970/sub").unwrap();

        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        tx.send("\ndata: some message\n\n").unwrap();
        thread::sleep(Duration::from_millis(200));
        event_source.close();
        tx.send("\ndata: some message\n\n").unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(400));

            assert_eq!(CALL_COUNT, 1);
        }

        tx.send("close").unwrap();
    }

    #[test]
    fn should_trigger_on_open_callback_when_connected() {
        static mut CONNECTION_OPEN: bool = false;
        static mut OPEN_CALLBACK_CALLS: i32 = 0;

        let tx = fake_server(String::from("127.0.0.1:6971"));
        let event_source = EventSource::new("http://127.0.0.1:6971/sub").unwrap();
        event_source.on_open(|| {
            unsafe {
                CONNECTION_OPEN = true;
                OPEN_CALLBACK_CALLS += 1;
            }
        });

        tx.send("HTTP/1.1 200 OK\n").unwrap();
        tx.send("Date: Thu, 24 May 2018 12:26:38 GMT\n").unwrap();
        tx.send("\n").unwrap();
        unsafe {
            thread::sleep(Duration::from_millis(200));
            assert!(CONNECTION_OPEN);
            assert_eq!(OPEN_CALLBACK_CALLS, 1);
        }
        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_have_status_connecting_while_opening_connection() {
        let tx = fake_server(String::from("127.0.0.1:6972"));
        let event_source = EventSource::new("http://127.0.0.1:6972/sub").unwrap();

        {
            let state = event_source.ready_state.lock().unwrap();
            assert_eq!(*state, State::CONNECTING);
        }

        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_have_status_open_after_connection_stabilished() {
        let tx = fake_server(String::from("127.0.0.1:6973"));
        let event_source = EventSource::new("http://127.0.0.1:6973/sub").unwrap();

        tx.send("\n").unwrap();
        thread::sleep(Duration::from_millis(200));

        {
            let state = event_source.ready_state.lock().unwrap();
            assert_eq!(*state, State::OPEN);
        }



        event_source.close();
        tx.send("close").unwrap();
    }

    #[test]
    fn should_have_status_closed_after_closing_connection() {
        let tx = fake_server(String::from("127.0.0.1:6974"));
        let event_source = EventSource::new("http://127.0.0.1:6974/sub").unwrap();

        event_source.close();
        thread::sleep(Duration::from_millis(200));
        {
            let state = event_source.ready_state.lock().unwrap();
            assert_eq!(*state, State::CLOSED);
        }
        tx.send("close").unwrap();
    }
}
