# SSE Client

EventSource implementation in Rust to handle streams of Server-Sent Events.
It handles connections, redirections, retries and message parsing.

To know more about SSE: <a href="https://html.spec.whatwg.org/multipage/server-sent-events.html" target="_new">Standard</a> | <a href="https://developer.mozilla.org/en-US/docs/Web/API/EventSource" target="_new">EventSource interface</a>

# Example:

```rust
extern crate sse_client;
use sse_client::EventSource;


let event_source = EventSource::new("http://event-stream-address/sub").unwrap();

event_source.on_message(|message| {
    println!("New message event {:?}", message);
});

event_source.add_event_listener("error", |error| {
    println!("Error {:?}", error);
});

```

