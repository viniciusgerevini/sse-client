//! # SSE Client
//! EventSource implementation to handle streams of Server-Sent Events.
//! It handles connections, redirections, retries and message parsing.
//!
//! To know more about SSE: <a href="https://html.spec.whatwg.org/multipage/server-sent-events.html" target="_new">Standard</a> | <a href="https://developer.mozilla.org/en-US/docs/Web/API/EventSource" target="_new">EventSource interface</a>
//!
//! # Example:
//!
//! ```no_run
//! extern crate sse_client;
//! use sse_client::EventSource;
//!
//!
//! let event_source = EventSource::new("http://event-stream-address/sub").unwrap();
//!
//! event_source.on_message(|message| {
//!     println!("New message event {:?}", message);
//! });
//!
//! event_source.add_event_listener("error", |error| {
//!     println!("Error {:?}", error);
//! });
//!
//! ```
extern crate url;

mod network;
mod pub_sub;
mod data;
#[cfg(test)]
mod test_helper;

use std::sync::{Arc, Mutex};
use url::{Url, ParseError};
use network::EventStream;
use pub_sub::Bus;
use data::{EventBuilder, EventBuilderState};

pub use data::Event;
pub use network::State;

/// Interface to interact with `event-streams`
pub struct EventSource {
    bus: Arc<Mutex<Bus<Event>>>,
    stream: Arc<Mutex<EventStream>>
}

impl EventSource {
    /// Create object and starts connection with event-stream
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    /// ```
    pub fn new(url: &str) -> Result<EventSource, ParseError> {
        let event_stream = Arc::new(Mutex::new(EventStream::new(Url::parse(url)?).unwrap()));
        let stream_for_update = Arc::clone(&event_stream);
        let stream = Arc::clone(&event_stream);
        let mut event_stream = event_stream.lock().unwrap();

        let bus = Arc::new(Mutex::new(Bus::new()));

        let event_bus = Arc::clone(&bus);
        event_stream.on_open(move || {
            publish_initial_stream_event(&event_bus);
        });

        let event_bus = Arc::clone(&bus);
        event_stream.on_error(move |message| {
            let event_bus = event_bus.lock().unwrap();
            let event = Event::new("error", &message);
            event_bus.publish(event.type_.clone(), event);
        });

        let event_builder = Arc::new(Mutex::new(EventBuilder::new()));
        let event_bus = Arc::clone(&bus);

        event_stream.on_message(move |message| {
            handle_message(&message, &event_builder, &event_bus, &stream_for_update);
        });

        Ok(EventSource{ stream, bus })
    }

    /// Close connection.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    /// event_source.close();
    /// ```
    pub fn close(&self) {
        self.stream.lock().unwrap().close();
    }

    /// Triggered when connection with stream is stabilished.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    ///
    /// event_source.on_open(|| {
    ///     println!("Connection stabilished!");
    /// });
    /// ```
    pub fn on_open<F>(&self, listener: F) where F: Fn() + Send + 'static {
        self.add_event_listener("stream_opened", move |_| { listener(); });
    }

    /// Triggered when `message` event is received.
    /// Any event that doesn't contain an `event` field is considered a `message` event.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    ///
    /// event_source.on_message(|message| {
    ///     println!("Message received: {}", message.data);
    /// });
    /// ```
    pub fn on_message<F>(&self, listener: F) where F: Fn(Event) + Send + 'static {
        self.add_event_listener("message", listener);
    }

    /// Triggered when event with specified type is received.
    ///
    /// Any connection error is notified as event with type `error`.
    ///
    /// Events with no type defined have `message` as default type.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    ///
    ///
    /// event_source.add_event_listener("myEvent", |event| {
    ///     println!("Event {} received: {}", event.type_, event.data);
    /// });
    ///
    /// event_source.add_event_listener("error", |error| {
    ///     println!("Error: {}", error.data);
    /// });
    ///
    /// // equivalent to `on_message`
    /// event_source.add_event_listener("message", |message| {
    ///     println!("Message received: {}", message.data);
    /// });
    /// ```
    pub fn add_event_listener<F>(&self, event_type: &str, listener: F) where F: Fn(Event) + Send + 'static {
        let mut bus = self.bus.lock().unwrap();
        bus.subscribe(event_type.to_string(), listener);
    }

    /// Returns client [`State`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// use sse_client::State;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    ///
    /// assert_eq!(event_source.state(), State::Connecting);
    /// ```
    /// [`State`]: enum.State.html
    pub fn state(&self) -> State {
        self.stream.lock().unwrap().state()
    }
}

fn publish_initial_stream_event(event_bus: &Arc<Mutex<Bus<Event>>>) {
    let event_bus = event_bus.lock().unwrap();
    let event = Event::new("stream_opened", "");
    event_bus.publish(event.type_.clone(), event);
}

fn handle_message(
    message: &str,
    event_builder: &Arc<Mutex<EventBuilder>>,
    event_bus: &Arc<Mutex<Bus<Event>>>,
    event_stream: &Arc<Mutex<EventStream>>) {

    let mut event_builder = event_builder.lock().unwrap();

    if let EventBuilderState::Complete(event) = event_builder.update(&message) {
        let event_bus = event_bus.lock().unwrap();
        event_stream.lock().unwrap().set_last_id(event.id.clone());
        event_bus.publish(event.type_.clone(), event);
        event_builder.clear();
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
        thread::sleep(Duration::from_millis(100));
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

        assert_eq!(event_source.state(), State::Connecting);

        fake_server.send("\n");
        thread::sleep(Duration::from_millis(200));

        assert_eq!(event_source.state(), State::Open);

        event_source.close();
        thread::sleep(Duration::from_millis(200));

        assert_eq!(event_source.state(), State::Closed);

        fake_server.close();
    }


    #[test]
    fn should_send_last_event_id_on_reconnection_2() {
        let (event_source, mut fake_server) = setup();
        let expected_header = Arc::new(Mutex::new(None));

        let header = Arc::clone(&expected_header);
        fake_server.on_client_message(move |message| {
            if message.starts_with("Last-Event-ID") {
                let mut header = header.lock().unwrap();
                *header = Some(message);
            }
        });

        fake_server.send("HTTP/1.1 200 OK\n");
        fake_server.send("\n");
        fake_server.send("id: helpMe\n");
        fake_server.send("data: my message\n\n");

        thread::sleep(Duration::from_millis(500));
        fake_server.break_current_connection();
        thread::sleep(Duration::from_millis(800));

        let expected_header = expected_header.lock().unwrap();

        if let Some(ref header) = *expected_header {
            assert_eq!(header, "Last-Event-ID: helpMe");
        } else {
            panic!("should contain last id in header");
        }

        event_source.close();
        fake_server.close();
    }
}
