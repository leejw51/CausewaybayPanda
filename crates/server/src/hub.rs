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

    /// One named session, whoever they are. An order belongs to the guest who
    /// placed it, so it goes to them rather than to the whole room.
    pub fn to_session(&self, session_id: &str, msg: ServerMsg) {
        let tx = {
            let g = self.inner.lock();
            g.guests
                .get(session_id)
                .or_else(|| g.owners.get(session_id))
                .cloned()
        };
        if let Some(tx) = tx {
            let _ = tx.send(msg);
        }
    }

    /// Every open session, so a change to the shop can reach each one with
    /// its own fresh cart or figures.
    pub fn sessions(&self) -> Vec<(String, causewaybay_panda_protocol::wire::Role)> {
        use causewaybay_panda_protocol::wire::Role;
        let g = self.inner.lock();
        g.guests
            .keys()
            .map(|k| (k.clone(), Role::Guest))
            .chain(g.owners.keys().map(|k| (k.clone(), Role::Owner)))
            .collect()
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
    fn a_message_for_one_session_reaches_nobody_else() {
        let hub = Hub::new();
        let (mine, mut mine_rx) = mpsc::unbounded_channel();
        let (theirs, mut theirs_rx) = mpsc::unbounded_channel();
        hub.register_guest("g1".into(), mine);
        hub.register_guest("g2".into(), theirs);

        hub.to_session("g1", ServerMsg::Pong);
        assert!(matches!(mine_rx.try_recv(), Ok(ServerMsg::Pong)));
        assert!(
            theirs_rx.try_recv().is_err(),
            "another table must not hear it"
        );

        // A session nobody is holding open is simply dropped.
        hub.to_session("gone", ServerMsg::Pong);
    }

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
