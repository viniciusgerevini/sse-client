pub struct EventBuilder {
    pending_event: EventBuilderState
}

/// Event data sent by server
#[derive(Debug, PartialEq, Clone)]
pub struct Event {
    /// Represents message `event` field.
    pub event: Option<String>,
    /// Represents message `data` field.
    pub data: Option<String>,
    /// Represents message `id` field.
    pub id: Option<String>,
    /// Represents message `retry` field.
    pub retry: Option<String>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum EventBuilderState {
    Empty,
    Pending(Event),
    Complete(Event)
}

impl Event {
    pub fn new() -> Event {
        Event {
            event: None,
            data: None,
            id: None,
            retry: None,
        }
    }
}

#[derive(Default)]
pub struct EventData {
    event: Option<String>,
    data: Option<String>,
    id: Option<String>,
    retry: Option<String>,
}

impl EventData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    pub fn data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn retry(mut self, retry: impl Into<String>) -> Self {
        self.retry = Some(retry.into());
        self
    }

    pub fn build(self) -> Event {
        Event {
            event: self.event,
            data: self.data,
            id: self.id,
            retry: self.retry,
        }
    }
}

impl EventBuilder {
    pub fn new() -> EventBuilder {
        EventBuilder { pending_event: EventBuilderState::Empty }
    }

    pub fn update(&mut self, message: &str) -> EventBuilderState {
        if message == "" {
            self.finalize_event();
        } else if !message.starts_with(":") && message.contains(":") {
            self.pending_event = self.update_event(message);
        }

        self.get_event()
    }

    fn finalize_event(&mut self) {
        self.pending_event = match self.pending_event {
            EventBuilderState::Complete(_) => EventBuilderState::Empty,
            EventBuilderState::Pending(ref event) => EventBuilderState::Complete(event.clone()),
            ref e => e.clone()
        };
    }

    fn update_event(&mut self, message: &str) -> EventBuilderState {
        let mut pending_event = match &self.pending_event {
            EventBuilderState::Pending(ref e) => e.clone(),
            _ => EventData::new()
                .event("message")
                .build(),
        };

        match parse_field(message) {
            ("event", value) => pending_event.event = Some(value.to_string()),
            ("data", value) => pending_event.data = Some(value.to_string()),
            ("id", value) => pending_event.id = Some(value.to_string()),
            ("retry", value) => pending_event.retry = Some(value.to_string()),
            _ => {}
        }

        EventBuilderState::Pending(pending_event)
    }

    pub fn clear(&mut self) {
        self.pending_event = EventBuilderState::Empty;
    }

    pub fn get_event(&self) -> EventBuilderState {
       self.pending_event.clone()
    }
}

fn parse_field<'a>(message: &'a str) -> (&'a str, &'a str) {
    let parts: Vec<&str> = message.splitn(2, ":").collect();
    (parts[0], parts[1].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_start_with_empty_state() {
        let e = EventBuilder::new();

        assert_eq!(e.get_event(), EventBuilderState::Empty);
    }

    #[test]
    fn should_set_message_as_default_event_type() {
        let mut e = EventBuilder::new();
        e.update("data: test");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event.event, Some("message".to_string()));
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_change_status_to_complete_when_empty_message_received() {
        let mut e = EventBuilder::new();
        e.update("data: test");
        e.update("");

        if let EventBuilderState::Complete(event) = e.get_event() {
            assert_eq!(event.data, Some("test".to_string()));
        } else {
            panic!("event should be complete");
        }
    }

    #[test]
    fn should_remain_as_empty_if_empty_message_is_received_while_buffer_is_empty() {
        let mut e = EventBuilder::new();
        e.update("");

        assert_eq!(e.get_event(), EventBuilderState::Empty);
    }

    #[test]
    fn should_fill_data_field() {
        let mut e = EventBuilder::new();
        e.update("data: test");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event.data, Some("test".to_string()));
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_fill_event_type_field() {
        let mut e = EventBuilder::new();
        e.update("event: some_event");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event.event, Some("some_event".to_string()));
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_fill_event_id_field() {
        let mut e = EventBuilder::new();
        e.update("id: 123abc");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event.id, Some("123abc".to_string()));
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_incrementally_fill_event_fields() {
        let expected_event = EventData::new()
            .event("some_event")
            .data("test")
            .build();
        let mut e = EventBuilder::new();
        e.update("event: some_event");
        e.update("data: test");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event, expected_event);
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_change_status_to_empty_when_event_is_complete_and_empty_message_is_received() {
        let mut e = EventBuilder::new();

        e.update("data: test");
        e.update("");
        e.update("");

        assert_eq!(e.get_event(), EventBuilderState::Empty);
    }

    #[test]
    fn should_start_clean_event_after_previous_is_completed() {
        let expected_event = EventData::new()
            .event("message")
            .data("test2")
            .build();
        let mut e = EventBuilder::new();

        e.update("event: some_event");
        e.update("data: test");
        e.update("");
        e.update("data: test2");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event, expected_event);
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_ignore_updates_started_with_colon() {
        let expected_event = EventData::new()
            .event("some_event")
            .data("test")
            .build();
        let mut e = EventBuilder::new();

        e.update("event: some_event");
        e.update(":some commentary");
        e.update("data: test");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event, expected_event);
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_return_event_on_update() {
        let mut e = EventBuilder::new();
        assert_eq!(e.update("event: some_event"), e.get_event());
        assert_eq!(e.update("data: test"), e.get_event());
    }

    #[test]
    fn should_parse_messages_even_with_colons() {
        let expected_event = EventData::new()
            .event("some:id:with:colons")
            .data("some:data:with:colons")
            .build();
        let mut e = EventBuilder::new();

        e.update("event: some:id:with:colons");
        e.update("data: some:data:with:colons");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event, expected_event);
        } else {
            panic!("event should be pending");
        }

    }

    #[test]
    fn should_be_empty_when_only_comments_received() {
        let mut e = EventBuilder::new();

        e.update(":hi");
        e.update("");

        assert_eq!(e.get_event(), EventBuilderState::Empty);
    }

    #[test]
    fn should_ignore_messages_without_colon() {
        let expected_event = EventData::new()
            .event("some_event")
            .data("test")
            .build();
        let mut e = EventBuilder::new();

        e.update("event: some_event");
        e.update("this is a random message that should be ignored");
        e.update("data: test");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event, expected_event);
        } else {
            panic!("event should be pending");
        }
    }
}
