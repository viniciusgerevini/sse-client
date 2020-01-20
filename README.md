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
## Cargo features

* `native-tls` enables support for HTTPS URLs using [native-tls](https://crates.io/crates/native-tls)
* `native-tls-vendored` will additionally link OpenSSL statically

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE))
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
