use std::io::prelude::*;
use std::net::TcpStream;
use std::io::BufReader;

pub fn load() {
    let mut stream = TcpStream::connect("127.0.0.1:8080").unwrap();

    println!("listening");

    let response = "GET /sub HTTP/1.1\r\nAccept: text/event-stream\r\nHost: localhost:8080\r\n\r\n\r\n";
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        println!("{}", line.unwrap());
    }
}
