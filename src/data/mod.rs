pub struct EventBuilder {
    pending_event: EventBuilderState
}

#[derive(Debug, PartialEq, Clone)]
pub struct Event {
    pub type_: String,
    pub data: String
}

#[derive(Debug, PartialEq, Clone)]
pub enum EventBuilderState {
    Empty,
    Pending(Event),
    Complete(Event)
}

impl Event {
    pub fn new(type_: &str, data: &str) -> Event {
        Event { type_: String::from(type_), data: String::from(data) }
    }
}

impl EventBuilder {
    pub fn new() -> EventBuilder {
        EventBuilder { pending_event: EventBuilderState::Empty }
    }

    pub fn update(&mut self, message: &str) {
        if message == "" {
            self.finalize_event();
        } else {
            self.pending_event = self.update_event(message);
        }
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
            _ => Event::new("message", "")
        };

        match parse_field(message) {
            ("data", value) => pending_event.data = String::from(value),
            ("event", value) => pending_event.type_ = String::from(value),
            _ => {}
        }

        EventBuilderState::Pending(pending_event)
    }

    pub fn get_event(&self) -> EventBuilderState {
       self.pending_event.clone()
    }
}

fn parse_field<'a>(message: &'a str) -> (&'a str, &'a str) {
    let parts: Vec<&str> = message.split(":").collect();
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
            assert_eq!(event.type_, String::from("message"));
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
            assert_eq!(event.data, String::from("test"));
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
            assert_eq!(event.data, String::from("test"));
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_fill_event_type_field() {
        let mut e = EventBuilder::new();
        e.update("event: some_event");

        if let EventBuilderState::Pending(event) = e.get_event() {
            assert_eq!(event.type_, String::from("some_event"));
        } else {
            panic!("event should be pending");
        }
    }

    #[test]
    fn should_incrementally_fill_event_fields() {
        let expected_event = Event::new("some_event", "test");
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
        let expected_event = Event::new("message", "test2");
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
        let expected_event = Event::new("some_event", "test");
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
}
