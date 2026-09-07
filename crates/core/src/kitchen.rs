//! The panda working the kitchen. Switched on, a ticket that lands is
//! picked up on the next beat and called ready a few beats later — through
//! the same `apply` an owner's tap goes through, so the queue, the guest's
//! card and the dashboard all move as they would for a person. Handing
//! over stays a person's tap: a cup still has to cross the counter.
//!
//! Unlike the demo driver this spends nothing and seats nobody, so a live
//! shop may run it. The choice is kept in the shop's settings so a restart
//! finds the kitchen as it was left.

use std::collections::HashMap;

use causewaybay_panda_protocol::intent::Intent;
use causewaybay_panda_protocol::wire::{OrderStatus, Role};

use crate::cafe::{apply, Apply};
use crate::shop::Shop;
use crate::store::{SessionRow, Store};

/// Settings key the switch is kept under; "on" or absent.
pub const SETTING: &str = "kitchen.auto";

/// Beats a ticket spends being made before it is called ready.
pub const PREP_BEATS: u32 = 3;

#[derive(Debug, Default)]
pub struct Kitchen {
    pub on: bool,
    /// Whether the host's ticker is alive, so a second "on" never starts a
    /// second beat.
    pub ticking: bool,
    /// The hands that move the tickets: an owner session named for the panda.
    hands: Option<SessionRow>,
    /// Beats each open ticket has been watched for.
    beats: HashMap<String, u32>,
}

impl Kitchen {
    pub fn new() -> Self {
        Self::default()
    }

    /// As the shop last left it.
    pub fn from_store(db: &dyn Store) -> Self {
        Self {
            on: matches!(db.setting(SETTING).ok().flatten().as_deref(), Some("on")),
            ..Self::default()
        }
    }

    /// Throw the switch and remember it.
    pub fn set(&mut self, on: bool, db: &dyn Store) -> Result<bool, String> {
        db.set_setting(SETTING, if on { Some("on") } else { None })?;
        self.on = on;
        if !on {
            self.beats.clear();
        }
        Ok(on)
    }

    /// One beat. Every ticket that has waited long enough moves one step;
    /// what happened comes back as frames to fan out.
    pub fn tick(&mut self, db: &dyn Store, shop: &Shop) -> Vec<Apply> {
        let mut out = Vec::new();
        if !self.on {
            return out;
        }
        let open = db.open_orders().unwrap_or_default();
        // Forget tickets that left the queue by another hand.
        self.beats.retain(|id, _| open.iter().any(|o| o.id == *id));
        let Some(hands) = self.hands(db) else {
            return out;
        };
        for o in &open {
            let n = self.beats.entry(o.id.clone()).or_insert(0);
            *n += 1;
            let due = match o.status {
                OrderStatus::Placed => true,
                OrderStatus::Preparing => *n >= PREP_BEATS,
                _ => false,
            };
            if due {
                *n = 0;
                out.push(apply(
                    db,
                    shop,
                    &hands,
                    Intent::OrderAdvance {
                        order_id: o.id.clone(),
                        status: String::new(),
                    },
                ));
            }
        }
        out
    }

    fn hands(&mut self, db: &dyn Store) -> Option<SessionRow> {
        if self.hands.is_none() {
            self.hands = db.create_session(Role::Owner, "Panda").ok();
        }
        self.hands.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemStore;
    use causewaybay_panda_protocol::wire::{PayMethod, ServerMsg};

    fn paid_latte(db: &MemStore, shop: &Shop) -> String {
        let g = db.create_session(Role::Guest, "Mei").unwrap();
        apply(
            db,
            shop,
            &g,
            Intent::Add {
                item_id: "latte".into(),
                qty: 1,
            },
        );
        apply(
            db,
            shop,
            &g,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        db.open_orders().unwrap().last().unwrap().id.clone()
    }

    fn status(db: &MemStore, id: &str) -> OrderStatus {
        db.order(id).unwrap().unwrap().status
    }

    #[test]
    fn off_touches_nothing() {
        let db = MemStore::new("panda");
        let shop = Shop::simulation();
        let id = paid_latte(&db, &shop);
        let mut k = Kitchen::new();
        for _ in 0..10 {
            assert!(k.tick(&db, &shop).is_empty());
        }
        assert_eq!(status(&db, &id), OrderStatus::Placed);
    }

    #[test]
    fn a_ticket_is_picked_up_then_called_ready_and_waits_for_a_hand() {
        let db = MemStore::new("panda");
        let shop = Shop::simulation();
        let id = paid_latte(&db, &shop);
        let mut k = Kitchen::new();
        k.set(true, &db).unwrap();
        let first = k.tick(&db, &shop);
        assert_eq!(status(&db, &id), OrderStatus::Preparing);
        // The frames are the same an owner's tap would send.
        assert!(first[0]
            .to_owners
            .iter()
            .any(|m| matches!(m, ServerMsg::OrderUpdate { .. })));
        assert!(first[0]
            .to_owners
            .iter()
            .any(|m| matches!(m, ServerMsg::Dashboard { preparing: 1, .. })));
        for _ in 0..PREP_BEATS {
            k.tick(&db, &shop);
        }
        assert_eq!(status(&db, &id), OrderStatus::Ready);
        for _ in 0..10 {
            k.tick(&db, &shop);
        }
        assert_eq!(
            status(&db, &id),
            OrderStatus::Ready,
            "handing over is a person's tap"
        );
    }

    #[test]
    fn the_switch_is_kept_by_the_shop() {
        let db = MemStore::new("panda");
        let mut k = Kitchen::new();
        k.set(true, &db).unwrap();
        assert!(Kitchen::from_store(&db).on);
        k.set(false, &db).unwrap();
        assert!(!Kitchen::from_store(&db).on);
    }

    #[test]
    fn a_ticket_a_person_moved_is_not_moved_twice() {
        let db = MemStore::new("panda");
        let shop = Shop::simulation();
        let id = paid_latte(&db, &shop);
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        let mut k = Kitchen::new();
        k.set(true, &db).unwrap();
        k.tick(&db, &shop);
        // The owner calls it ready by hand and hands it over.
        apply(
            &db,
            &shop,
            &owner,
            Intent::OrderAdvance {
                order_id: id.clone(),
                status: "ready".into(),
            },
        );
        apply(
            &db,
            &shop,
            &owner,
            Intent::OrderAdvance {
                order_id: id.clone(),
                status: "collected".into(),
            },
        );
        assert!(k.tick(&db, &shop).is_empty());
        assert_eq!(status(&db, &id), OrderStatus::Collected);
    }
}
