use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Handler = Arc<dyn Fn(&dyn Any) + Send + Sync>;

#[derive(Default)]
pub struct EventEmitter {
    handlers: Mutex<HashMap<TypeId, Vec<Handler>>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on<E: Any + Send + Sync + 'static>(&self, handler: impl Fn(&E) + Send + Sync + 'static) {
        let wrapped: Handler = Arc::new(move |evt: &dyn Any| {
            if let Some(e) = evt.downcast_ref::<E>() {
                handler(e);
            }
        });
        let type_id = TypeId::of::<E>();
        self.handlers.lock().unwrap().entry(type_id).or_default().push(wrapped);
    }

    pub fn emit<E: Any + Send + Sync + 'static>(&self, event: E) {
        let type_id = TypeId::of::<E>();
        let guard = self.handlers.lock().unwrap();
        if let Some(handlers) = guard.get(&type_id) {
            for handler in handlers {
                handler(&event);
            }
        }
    }

    pub fn clear<E: Any>(&self) {
        self.handlers.lock().unwrap().remove(&TypeId::of::<E>());
    }

    pub fn listener_count<E: Any>(&self) -> usize {
        self.handlers.lock().unwrap()
            .get(&TypeId::of::<E>())
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)] struct MyEvent(i32);

    #[test]
    fn test_emit_and_receive() {
        let emitter = EventEmitter::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let r = received.clone();
        emitter.on(move |e: &MyEvent| r.lock().unwrap().push(e.0));
        emitter.emit(MyEvent(42));
        assert_eq!(*received.lock().unwrap(), vec![42]);
    }
}
