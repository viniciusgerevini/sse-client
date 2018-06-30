use std::collections::HashMap;

pub struct Bus<ListenerData> where ListenerData: Clone {
    listeners: HashMap<String, Vec<Box<Fn(ListenerData) + Send>>>
}

impl<ListenerData> Bus <ListenerData> where ListenerData: Clone {
    pub fn new() -> Bus<ListenerData> {
        let listeners = HashMap::new();
        Bus { listeners }
    }

    pub fn subscribe<F>(&mut self, name: String, listener: F) where F: Fn(ListenerData) + Send + 'static {
        let listener = Box::new(listener);
        if self.listeners.contains_key(&name) {
            self.listeners.get_mut(&name).unwrap().push(listener);
        } else {
            self.listeners.insert(name, vec!(listener));
        }
    }

    pub fn publish(&self, name: String, data: ListenerData) {
        if self.listeners.contains_key(&name) {
            for listener in self.listeners.get(&name).unwrap().iter() {
                listener(data.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn should_trigger_listener() {
        let (tx, rx) = mpsc::channel();
        let mut bus: Bus<String> = Bus::new();
        bus.subscribe("event".to_string(), move |data| {
            tx.send(data).unwrap();
        });

        bus.publish("event".to_string(), "data".to_string());

        let message = rx.recv().unwrap();
        assert_eq!(message, "data");
    }

    #[test]
    fn should_allow_multiple_listener_for_same_event() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let mut bus = Bus::new();

        bus.subscribe("event".to_string(), move |data| {
            tx.send(data).unwrap();
        });

        bus.subscribe("event".to_string(), move |data| {
            tx2.send(data).unwrap();
        });

        bus.publish("event".to_string(), "data".to_string());

        let message = rx.recv().unwrap();
        let message2 = rx.recv().unwrap();
        assert_eq!(message, "data");
        assert_eq!(message2, "data");
    }

    #[test]
    fn should_trigger_only_listener_for_published_event() {
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let mut bus = Bus::new();

        bus.subscribe("event".to_string(), move |data| {
            tx.send(data).unwrap();
        });

        bus.subscribe("another event".to_string(), move |data| {
            tx2.send(data).unwrap();
        });

        bus.publish("event".to_string(), "data".to_string());

        let message = rx.recv().unwrap();
        assert_eq!(message, "data");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn should_work_with_other_types() {
        let (tx, rx) = mpsc::channel();
        let mut bus: Bus<i32> = Bus::new();
        bus.subscribe("event".to_string(), move |data| {
            tx.send(data).unwrap();
        });

        bus.publish("event".to_string(), 3);

        let message = rx.recv().unwrap();
        assert_eq!(message, 3);
    }

    #[test]
    fn should_not_break_when_publish_to_event_with_no_subscribers() {
        let bus: Bus<String> = Bus::new();
        bus.publish("event".to_string(), "data".to_string());
    }
}
