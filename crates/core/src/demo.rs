//! The cafe running itself. Flip the switch and guests arrive, order and pay,
//! and the kitchen works the tickets — every step through the same `apply`
//! a real person's tap goes through, so what the counter shows is the real
//! thing, not a film of it.
//!
//! The driver is pure: `tick` takes a store and hands back the frames to fan
//! out. A server calls it from a timer task; a browser tab from `setInterval`.

use causewaybay_panda_protocol::intent::Intent;
use causewaybay_panda_protocol::wire::{PayMethod, Role};
use causewaybay_panda_protocol::COIN_NAME;

use crate::cafe::{apply, Apply};
use crate::shop::Shop;
use crate::store::{SessionRow, Store};

/// Regulars, so the counter shows names rather than numbers.
const REGULARS: &[&str] = &[
    "Mei", "Wing", "Ling", "Kwok", "Ah Fai", "Suet", "Chan", "Yuki", "Ho Yin", "Priya",
];

/// The queue is never let grow past this: a demo should look busy, not broken.
const MAX_OPEN: usize = 5;

/// xorshift64*. No crate, no entropy source: the host seeds it once.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

#[derive(Debug)]
pub struct Demo {
    rng: Rng,
    /// Simulated guests, opened lazily and reused so the session table does
    /// not grow with every tick.
    regulars: Vec<SessionRow>,
    /// The hands that move the tickets.
    kitchen: Option<SessionRow>,
    pub on: bool,
    /// Whether the host's ticker is alive. The host sets it when it starts
    /// one and clears it when that ticker ends, so a second "on" never starts
    /// a second beat — the cafe would run twice as fast.
    pub ticking: bool,
}

impl Demo {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::seeded(seed),
            regulars: Vec::new(),
            kitchen: None,
            on: false,
            ticking: false,
        }
    }

    /// Switch the cafe on or off. Only a simulation may run itself: real
    /// USDC is never moved by a script.
    pub fn set(&mut self, on: bool, shop: &Shop) -> Result<bool, String> {
        if on && !shop.is_simulation() {
            return Err(format!(
                "a live shop does not run on its own — that would spend real USDC, not {COIN_NAME}"
            ));
        }
        self.on = on;
        Ok(self.on)
    }

    /// One beat of the cafe. Returns what happened, as frames to fan out.
    pub fn tick(&mut self, db: &dyn Store, shop: &Shop) -> Vec<Apply> {
        let mut out = Vec::new();
        if !self.on {
            return out;
        }
        let open = db.open_orders().unwrap_or_default();

        // A new guest orders, unless the counter is already full.
        if open.len() < MAX_OPEN && self.rng.chance(70) {
            if let Some(a) = self.someone_orders(db, shop) {
                out.extend(a);
            }
        }

        // The kitchen moves the oldest ticket along.
        if let Some(oldest) = open.first() {
            if self.rng.chance(80) {
                let kitchen = match self.kitchen_hands(db) {
                    Some(k) => k,
                    None => return out,
                };
                out.push(apply(
                    db,
                    shop,
                    &kitchen,
                    Intent::OrderAdvance {
                        order_id: oldest.id.clone(),
                        status: String::new(),
                    },
                ));
            }
        }
        out
    }

    fn kitchen_hands(&mut self, db: &dyn Store) -> Option<SessionRow> {
        if self.kitchen.is_none() {
            self.kitchen = db.create_session(Role::Owner, "Kitchen").ok();
        }
        self.kitchen.clone()
    }

    fn someone_orders(&mut self, db: &dyn Store, shop: &Shop) -> Option<Vec<Apply>> {
        let board = db.menu_available().ok()?;
        if board.is_empty() {
            return None;
        }
        // Pick a regular, seating a new one now and then.
        let guest = if self.regulars.len() < REGULARS.len() && self.rng.chance(40) {
            let name = REGULARS[self.regulars.len()];
            let row = db.create_session(Role::Guest, name).ok()?;
            self.regulars.push(row.clone());
            row
        } else if self.regulars.is_empty() {
            let row = db.create_session(Role::Guest, REGULARS[0]).ok()?;
            self.regulars.push(row.clone());
            row
        } else {
            let i = self.rng.below(self.regulars.len());
            self.regulars[i].clone()
        };

        let mut out = Vec::new();
        // One to three dishes, with a favourite doubled now and then.
        let dishes = 1 + self.rng.below(3);
        for _ in 0..dishes {
            let d = &board[self.rng.below(board.len())];
            let qty = if self.rng.chance(20) { 2 } else { 1 };
            out.push(apply(
                db,
                shop,
                &guest,
                Intent::Add {
                    item_id: d.id.clone(),
                    qty,
                },
            ));
        }
        // Short of coin? A regular tops up rather than walking out.
        let (_, total, balance) = db.cart(&guest.id).ok()?;
        if total > balance {
            out.push(apply(db, shop, &guest, Intent::Faucet));
        }
        out.push(apply(
            db,
            shop,
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        ));
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemStore;
    use causewaybay_panda_protocol::wire::{OrderStatus, ServerMsg};

    fn count_paid(applies: &[Apply]) -> usize {
        applies
            .iter()
            .flat_map(|a| a.to_self.iter())
            .filter(|m| matches!(m, ServerMsg::Paid { .. }))
            .count()
    }

    #[test]
    fn off_does_nothing() {
        let db = MemStore::new("panda");
        let shop = Shop::simulation();
        let mut d = Demo::new(7);
        assert!(d.tick(&db, &shop).is_empty());
        assert!(db.orders().unwrap().is_empty());
    }

    #[test]
    fn switched_on_the_cafe_fills_and_works_its_own_tickets() {
        let db = MemStore::new("panda");
        let shop = Shop::simulation();
        let mut d = Demo::new(7);
        d.set(true, &shop).unwrap();
        let mut paid = 0;
        for _ in 0..40 {
            paid += count_paid(&d.tick(&db, &shop));
        }
        assert!(paid >= 10, "guests should keep ordering; got {paid}");
        let all = db.orders().unwrap();
        assert!(all.len() >= 10);
        // The kitchen has been moving them: some are finished.
        assert!(all.iter().any(|o| o.status == OrderStatus::Collected));
        // And it never let the counter overflow.
        assert!(db.open_orders().unwrap().len() <= MAX_OPEN + 1);
        // Money moved through the till, in coin.
        let t = db.takings_today().unwrap();
        assert_eq!(t.orders as usize, all.len());
        assert_eq!(t.coin_micro, t.total_micro);
        assert_eq!(t.wallet_micro, 0);
    }

    #[test]
    fn regulars_are_reused_not_minted_every_tick() {
        let db = MemStore::new("panda");
        let shop = Shop::simulation();
        let mut d = Demo::new(3);
        d.set(true, &shop).unwrap();
        for _ in 0..60 {
            d.tick(&db, &shop);
        }
        assert!(d.regulars.len() <= REGULARS.len());
        // Every order names one of the regulars.
        for o in db.orders().unwrap() {
            assert!(REGULARS.contains(&o.guest.as_str()), "{}", o.guest);
        }
    }

    #[test]
    fn a_live_shop_refuses_to_run_itself() {
        let live = Shop::resolve(
            &crate::shop::Config {
                mode: Some("live".into()),
                ..Default::default()
            },
            crate::settlement::Settle::resolve(
                &crate::settlement::Config {
                    chain_key: Some("cronos_mainnet".into()),
                    treasury: Some("0x1111111111111111111111111111111111111111".into()),
                    ..Default::default()
                },
                causewaybay_panda_protocol::seed::TREASURY,
            ),
        );
        let mut d = Demo::new(1);
        let err = d.set(true, &live).unwrap_err();
        assert!(err.contains("real USDC"));
        assert!(!d.on);
    }

    #[test]
    fn the_same_seed_tells_the_same_story() {
        let run = |seed| {
            let db = MemStore::new("panda");
            let shop = Shop::simulation();
            let mut d = Demo::new(seed);
            d.set(true, &shop).unwrap();
            for _ in 0..20 {
                d.tick(&db, &shop);
            }
            db.orders()
                .unwrap()
                .iter()
                .map(|o| (o.guest.clone(), o.total_usdc.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(42), run(42));
        assert_ne!(run(42), run(43));
    }
}
