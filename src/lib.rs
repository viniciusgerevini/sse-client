extern crate url;

mod network;
mod pub_sub;
mod data;
#[cfg(test)]
mod test_helper;

use std::sync::{Arc, Mutex};
use url::{Url, ParseError};
use network::{EventStream, State};
use pub_sub::Bus;
use data::{EventBuilder, EventBuilderState, Event};

pub struct EventSource {
    bus: Arc<Mutex<Bus<Event>>>,
    stream: EventStream
}

impl EventSource {
    pub fn new(url: &str) -> Result<EventSource, ParseError> {
        let mut stream = EventStream::new(Url::parse(url)?).unwrap();
        let bus = Arc::new(Mutex::new(Bus::new()));

        let event_bus = Arc::clone(&bus);
        stream.on_open(move || {
            publish_initial_stream_event(&event_bus);
        });

        let event_bus = Arc::clone(&bus);
        stream.on_error(move |message| {
            let event_bus = event_bus.lock().unwrap();
            let event = Event::new("error", &message);
            event_bus.publish(event.type_.clone(), event);
        });

        let event_builder = Arc::new(Mutex::new(EventBuilder::new()));
        let event_bus = Arc::clone(&bus);
        stream.on_message(move |message| {
            handle_message(&message, &event_builder, &event_bus);
        });

        Ok(EventSource{ stream, bus })
    }

    pub fn close(&self) {
        self.stream.close();
    }

    pub fn on_open<F>(&self, listener: F) where F: Fn() + Send + 'static {
        self.add_event_listener("stream_opened", move |_| { listener(); });
    }

    pub fn on_message<F>(&self, listener: F) where F: Fn(Event) + Send + 'static {
        self.add_event_listener("message", listener);
    }

    pub fn add_event_listener<F>(&self, event_type: &str, listener: F) where F: Fn(Event) + Send + 'static {
        let mut bus = self.bus.lock().unwrap();
        bus.subscribe(event_type.to_string(), listener);
    }

    pub fn state(&self) -> State {
        self.stream.state()
    }
}

fn publish_initial_stream_event(event_bus: &Arc<Mutex<Bus<Event>>>) {
    let event_bus = event_bus.lock().unwrap();
    let event = Event::new("stream_opened", "");
    event_bus.publish(event.type_.clone(), event);
}

fn handle_message(message: &str, event_builder: &Arc<Mutex<EventBuilder>>, event_bus: &Arc<Mutex<Bus<Event>>>) {
    let mut event_builder = event_builder.lock().unwrap();

    if let EventBuilderState::Complete(event) = event_builder.update(&message) {
        let event_bus = event_bus.lock().unwrap();
        event_bus.publish(event.type_.clone(), event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
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
    fn should_return_stream_connection_status() {
        let (event_source, fake_server) = setup();

        assert_eq!(event_source.state(), State::CONNECTING);

        fake_server.send("\n");
        thread::sleep(Duration::from_millis(200));

        assert_eq!(event_source.state(), State::OPEN);

        event_source.close();
        thread::sleep(Duration::from_millis(200));

        assert_eq!(event_source.state(), State::CLOSED);

        fake_server.close();
    }
}
