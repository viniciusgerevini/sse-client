extern crate url;

use std::io::prelude::*;
use std::io::Error;
use std::net::TcpStream;
use std::io::BufReader;
use std::thread;
use std::time::Duration;

use url::{Url, ParseError};

pub fn load() {
    println!("is it running");
    let _ = EventSource::new("http://127.0.0.1:8080/sub").unwrap();
    thread::sleep(Duration::from_millis(4000));
}

pub struct EventSource {
}

impl EventSource {
    pub fn new(url: &str) -> Result<EventSource, ParseError> {
        let mut stream = open_connection(Url::parse(url)?).unwrap();
        let reader = BufReader::new(stream);

        thread::spawn(|| {
            for line in reader.lines() {
                println!("{}", line.unwrap());
            }
        });

        Ok(EventSource{})
    }
}

fn open_connection(url: Url) -> Result<TcpStream, Error> {
    let path = url.path();
    let host = get_host(&url);
    let host = host.as_str();

    let mut stream = TcpStream::connect(host).unwrap();

    let response = format!(
        "GET {} HTTP/1.1\r\nAccept: text/event-stream\r\nHost: {}\r\n\r\n",
        path,
        host
    );

    stream.write(response.as_bytes()).unwrap();
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
}
