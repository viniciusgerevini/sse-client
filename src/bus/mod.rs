use std::collections::HashMap;

pub struct PubSub {
    listeners: HashMap<String, Vec<fn(&str)>>
}

impl PubSub {
    pub fn new() -> PubSub {
        let listeners = HashMap::new();
        PubSub{ listeners }
    }
    pub fn subscribe(&mut self, event: &str, listener: fn(&str)) -> () {
         if self.listeners.contains_key(event) {
             self.listeners.get_mut(event).unwrap().push(listener);
         } else {
             self.listeners.insert(String::from(event), vec!(listener));
         }
    }
    pub fn publish(&self, event: &str, data: &str) {
        if self.listeners.contains_key(event) {
            self.listeners.get(event).unwrap().iter().for_each(|listener| {
                listener(data);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_to_event() {
        let mut  bus = PubSub::new();

        fn handler(message: &str) {
            assert_eq!(message, "test message");
        }

        fn do_not_call_me(message: &str) {
            panic!("should not call this listener {}", message);
        }

        bus.subscribe("event_name", handler);
        bus.subscribe("another_event", do_not_call_me);

        bus.publish("event_name", "test message");
    }

    #[test]
    fn register_more_than_one_listener_for_event() {
        let mut  bus = PubSub::new();

        fn handler(message: &str) {
            assert_eq!(message, "test message");
        }

        fn another_handler(message: &str) {
            assert_eq!(message, "test message");
        }

        bus.subscribe("event_name", handler);
        bus.subscribe("event_name", another_handler);

        bus.publish("event_name", "test message");
    }

    #[test]
    fn do_nothing_when_published_event_with_no_listeners() {
        let mut  bus = PubSub::new();
        bus.publish("event_name", "test message");
    }
}
