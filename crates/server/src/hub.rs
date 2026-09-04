//! Fan-out of JSON events to open guest and owner sockets.

use std::collections::HashMap;
use std::sync::Arc;

use causewaybay_panda_protocol::wire::ServerMsg;
use parking_lot::Mutex;
use tokio::sync::mpsc;

pub type Tx = mpsc::UnboundedSender<ServerMsg>;

#[derive(Clone, Default)]
pub struct Hub {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    guests: HashMap<String, Tx>,
    owners: HashMap<String, Tx>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_guest(&self, session_id: String, tx: Tx) {
        self.inner.lock().guests.insert(session_id, tx);
    }

    pub fn register_owner(&self, session_id: String, tx: Tx) {
        self.inner.lock().owners.insert(session_id, tx);
    }

    pub fn drop_session(&self, session_id: &str) {
        let mut g = self.inner.lock();
        g.guests.remove(session_id);
        g.owners.remove(session_id);
    }

    pub fn to_owners(&self, msg: ServerMsg) {
        let owners: Vec<Tx> = self.inner.lock().owners.values().cloned().collect();
        for tx in owners {
            let _ = tx.send(msg.clone());
        }
    }

    pub fn to_guests(&self, msg: ServerMsg) {
        let guests: Vec<Tx> = self.inner.lock().guests.values().cloned().collect();
        for tx in guests {
            let _ = tx.send(msg.clone());
        }
    }

    pub fn guest_count(&self) -> usize {
        self.inner.lock().guests.len()
    }

    pub fn owner_count(&self) -> usize {
        self.inner.lock().owners.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use causewaybay_panda_protocol::wire::ServerMsg;

    #[test]
    fn owner_sees_broadcast() {
        let hub = Hub::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        hub.register_owner("o1".into(), tx);
        hub.to_owners(ServerMsg::Pong);
        assert!(matches!(rx.try_recv(), Ok(ServerMsg::Pong)));
        assert_eq!(hub.owner_count(), 1);
        hub.drop_session("o1");
        assert_eq!(hub.owner_count(), 0);
    }
}
