# SSE Client

[![Documentation](https://docs.rs/sse-client/badge.svg)](https://docs.rs/sse-client/)

EventSource implementation in Rust to handle streams of Server-Sent Events.
It handles connections, redirections, retries and message parsing.

To know more about SSE: [Standard](https://html.spec.whatwg.org/multipage/server-sent-events.html) | [EventSource interface](https://developer.mozilla.org/en-US/docs/Web/API/EventSource)

# Example:

Usage:

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

Or:

```rust
extern crate sse_client;
use sse_client::EventSource;

let event_source = EventSource::new("http://event-stream-address/sub").unwrap();

for event in event_source.receiver().iter() {
    println!("New Message: {}", event.data);
}
```

