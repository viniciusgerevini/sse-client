extern crate sse_client;

use std::thread;
use std::time::Duration;
use sse_client::EventSource;

fn main() {
    println!("Running sample");
    let event_source = EventSource::new("http://127.0.0.1:8080/sub").unwrap();

    event_source.on_message(|message| {
        println!("New message {:?}", message);
    });

    thread::sleep(Duration::from_millis(4000));
}
