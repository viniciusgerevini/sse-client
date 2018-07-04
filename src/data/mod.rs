pub struct EventBuilder {
    pending_event: Option<Event>,
    event_state: EventBuilderState
}

#[derive(Debug, PartialEq, Clone)]
pub struct Event {
    type_: String,
    data: String
}

#[derive(Debug, PartialEq, Clone)]
pub enum EventBuilderState {
    EMPTY,
    PENDING,
    COMPLETE
}

impl EventBuilder {
    pub fn new() -> EventBuilder {
        EventBuilder { pending_event: None, event_state: EventBuilderState::EMPTY }
    }

    pub fn update(&mut self, message: &str) {
        if message == "" {
            self.finalize_event();
        } else {
            self.pending_event = self.update_event(message);
        }
    }

    fn finalize_event(&mut self) {
        match self.event_state {
            EventBuilderState::COMPLETE => {
                self.event_state = EventBuilderState::EMPTY;
                self.pending_event = None;
            },
            EventBuilderState::PENDING =>
                self.event_state = EventBuilderState::COMPLETE,
            _ => {}
        }
    }

    fn update_event(&mut self, message: &str) -> Option<Event> {
        let mut pending_event = match &self.event_state {
            EventBuilderState::PENDING => self.pending_event.clone().unwrap(),
            _ => {
                self.event_state = EventBuilderState::PENDING;
                Event { type_: String::from("message"), data: String::from("") }
            }
        };

        match parse_field(message) {
            ("data", value) => pending_event.data = String::from(value),
            ("event", value) => pending_event.type_ = String::from(value),
            _ => {}
        }

        Some(pending_event)
    }

    pub fn build(&self) -> Option<Event> {
       self.pending_event.clone()
    }

    pub fn state(&self) -> EventBuilderState {
       self.event_state.clone()
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
        let event = e.build();

        assert_eq!(e.state(), EventBuilderState::EMPTY);
        assert_eq!(event, None);
    }

    #[test]
    fn should_set_message_as_default_event_type() {
        let mut e = EventBuilder::new();
        e.update("data: test");

        let event = e.build().unwrap();

        assert_eq!(e.state(), EventBuilderState::PENDING);
        assert_eq!(event.type_, String::from("message"));
    }

    #[test]
    fn should_change_status_to_complete_when_empty_message_received() {
        let mut e = EventBuilder::new();
        e.update("data: test");
        e.update("");

        e.build().unwrap();

        assert_eq!(e.state(), EventBuilderState::COMPLETE);
    }

    #[test]
    fn should_remain_as_empty_if_empty_message_is_received_while_buffer_is_empty() {
        let mut e = EventBuilder::new();
        e.update("");

        let event = e.build();

        assert_eq!(e.state(), EventBuilderState::EMPTY);
        assert_eq!(event, None);
    }

    #[test]
    fn should_fill_data_field() {
        let mut e = EventBuilder::new();
        e.update("data: test");

        let event = e.build().unwrap();

        assert_eq!(event.data, String::from("test"));
    }

    #[test]
    fn should_fill_event_type_field() {
        let mut e = EventBuilder::new();
        e.update("event: some_event");

        let event = e.build().unwrap();

        assert_eq!(event.type_, String::from("some_event"));
    }

    #[test]
    fn should_incrementally_fill_event_fields() {
        let expected_event = Event {
            type_: String::from("some_event"),
            data: String::from("test")
        };
        let mut e = EventBuilder::new();
        e.update("event: some_event");
        e.update("data: test");

        let event = e.build().unwrap();

        assert_eq!(event, expected_event);
    }

    #[test]
    fn should_change_status_to_empty_when_event_is_complete_and_empty_message_is_received() {
        let mut e = EventBuilder::new();
        e.update("data: test");
        e.update("");
        e.update("");

        let event = e.build();

        assert_eq!(e.state(), EventBuilderState::EMPTY);
        assert_eq!(event, None);
    }

    #[test]
    fn should_start_clean_event_after_previous_is_completed() {
        let expected_event = Event {
            type_: String::from("message"),
            data: String::from("test2")
        };
        let mut e = EventBuilder::new();
        e.update("event: some_event");
        e.update("data: test");
        e.update("");
        e.update("data: test2");

        let event = e.build().unwrap();

        assert_eq!(e.state(), EventBuilderState::PENDING);
        assert_eq!(event, expected_event);
    }

    #[test]
    fn should_ignore_updates_started_with_colon() {
        let expected_event = Event {
            type_: String::from("some_event"),
            data: String::from("test")
        };
        let mut e = EventBuilder::new();
        e.update("event: some_event");
        e.update(":some commentary");
        e.update("data: test");

        let event = e.build().unwrap();

        assert_eq!(event, expected_event);
    }
}
