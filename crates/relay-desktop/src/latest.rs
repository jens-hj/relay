//! A latest-value handoff for complete network states. Keep at most one state
//! while the UI is busy, rather than queuing obsolete workspace snapshots.

use mosaic::prelude::{State, StateSender, state_channel};
use std::sync::{Arc, Mutex};

struct Slot<T> {
    value: Option<T>,
    closed: bool,
}

pub struct Sender<T> {
    slot: Arc<Mutex<Slot<T>>>,
    wake: StateSender<()>,
}

pub struct Receiver<T> {
    slot: Arc<Mutex<Slot<T>>>,
    wake: State<()>,
}

pub fn channel<T>(initial: T) -> (Receiver<T>, Sender<T>) {
    let (wake, notify) = state_channel(());
    let slot = Arc::new(Mutex::new(Slot {
        value: Some(initial),
        closed: false,
    }));
    (
        Receiver {
            slot: slot.clone(),
            wake,
        },
        Sender { slot, wake: notify },
    )
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), T> {
        let mut slot = self.slot.lock().expect("network state handoff");
        if slot.closed {
            return Err(value);
        }
        let notify = slot.value.replace(value).is_none();
        if notify && self.wake.send(()).is_err() {
            slot.closed = true;
            return Err(slot.value.take().expect("pending network state"));
        }
        Ok(())
    }
}

impl<T> Receiver<T> {
    /// Subscribe the current effect and consume the newest complete state.
    pub fn take(&self) -> Option<T> {
        self.wake.get();
        self.slot
            .lock()
            .expect("network state handoff")
            .value
            .take()
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut slot = self.slot.lock().expect("network state handoff");
        slot.closed = true;
        slot.value = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mosaic::{
        core::reactive::flush,
        prelude::{Effect, Scope},
    };
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn a_stalled_ui_receives_only_the_latest_state_and_wakes_for_the_next_batch() {
        let scope = Scope::new(|| {});
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sender = scope.run(|| {
            let (receiver, sender) = channel(0);
            let seen = seen.clone();
            Effect::new(move || {
                if let Some(value) = receiver.take() {
                    seen.borrow_mut().push(value);
                }
            });
            sender
        });
        flush();
        assert_eq!(*seen.borrow(), vec![0]);
        let sender = std::thread::spawn(move || {
            for value in 1..=10_000 {
                sender.send(value).unwrap();
            }
            sender
        })
        .join()
        .unwrap();
        assert_eq!(*seen.borrow(), vec![0]);
        flush();
        assert_eq!(*seen.borrow(), vec![0, 10_000]);
        sender.send(10_001).unwrap();
        flush();
        assert_eq!(*seen.borrow(), vec![0, 10_000, 10_001]);
        scope.dispose();
        assert_eq!(sender.send(10_002), Err(10_002));
    }
}
