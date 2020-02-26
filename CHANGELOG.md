# Changelog

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)

## 1.1.0 (2020-01-20)

### Added

- Added optional HTTPS support. Thanks to @Atte for this contribution.

## 1.0.2 (2020-01-15)

### Fixed

- Fixed deprecation warnings

## 1.0.1 (2019-12-13)

### Added

- Dual license as MIT and Apache 2.0 for extended compatibility.

https://rust-lang.github.io/api-guidelines/necessities.html#crate-and-its-dependencies-have-a-permissive-license-c-permissive

## 1.0.0 (2018-10-04)

Although this version is backwards compatible, I decided to bump from 0.1.1 to 1.0.0 to make clear the implementation is complete.

### Added

- Blocking API for reading events:

```rust
let event_source = EventSource::new("http://event-stream-address/sub").unwrap();

for event in event_source.receiver().iter() {
    println!("New Message: {}", event.data);
}
```

## 0.1.1 (2018-10-04)

### Fixed:

- hosts didn't work when port not provided
- fields without `:` or any random message could break the stream

## 0.1.0 (2018-08-26)

Initial release

