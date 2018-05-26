extern crate url;

use std::io::prelude::*;
use std::io::Error;
use std::io::BufReader;
use std::net::TcpStream;
use std::thread;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use url::{Url, ParseError};

pub struct EventSource {
    listeners: Arc<Mutex<HashMap<String, Vec<fn(Event)>>>>
}

#[derive(Debug, Clone)]
pub struct Event {
    type_: String,
    data: String
}

impl EventSource {
    pub fn new(url: &str) -> Result<EventSource, ParseError> {
        let stream = open_connection(Url::parse(url)?).unwrap();
        let reader = BufReader::new(stream);

        let listeners = Arc::new(Mutex::new(HashMap::new()));
        let event_source = EventSource{ listeners };

        event_source.start(reader)?;

        Ok(event_source)
    }

    fn start<R: BufRead + Send + 'static>(&self, reader: R) -> Result<(), ParseError> {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let mut body_started = false;

            for line in reader.lines() {
                let line = line.unwrap();

                if !body_started {
                    body_started = line == "";
                    continue;
                }

                tx.send(line).unwrap();
            }
        });

        let listeners = Arc::clone(&self.listeners);

        thread::spawn(move || {
            let mut pending_event: Option<Event> = None;

            for received in rx {
                if received.starts_with(":") {
                    continue;
                }

                if received == "" {
                    if let Some(e) = pending_event {
                        dispatch_event(&listeners, &e);
                        pending_event = None;
                    }
                    continue;
                }

                pending_event = update_event(pending_event, received);
            }
        });

        Ok(())
    }

    pub fn on_message(&self, listener: fn(Event)) {
        self.add_event_listener("message", listener);
    }

    pub fn add_event_listener(&self, event_type: &str, listener: fn(Event)) {
        let mut listeners = self.listeners.lock().unwrap();
         if listeners.contains_key(event_type) {
             listeners.get_mut(event_type).unwrap().push(listener);
         } else {
             listeners.insert(String::from(event_type), vec!(listener));
         }
    }
}

fn dispatch_event(listeners: &Arc<Mutex<HashMap<String, Vec<fn(Event)>>>>, event: &Event) {
    let listeners = listeners.lock().unwrap();
    if listeners.contains_key(&event.type_) {
        for listener in listeners.get(&event.type_).unwrap().iter() {
            listener(event.clone())
        }
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

fn open_connection(url: Url) -> Result<TcpStream, Error> {
    let path = url.path();
    let host = get_host(&url);
    let host = host.as_str();

    let mut stream = TcpStream::connect(host)?;

    let response = format!(
        "GET {} HTTP/1.1\r\nAccept: text/event-stream\r\nHost: {}\r\n\r\n",
        path,
        host
    );

    stream.write(response.as_bytes())?;
    stream.flush().unwrap();

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn should_create_client() {
        let _ = EventSource::new("127.0.0.1:1236/sub");
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
        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.on_message(|_| {});
        event_source.on_message(|_| {});

        let listeners = event_source.listeners.lock().unwrap();

        if let Some(l) = listeners.get("message") {
            assert_eq!(l.len(), 2)
        } else {
            panic!("should contain listeners")
        }
    }

    #[test]
    fn should_trigger_listeners_when_message_received() {
        static mut CALL_COUNT: i32 = 0;
        static mut IS_RIGHT_MESSAGE: bool = false;

        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.on_message(|message| {
            unsafe {
                CALL_COUNT += 1;
                IS_RIGHT_MESSAGE = message.data == "some message";
            }
        });

        let test_stream = ("\ndata: some message\n\n").as_bytes();
        event_source.start(test_stream).unwrap();

        unsafe {
            let mut retry_count = 0;
            while CALL_COUNT == 0 && retry_count < 5 {
              thread::sleep(Duration::from_millis(100));
              retry_count += 1;
            }

            assert_eq!(CALL_COUNT, 1);
            assert!(IS_RIGHT_MESSAGE);
        }
    }

    #[test]
    fn should_not_trigger_listeners_for_comments() {
        static mut CALL_COUNT: i32 = 0;

        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        let test_stream = "
data: message\n
:this is a comment
:this is another comment
data: this is a message\n\n"
            .as_bytes();

        event_source.start(test_stream).unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert_eq!(CALL_COUNT, 2);
        }
    }

    #[test]
    fn ensure_stream_is_parsed_after_headers() {
        static mut CALL_COUNT: i32 = 0;

        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        let test_stream = "HTTP/1.1 200 OK
Server: nginx/1.10.3
Date: Thu, 24 May 2018 12:26:38 GMT
Content-Type: text/event-stream; charset=utf-8
Connection: keep-alive

data: this is a message\n\n"
            .as_bytes();

        event_source.start(test_stream).unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(300));
            assert_eq!(CALL_COUNT, 1);
        }
    }

    #[test]
    fn ignore_empty_messages() {
        static mut CALL_COUNT: i32 = 0;

        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.on_message(|_| {
            unsafe {
                CALL_COUNT += 1;
            }
        });

        let test_stream = "
data: message


data: this is a message\n\n"
            .as_bytes();

        event_source.start(test_stream).unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert_eq!(CALL_COUNT, 2);
        }
    }

    #[test]
    fn event_trigger_its_defined_listener() {
        static mut IS_RIGHT_EVENT: bool = false;

        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.add_event_listener("myEvent", |event| {
            unsafe {
                IS_RIGHT_EVENT = event.type_ == String::from("myEvent");
                IS_RIGHT_EVENT = IS_RIGHT_EVENT && event.data == String::from("my message");
            }
        });

        let test_stream = "
event: myEvent
data: my message\n\n"
            .as_bytes();

        event_source.start(test_stream).unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert!(IS_RIGHT_EVENT);
        }
    }

    #[test]
    fn dont_trigger_on_message_for_event() {
        static mut ON_MESSAGE_WAS_CALLED: bool = false;

        let event_source = EventSource {
            listeners: Arc::new(Mutex::new(HashMap::new()))
        };

        event_source.on_message(|_| {
            unsafe {
                ON_MESSAGE_WAS_CALLED = true;
            }
        });

        let test_stream = "
event: myEvent
data: my message\n\n"
            .as_bytes();

        event_source.start(test_stream).unwrap();

        unsafe {
            thread::sleep(Duration::from_millis(500));
            assert!(!ON_MESSAGE_WAS_CALLED);
        }
    }
}
