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
    listeners: Arc<Mutex<HashMap<String, Vec<fn(&str)>>>>
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
            for line in reader.lines() {
                tx.send(line.unwrap()).unwrap();
            }
        });

        let listeners = Arc::clone(&self.listeners);

        thread::spawn(move || {
            for received in rx {
                let listeners = listeners.lock().unwrap();
                listeners.get("message").unwrap().iter().for_each(|listener| {
                    listener(received.as_str());
                });
            }
        });

        Ok(())
    }

    pub fn on_message(&self, listener: fn(&str)) {
        let mut listeners = self.listeners.lock().unwrap();
         if listeners.contains_key("message") {
             listeners.get_mut("message").unwrap().push(listener);
         } else {
             listeners.insert(String::from("message"), vec!(listener));
         }
    }
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
                IS_RIGHT_MESSAGE = message == "some message";
            }
        });

        let test_stream = ("some message\n").as_bytes();
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
}
