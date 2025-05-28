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
//!
//! Or:
//! ```no_run
//! extern crate sse_client;
//! use sse_client::EventSource;
//!
//!
//! let event_source = EventSource::new("http://event-stream-address/sub").unwrap();
//!
//! for event in event_source.receiver().iter() {
//!     println!("New Message: {:?}", event.data);
//! }
//!
//! ```
extern crate url;

#[cfg(feature = "native-tls")]
extern crate native_tls_crate as native_tls;

#[cfg(test)]
extern crate http_test_server;

#[cfg(feature = "native-tls")]
mod tls;
mod network;
mod pub_sub;
mod data;

use std::sync::{Arc, Mutex, mpsc };
use url::{Url, ParseError};
use network::EventStream;
use pub_sub::Bus;
use data::{EventBuilder, EventBuilderState};

pub use data::{Event, EventData};
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
            let event = EventData::new()
                .event("error")
                .data(message)
                .build();
            event_bus.publish("error".to_string(), event);
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
    ///     println!("Message received: {:?}", message.data);
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
    ///     println!("Event {:?} received: {:?}", event.event, event.data);
    /// });
    ///
    /// event_source.add_event_listener("error", |error| {
    ///     println!("Error: {:?}", error.data);
    /// });
    ///
    /// // equivalent to `on_message`
    /// event_source.add_event_listener("message", |message| {
    ///     println!("Message received: {:?}", message.data);
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

    /// Returns a receiver that is triggered on any new message or error.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    ///
    /// for event in event_source.receiver().iter() {
    ///     println!("New Message: {:?}", event.data);
    /// }
    ///
    /// ```
    /// ```no_run
    /// # extern crate sse_client;
    /// # use sse_client::EventSource;
    /// let event_source = EventSource::new("http://example.com/sub").unwrap();
    /// let rx = event_source.receiver();
    ///
    /// let event = rx.recv().unwrap();
    /// //...
    ///
    /// ```
    pub fn receiver(&self) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        let error_tx = tx.clone();

        self.on_message(move |event| {
            tx.send(event).unwrap();
        });

        self.add_event_listener("error", move |error| {
            error_tx.send(error).unwrap();
        });

        rx
    }
}

fn publish_initial_stream_event(event_bus: &Arc<Mutex<Bus<Event>>>) {
    let event_bus = event_bus.lock().unwrap();
    let event = EventData::new()
        .event("stream_opened")
        .build();
    event_bus.publish("stream_opened".to_string(), event);
}

fn handle_message(
    message: &str,
    event_builder: &Arc<Mutex<EventBuilder>>,
    event_bus: &Arc<Mutex<Bus<Event>>>,
    event_stream: &Arc<Mutex<EventStream>>) {

    let mut event_builder = event_builder.lock().unwrap();

    if let EventBuilderState::Complete(event) = event_builder.update(&message) {
        let event_bus = event_bus.lock().unwrap();
        if let Some(id) = event.id.as_ref() {
            event_stream.lock().unwrap().set_last_id(id.clone());
        }
        if let Some(e) = event.event.as_ref() {
            event_bus.publish(e.clone(), event);
        }
        event_builder.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use std::sync::mpsc;
    use http_test_server::{ TestServer, Resource };
    use http_test_server::http::Status;

    fn setup() -> (TestServer, Resource, String) {
        let server = TestServer::new().unwrap();
        let resource = server.create_resource("/sub");
        resource.header("Content-Type", "text/event-stream").stream();
        let address = format!("http://localhost:{}/sub", server.port());
        thread::sleep(Duration::from_millis(100));
        (server, resource, address)
    }


    #[test]
    fn should_create_client() {
        let (_server, _stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.close();
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
        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        while event_source.state() == State::Connecting {
            thread::sleep(Duration::from_millis(100));
        }

        stream_endpoint
            .send_line("data: some message").send_line("");

        let message = rx.recv().unwrap();
        assert_eq!(message, Some("some message".to_string()));

        event_source.close();
    }

    #[test]
    fn should_trigger_listeners_when_message_received() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        event_source.on_message(move |message| {
            tx2.send(message.data).unwrap();
        });

        while event_source.state() == State::Connecting {
            thread::sleep(Duration::from_millis(100));
        }

        stream_endpoint.send_line("data: some message").send_line("");

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();

        assert_eq!(message, Some("some message".to_string()));
        assert_eq!(message2, Some("some message".to_string()));

        event_source.close();
    }

    #[test]
    fn should_not_trigger_listeners_for_comments() {
        let (tx, rx) = mpsc::channel();

        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        while event_source.state() != State::Open {
            thread::sleep(Duration::from_millis(100));
        };

        stream_endpoint
            .send_line("data: message")
            .send_line("")
            .send_line(":this is a comment")
            .send_line(":this is another comment")
            .send_line("data: this is a message")
            .send_line("");

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();

        assert_eq!(message, Some("message".to_string()));
        assert_eq!(message2, Some("this is a message".to_string()));

        event_source.close();
    }

    #[test]
    fn ignore_empty_messages() {
        let (tx, rx) = mpsc::channel();

        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        while event_source.state() != State::Open {
            thread::sleep(Duration::from_millis(100));
        };

        stream_endpoint
            .send_line("data: message")
            .send_line("")
            .send_line("")
            .send_line("data: this is a message")
            .send_line("");

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();

        assert_eq!(message, Some("message".to_string()));
        assert_eq!(message2, Some("this is a message".to_string()));

        event_source.close();
    }

    #[test]
    fn event_trigger_its_defined_listener() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.add_event_listener("myEvent", move |event| {
            tx.send(event).unwrap();
        });

        while event_source.state() == State::Connecting {
            thread::sleep(Duration::from_millis(100));
        }

        stream_endpoint
            .send_line("event: myEvent")
            .send_line("data: my message\n");

        let message = rx.recv().unwrap();

        assert_eq!(message.event, Some("myEvent".to_string()));
        assert_eq!(message.data, Some("my message".to_string()));

        event_source.close();
    }

    #[test]
    fn dont_trigger_on_message_for_event() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

        event_source.on_message(move |_| {
            tx.send("NOOOOOOOOOOOOOOOOOOO!").unwrap();
        });

        stream_endpoint
            .send("event: myEvent\n")
            .send("data: my message\n\n");

        thread::sleep(Duration::from_millis(500));
        assert!(rx.try_recv().is_err());

        event_source.close();
    }

    #[test]
    fn should_close_connection() {
        let (tx, rx) = mpsc::channel();

        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();

         event_source.on_message(move |message| {
            tx.send(message.data).unwrap();
        });

        while event_source.state() != State::Open {
            thread::sleep(Duration::from_millis(100));
        };

        stream_endpoint.send("\ndata: some message\n\n");
        rx.recv().unwrap();
        event_source.close();
        stream_endpoint.send("\ndata: some message\n\n");

        thread::sleep(Duration::from_millis(400));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn should_trigger_on_open_callback_when_connected() {
        let (tx, rx) = mpsc::channel();
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint.delay(Duration::from_millis(200));

        let event_source = EventSource::new(&address).unwrap();

        event_source.on_open(move || {
            tx.send("open").unwrap();
        });

        rx.recv().unwrap();

        event_source.close();
    }

    #[test]
    fn should_return_stream_connection_status() {
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint
            .delay(Duration::from_millis(200))
            .stream();

        let event_source = EventSource::new(&address).unwrap();
        thread::sleep(Duration::from_millis(100));

        assert_eq!(event_source.state(), State::Connecting);

        thread::sleep(Duration::from_millis(200));

        assert_eq!(event_source.state(), State::Open);

        event_source.close();
        thread::sleep(Duration::from_millis(200));

        assert_eq!(event_source.state(), State::Closed);
    }


    #[test]
    fn should_send_last_event_id_on_reconnection() {
        let (server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();
        thread::sleep(Duration::from_millis(100));

        stream_endpoint.send("id: helpMe\n");
        stream_endpoint.send("data: my message\n\n");

        thread::sleep(Duration::from_millis(500));

        stream_endpoint.close_open_connections();

        let request = server.requests().recv().unwrap();

        assert_eq!(request.headers.get("Last-Event-ID").unwrap(), "helpMe");

        event_source.close();
    }

    #[test]
    fn should_expose_blocking_api() {
        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();
        thread::sleep(Duration::from_millis(100));
        let rx = event_source.receiver();

        stream_endpoint.send("data: some message\n\n");
        stream_endpoint.send("data: some message 2\n\n");

        assert_eq!(rx.recv().unwrap().data, Some("some message".to_string()));
        assert_eq!(rx.recv().unwrap().data, Some("some message 2".to_string()));

        event_source.close();
    }

    #[test]
    fn receiver_should_get_error_events() {
        let (_server, stream_endpoint, address) = setup();
        stream_endpoint
            .delay(Duration::from_millis(100))
            .status(Status::InternalServerError);
        let event_source = EventSource::new(&address).unwrap();
        let rx = event_source.receiver();

        assert_eq!(rx.recv().unwrap().event, Some("error".to_string()));

        event_source.close();
    }

    #[test]
    fn should_receive_multiple_data() {
        let (_server, stream_endpoint, address) = setup();
        let event_source = EventSource::new(&address).unwrap();
        thread::sleep(Duration::from_millis(100));
        let rx = event_source.receiver();

        stream_endpoint.send("data: YHOO\n");
        stream_endpoint.send("data: +2\n");
        stream_endpoint.send("data: 10\n\n");

        assert_eq!(rx.recv().unwrap().data, Some("YHOO\n+2\n10".to_string()));

        event_source.close();
    }
}
