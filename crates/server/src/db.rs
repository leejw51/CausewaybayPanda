//! One SQLite file per restaurant.

use std::path::Path;
use std::sync::Arc;

use causewaybay_panda_protocol::denom::Denom;
use causewaybay_panda_protocol::money::format_usdc;
use causewaybay_panda_protocol::seed::{self, TREASURY};
use causewaybay_panda_protocol::wire::OrderStatus;
use causewaybay_panda_protocol::wire::{CartLine, MenuDraft, MenuItem, OrderView, PaymentView};
use causewaybay_panda_protocol::{Role, CAFE_NAME, CAFE_NAME_ZH, GUEST_GRANT};
use causewaybay_panda_protocol::{FAUCET_CAP, FAUCET_GRANT};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// How this shop writes money for people. Amounts are stored in
    /// micro-USDC; this only decides how they read.
    denom: Denom,
}

pub use causewaybay_panda_core::store::{SessionRow, Store, Takings};

impl Db {
    pub fn open(path: &Path, pin: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(err)?;
        Self::from_conn(conn, pin)
    }

    pub fn memory(pin: &str) -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(err)?;
        Self::from_conn(conn, pin)
    }

    fn from_conn(conn: Connection, pin: &str) -> Result<Self, String> {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            CREATE TABLE IF NOT EXISTS cafe (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                name TEXT NOT NULL,
                name_zh TEXT NOT NULL,
                pin_hash TEXT NOT NULL,
                treasury TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS menu (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                name_zh TEXT NOT NULL,
                description TEXT NOT NULL,
                price_micro INTEGER NOT NULL,
                category TEXT NOT NULL,
                image TEXT NOT NULL,
                available INTEGER NOT NULL DEFAULT 1,
                sort INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                role TEXT NOT NULL,
                name TEXT NOT NULL,
                balance_micro INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cart (
                session_id TEXT NOT NULL,
                item_id TEXT NOT NULL,
                qty INTEGER NOT NULL,
                PRIMARY KEY (session_id, item_id)
            );
            CREATE TABLE IF NOT EXISTS orders (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                guest TEXT NOT NULL,
                total_micro INTEGER NOT NULL,
                status TEXT NOT NULL,
                order_no INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS orders_by_session ON orders (session_id);
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS order_lines (
                order_id TEXT NOT NULL,
                item_id TEXT NOT NULL,
                name TEXT NOT NULL,
                qty INTEGER NOT NULL,
                unit_micro INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS payments (
                id TEXT PRIMARY KEY,
                order_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                guest TEXT NOT NULL,
                amount_micro INTEGER NOT NULL,
                method TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )
        .map_err(err)?;

        // A shop that predates order numbers gets the column added under it.
        let has_no: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('orders') WHERE name = 'order_no'")
            .and_then(|mut st| st.exists([]))
            .unwrap_or(false);
        if !has_no {
            conn.execute_batch(
                "ALTER TABLE orders ADD COLUMN order_no INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(err)?;
        }

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            denom: Denom::default(),
        };
        db.seed_if_empty(pin)?;
        Ok(db)
    }

    fn seed_if_empty(&self, pin: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM cafe", [], |r| r.get(0))
            .map_err(err)?;
        if n == 0 {
            conn.execute(
                "INSERT INTO cafe (id, name, name_zh, pin_hash, treasury) VALUES (1, ?1, ?2, ?3, ?4)",
                params![CAFE_NAME, CAFE_NAME_ZH, hash_pin(pin), TREASURY],
            )
            .map_err(err)?;
            for (i, item) in seed::cafe_menu().iter().enumerate() {
                conn.execute(
                    "INSERT INTO menu (id, name, name_zh, description, price_micro, category, image, available, sort)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
                    params![
                        item.id,
                        item.name,
                        item.name_zh,
                        item.description,
                        item.price_micro(),
                        item.category,
                        item.image,
                        i as i64
                    ],
                )
                .map_err(err)?;
            }
        }
        Ok(())
    }

    /// Board this shop in a different denomination.
    pub fn with_denom(mut self, denom: Denom) -> Self {
        self.denom = denom;
        self
    }

    pub fn denom(&self) -> &Denom {
        &self.denom
    }

    pub fn check_pin(&self, pin: &str) -> Result<bool, String> {
        let conn = self.conn.lock();
        let stored: String = conn
            .query_row("SELECT pin_hash FROM cafe WHERE id = 1", [], |r| r.get(0))
            .map_err(err)?;
        Ok(stored == hash_pin(pin))
    }

    pub fn treasury(&self) -> Result<String, String> {
        let conn = self.conn.lock();
        conn.query_row("SELECT treasury FROM cafe WHERE id = 1", [], |r| r.get(0))
            .map_err(err)
    }

    pub fn create_session(&self, role: Role, name: &str) -> Result<SessionRow, String> {
        let id = Uuid::new_v4().to_string();
        let name = if name.trim().is_empty() {
            match role {
                Role::Guest => "guest",
                Role::Owner => "owner",
            }
        } else {
            name.trim()
        };
        let balance = match role {
            Role::Guest => GUEST_GRANT,
            Role::Owner => 0,
        };
        let role_s = match role {
            Role::Guest => "guest",
            Role::Owner => "owner",
        };
        let now = now_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions (id, role, name, balance_micro, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, role_s, name, balance, now],
        )
        .map_err(err)?;
        Ok(SessionRow {
            id,
            role,
            name: name.to_string(),
            balance_micro: balance,
        })
    }

    pub fn session(&self, id: &str) -> Result<Option<SessionRow>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, role, name, balance_micro FROM sessions WHERE id = ?1",
            params![id],
            |r| {
                let role_s: String = r.get(1)?;
                let role = if role_s == "owner" {
                    Role::Owner
                } else {
                    Role::Guest
                };
                Ok(SessionRow {
                    id: r.get(0)?,
                    role,
                    name: r.get(2)?,
                    balance_micro: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(err)
    }

    pub fn menu(&self) -> Result<Vec<MenuItem>, String> {
        self.menu_filtered(false)
    }

    pub fn menu_available(&self) -> Result<Vec<MenuItem>, String> {
        self.menu_filtered(true)
    }

    fn menu_filtered(&self, only_available: bool) -> Result<Vec<MenuItem>, String> {
        let conn = self.conn.lock();
        let sql = if only_available {
            "SELECT id, name, name_zh, description, price_micro, category, image, available
             FROM menu WHERE available = 1 ORDER BY sort, name"
        } else {
            "SELECT id, name, name_zh, description, price_micro, category, image, available
             FROM menu ORDER BY sort, name"
        };
        let mut stmt = conn.prepare(sql).map_err(err)?;
        let denom = self.denom.clone();
        let rows = stmt
            .query_map([], move |r| Ok(row_item(r, &denom)))
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(err)?);
        }
        Ok(out)
    }

    pub fn item(&self, id: &str) -> Result<Option<MenuItem>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, name_zh, description, price_micro, category, image, available
             FROM menu WHERE id = ?1",
            params![id],
            |r| Ok(row_item(r, &self.denom)),
        )
        .optional()
        .map_err(err)
    }

    pub fn find_item_named(&self, hay: &str) -> Result<Option<MenuItem>, String> {
        let hay = hay.trim().to_lowercase();
        if hay.is_empty() {
            return Ok(None);
        }
        let mut best: Option<(MenuItem, usize)> = None;
        for item in self.menu()? {
            let names = causewaybay_panda_protocol::intent::aliases_from_names(
                &item.id,
                &item.name,
                &item.name_zh,
            );
            for n in names {
                if !n.is_empty() && (hay == n || hay.contains(&n)) {
                    let score = n.chars().count();
                    if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                        best = Some((item.clone(), score));
                    }
                }
            }
        }
        Ok(best.map(|(i, _)| i))
    }

    pub fn upsert_item(&self, draft: &MenuDraft) -> Result<MenuItem, String> {
        // The owner writes what the board will say, in the shop's own
        // denomination — not in the settlement unit behind it.
        let price = self
            .denom
            .parse(&draft.price)
            .ok_or_else(|| format!("bad price {}", draft.price))?;
        if price <= 0 {
            return Err("price must be positive".into());
        }
        let id = if draft.id.trim().is_empty() {
            slug(&draft.name)
        } else {
            draft.id.trim().to_string()
        };
        if id.is_empty() {
            return Err("need a name".into());
        }
        let conn = self.conn.lock();
        let sort: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM menu", [], |r| {
                r.get(0)
            })
            .map_err(err)?;
        conn.execute(
            "INSERT INTO menu (id, name, name_zh, description, price_micro, category, image, available, sort)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                name_zh = excluded.name_zh,
                description = excluded.description,
                price_micro = excluded.price_micro,
                category = excluded.category,
                image = CASE WHEN excluded.image = '' THEN menu.image ELSE excluded.image END,
                available = 1",
            params![
                id,
                draft.name.trim(),
                draft.name_zh.trim(),
                draft.description.trim(),
                price,
                if draft.category.trim().is_empty() {
                    "other"
                } else {
                    draft.category.trim()
                },
                draft.image.trim(),
                sort
            ],
        )
        .map_err(err)?;
        drop(conn);
        self.item(&id)?.ok_or_else(|| "upsert vanished".into())
    }

    pub fn hide_item(&self, id: &str) -> Result<bool, String> {
        self.set_available(id, false)
    }

    pub fn show_item(&self, id: &str) -> Result<bool, String> {
        self.set_available(id, true)
    }

    fn set_available(&self, id: &str, on: bool) -> Result<bool, String> {
        let conn = self.conn.lock();
        let n = conn
            .execute(
                "UPDATE menu SET available = ?2 WHERE id = ?1",
                params![id, if on { 1 } else { 0 }],
            )
            .map_err(err)?;
        Ok(n > 0)
    }

    pub fn add_to_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        let qty = qty.max(1) as i64;
        let item = self
            .item(item_id)?
            .ok_or_else(|| format!("no such item {item_id}"))?;
        if !item.available {
            return Err(format!("{} is not on today", item.name));
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO cart (session_id, item_id, qty) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, item_id) DO UPDATE SET qty = qty + excluded.qty",
            params![session_id, item_id, qty],
        )
        .map_err(err)?;
        Ok(())
    }

    pub fn remove_from_cart(
        &self,
        session_id: &str,
        item_id: &str,
        qty: u32,
    ) -> Result<(), String> {
        let qty = qty.max(1) as i64;
        let conn = self.conn.lock();
        let current: Option<i64> = conn
            .query_row(
                "SELECT qty FROM cart WHERE session_id = ?1 AND item_id = ?2",
                params![session_id, item_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let Some(current) = current else {
            return Ok(());
        };
        if current <= qty {
            conn.execute(
                "DELETE FROM cart WHERE session_id = ?1 AND item_id = ?2",
                params![session_id, item_id],
            )
            .map_err(err)?;
        } else {
            conn.execute(
                "UPDATE cart SET qty = qty - ?3 WHERE session_id = ?1 AND item_id = ?2",
                params![session_id, item_id, qty],
            )
            .map_err(err)?;
        }
        Ok(())
    }

    /// Put an exact count on a line. Zero takes it off the ticket.
    pub fn set_cart_qty(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        if qty == 0 {
            let conn = self.conn.lock();
            conn.execute(
                "DELETE FROM cart WHERE session_id = ?1 AND item_id = ?2",
                params![session_id, item_id],
            )
            .map_err(err)?;
            return Ok(());
        }
        let item = self
            .item(item_id)?
            .ok_or_else(|| format!("no such item {item_id}"))?;
        if !item.available {
            return Err(format!("{} is not on today", item.name));
        }
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO cart (session_id, item_id, qty) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, item_id) DO UPDATE SET qty = excluded.qty",
            params![session_id, item_id, qty as i64],
        )
        .map_err(err)?;
        Ok(())
    }

    /// Hand a simulation guest more test money. Returns the new balance.
    /// Refused once they are already holding the cap, so the faucet cannot be
    /// milked into a meaningless number.
    pub fn faucet(&self, session_id: &str) -> Result<i64, String> {
        let conn = self.conn.lock();
        let balance: i64 = conn
            .query_row(
                "SELECT balance_micro FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if balance >= FAUCET_CAP {
            return Err(format!(
                "you already have {}; the faucet stops there",
                self.denom.price(balance)
            ));
        }
        let next = (balance + FAUCET_GRANT).min(FAUCET_CAP);
        conn.execute(
            "UPDATE sessions SET balance_micro = ?2 WHERE id = ?1",
            params![session_id, next],
        )
        .map_err(err)?;
        Ok(next)
    }

    pub fn balance(&self, session_id: &str) -> Result<i64, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT balance_micro FROM sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .map_err(err)
    }

    pub fn clear_cart(&self, session_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM cart WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(err)?;
        Ok(())
    }

    pub fn cart(&self, session_id: &str) -> Result<(Vec<CartLine>, i64, i64), String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.item_id, m.name, c.qty, m.price_micro
                 FROM cart c JOIN menu m ON m.id = c.item_id
                 WHERE c.session_id = ?1 ORDER BY m.sort",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![session_id], |r| {
                let item_id: String = r.get(0)?;
                let name: String = r.get(1)?;
                let qty: i64 = r.get(2)?;
                let unit: i64 = r.get(3)?;
                Ok((item_id, name, qty, unit))
            })
            .map_err(err)?;
        let mut lines = Vec::new();
        let mut total = 0i64;
        for row in rows {
            let (item_id, name, qty, unit) = row.map_err(err)?;
            let line = unit.saturating_mul(qty);
            total = total.saturating_add(line);
            lines.push(CartLine {
                item_id,
                name,
                qty: qty as u32,
                unit_usdc: format_usdc(unit),
                line_usdc: format_usdc(line),
                unit_display: self.denom.price(unit),
                line_display: self.denom.price(line),
            });
        }
        let balance: i64 = conn
            .query_row(
                "SELECT balance_micro FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok((lines, total, balance))
    }

    /// `debit` draws on the session's play-money grant. An on-chain payment
    /// has already moved real USDC, so it settles the order without touching
    /// the grant or checking it.
    pub fn checkout(
        &self,
        session_id: &str,
        method: &str,
        tx_hash: &str,
        debit: bool,
    ) -> Result<(String, i64, i64, String), String> {
        let (lines, total, balance) = self.cart(session_id)?;
        if lines.is_empty() {
            return Err("cart is empty".into());
        }
        if debit && total > balance {
            return Err(format!(
                "you need {} and have {} — tap Top up",
                self.denom.price(total),
                self.denom.price(balance)
            ));
        }
        let order_id = Uuid::new_v4().to_string();
        let pay_id = Uuid::new_v4().to_string();
        let order_no = {
            let conn = self.conn.lock();
            let n: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(order_no), 0) + 1 FROM orders",
                    [],
                    |r| r.get(0),
                )
                .map_err(err)?;
            n
        };
        let hash = if tx_hash.trim().is_empty() {
            format!("demo-{}", &order_id[..8])
        } else {
            tx_hash.trim().to_string()
        };
        let now = now_rfc3339();
        // Debit, order, lines and payment land together or not at all.
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(err)?;
        // A chain transaction pays exactly one order.
        let reused: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM payments WHERE lower(tx_hash) = lower(?1)",
                params![hash],
                |r| r.get(0),
            )
            .map_err(err)?;
        if reused > 0 {
            return Err("that transaction has already paid for an order".into());
        }
        let guest: String = tx
            .query_row(
                "SELECT name FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if debit {
            tx.execute(
                "UPDATE sessions SET balance_micro = balance_micro - ?2 WHERE id = ?1",
                params![session_id, total],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO orders (id, session_id, guest, total_micro, status, order_no, created_at)
             VALUES (?1, ?2, ?3, ?4, 'placed', ?5, ?6)",
            params![order_id, session_id, guest, total, order_no, now],
        )
        .map_err(err)?;
        for line in &lines {
            let unit = causewaybay_panda_protocol::parse_usdc(&line.unit_usdc).unwrap_or(0);
            tx.execute(
                "INSERT INTO order_lines (order_id, item_id, name, qty, unit_micro)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![order_id, line.item_id, line.name, line.qty as i64, unit],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO payments (id, order_id, session_id, guest, amount_micro, method, tx_hash, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'confirmed', ?8)",
            params![pay_id, order_id, session_id, guest, total, method, hash, now],
        )
        .map_err(err)?;
        tx.execute(
            "DELETE FROM cart WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok((order_id, order_no, total, hash))
    }

    /// Every order, newest first — the owner's book.
    pub fn orders(&self) -> Result<Vec<OrderView>, String> {
        self.orders_where("1 = 1", [])
    }

    /// Today's orders by this Mac's clock, the same day the takings count.
    pub fn orders_today(&self) -> Result<Vec<OrderView>, String> {
        self.orders_where(
            "date(created_at, 'localtime') = date('now', 'localtime')",
            [],
        )
    }

    /// Just what the kitchen still owes somebody, oldest first so the queue
    /// reads in the order people arrived.
    pub fn open_orders(&self) -> Result<Vec<OrderView>, String> {
        self.orders_where(
            "status IN ('placed', 'preparing', 'ready') ORDER BY order_no ASC",
            [],
        )
    }

    /// One guest's orders, for the card they watch while they wait.
    pub fn orders_for_session(&self, session_id: &str) -> Result<Vec<OrderView>, String> {
        self.orders_where("session_id = ?1", params![session_id])
    }

    /// The session that placed an order, so an update reaches that table.
    pub fn order_session(&self, id: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT session_id FROM orders WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn order(&self, id: &str) -> Result<Option<OrderView>, String> {
        Ok(self
            .orders_where("id = ?1", params![id])?
            .into_iter()
            .next())
    }

    fn orders_where<P: rusqlite::Params>(
        &self,
        clause: &str,
        args: P,
    ) -> Result<Vec<OrderView>, String> {
        let conn = self.conn.lock();
        let ordered = clause.contains("ORDER BY");
        let sql = format!(
            "SELECT id, guest, total_micro, status, created_at, order_no, session_id
             FROM orders WHERE {clause}{}",
            if ordered {
                ""
            } else {
                " ORDER BY order_no DESC"
            }
        );
        let mut stmt = conn.prepare(&sql).map_err(err)?;
        let rows: Vec<(String, String, i64, String, String, i64, String)> = stmt
            .query_map(args, |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;

        let mut out = Vec::new();
        for (id, guest, total, status, created_at, order_no, _session) in rows {
            let mut ls = conn
                .prepare(
                    "SELECT item_id, name, qty, unit_micro FROM order_lines WHERE order_id = ?1",
                )
                .map_err(err)?;
            let denom = self.denom.clone();
            let lines = ls
                .query_map(params![id], move |r| {
                    let qty: i64 = r.get(2)?;
                    let unit: i64 = r.get(3)?;
                    Ok(CartLine {
                        item_id: r.get(0)?,
                        name: r.get(1)?,
                        qty: qty as u32,
                        unit_usdc: format_usdc(unit),
                        line_usdc: format_usdc(unit.saturating_mul(qty)),
                        unit_display: denom.price(unit),
                        line_display: denom.price(unit.saturating_mul(qty)),
                    })
                })
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            out.push(OrderView {
                id,
                order_no,
                guest,
                total_usdc: format_usdc(total),
                total_display: self.denom.price(total),
                // A row written before the lifecycle existed reads as placed.
                status: OrderStatus::parse(&status).unwrap_or(OrderStatus::Placed),
                created_at,
                lines,
            });
        }
        Ok(out)
    }

    /// Move an order along. `to` must be the step that actually follows, or
    /// cancelled; anything else is refused so the board cannot skip ahead.
    /// Move an order along. `to` must be the step that actually follows, or
    /// cancelled; anything else is refused so the board cannot skip ahead.
    pub fn set_order_status(&self, id: &str, to: OrderStatus) -> Result<OrderView, String> {
        // Decide under the lock, then read the order back once it is released:
        // the connection mutex is not reentrant.
        {
            let conn = self.conn.lock();
            let current: String = conn
                .query_row(
                    "SELECT status FROM orders WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?
                .ok_or("no such order")?;
            let current = OrderStatus::parse(&current).unwrap_or(OrderStatus::Placed);
            if current != to {
                let allowed = current.next() == Some(to)
                    || (to == OrderStatus::Cancelled && current.is_open());
                if !allowed {
                    return Err(format!(
                        "an order that is {} cannot become {}",
                        current.as_str(),
                        to.as_str()
                    ));
                }
                conn.execute(
                    "UPDATE orders SET status = ?2 WHERE id = ?1",
                    params![id, to.as_str()],
                )
                .map_err(err)?;
            }
        }
        self.order(id)?.ok_or_else(|| "no such order".into())
    }

    /// What the shop has taken today, by this Mac's clock. "Today" is the
    /// owner's day, not UTC's: a 7am order in Causeway Bay is still yesterday
    /// in Greenwich.
    pub fn takings_today(&self) -> Result<Takings, String> {
        let conn = self.conn.lock();
        let (total, orders, coin, wallet): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_micro), 0),
                        COUNT(*),
                        COALESCE(SUM(CASE WHEN method = 'wallet' THEN 0 ELSE amount_micro END), 0),
                        COALESCE(SUM(CASE WHEN method = 'wallet' THEN amount_micro ELSE 0 END), 0)
                 FROM payments
                 WHERE status = 'confirmed'
                   AND date(created_at, 'localtime') = date('now', 'localtime')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(err)?;
        Ok(Takings {
            total_micro: total,
            orders,
            coin_micro: coin,
            wallet_micro: wallet,
        })
    }

    pub fn payments(&self) -> Result<Vec<PaymentView>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, order_id, guest, amount_micro, method, tx_hash, status, created_at
                 FROM payments ORDER BY created_at DESC",
            )
            .map_err(err)?;
        let denom = self.denom.clone();
        let rows = stmt
            .query_map([], move |r| {
                let amount: i64 = r.get(3)?;
                Ok(PaymentView {
                    id: r.get(0)?,
                    order_id: r.get(1)?,
                    amount_display: denom.price(amount),
                    guest: r.get(2)?,
                    amount_usdc: format_usdc(amount),
                    method: r.get(4)?,
                    tx_hash: r.get(5)?,
                    status: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }
}

fn row_item(r: &rusqlite::Row<'_>, denom: &Denom) -> MenuItem {
    let price: i64 = r.get(4).unwrap_or(0);
    let avail: i64 = r.get(7).unwrap_or(1);
    MenuItem {
        id: r.get(0).unwrap_or_default(),
        name: r.get(1).unwrap_or_default(),
        name_zh: r.get(2).unwrap_or_default(),
        description: r.get(3).unwrap_or_default(),
        price_usdc: format_usdc(price),
        price_micro: price,
        price_display: denom.price(price),
        category: r.get(5).unwrap_or_default(),
        image: r.get(6).unwrap_or_default(),
        available: avail != 0,
    }
}

fn hash_pin(pin: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"causewaybay-panda-pin-v1:");
    h.update(pin.as_bytes());
    hex::encode(h.finalize())
}

fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use causewaybay_panda_protocol::GUEST_GRANT;

    #[test]
    fn seed_menu_and_guest_grant() {
        let db = Db::memory("panda").unwrap();
        let menu = db.menu().unwrap();
        assert_eq!(menu.len(), seed::cafe_menu().len());
        assert!(menu.iter().any(|i| i.id == "latte"));
        let s = db.create_session(Role::Guest, "Mei").unwrap();
        assert_eq!(s.balance_micro, GUEST_GRANT);
        assert!(db.check_pin("panda").unwrap());
        assert!(!db.check_pin("wrong").unwrap());
    }

    #[test]
    fn cart_add_remove_clear() {
        let db = Db::memory("panda").unwrap();
        let s = db.create_session(Role::Guest, "Mei").unwrap();
        db.add_to_cart(&s.id, "latte", 2).unwrap();
        db.add_to_cart(&s.id, "egg_tart", 1).unwrap();
        let (lines, total, _) = db.cart(&s.id).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.iter().find(|l| l.item_id == "latte").unwrap().qty, 2);
        assert!(total > 0);
        db.remove_from_cart(&s.id, "latte", 1).unwrap();
        let (lines, _, _) = db.cart(&s.id).unwrap();
        assert_eq!(lines.iter().find(|l| l.item_id == "latte").unwrap().qty, 1);
        db.clear_cart(&s.id).unwrap();
        let (lines, total, _) = db.cart(&s.id).unwrap();
        assert!(lines.is_empty());
        assert_eq!(total, 0);
    }
}

/// The SQLite store is the cafe's `Store`. Every method already exists on
/// `Db`; this just says so, and the browser's `MemStore` says the same.
impl Store for Db {
    fn denom(&self) -> &Denom {
        Db::denom(self)
    }
    fn check_pin(&self, pin: &str) -> Result<bool, String> {
        Db::check_pin(self, pin)
    }
    fn treasury(&self) -> Result<String, String> {
        Db::treasury(self)
    }
    fn create_session(&self, role: Role, name: &str) -> Result<SessionRow, String> {
        Db::create_session(self, role, name)
    }
    fn session(&self, id: &str) -> Result<Option<SessionRow>, String> {
        Db::session(self, id)
    }
    fn menu(&self) -> Result<Vec<MenuItem>, String> {
        Db::menu(self)
    }
    fn menu_available(&self) -> Result<Vec<MenuItem>, String> {
        Db::menu_available(self)
    }
    fn item(&self, id: &str) -> Result<Option<MenuItem>, String> {
        Db::item(self, id)
    }
    fn find_item_named(&self, hay: &str) -> Result<Option<MenuItem>, String> {
        Db::find_item_named(self, hay)
    }
    fn upsert_item(&self, draft: &MenuDraft) -> Result<MenuItem, String> {
        Db::upsert_item(self, draft)
    }
    fn hide_item(&self, id: &str) -> Result<bool, String> {
        Db::hide_item(self, id)
    }
    fn show_item(&self, id: &str) -> Result<bool, String> {
        Db::show_item(self, id)
    }
    fn add_to_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        Db::add_to_cart(self, session_id, item_id, qty)
    }
    fn remove_from_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        Db::remove_from_cart(self, session_id, item_id, qty)
    }
    fn set_cart_qty(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        Db::set_cart_qty(self, session_id, item_id, qty)
    }
    fn clear_cart(&self, session_id: &str) -> Result<(), String> {
        Db::clear_cart(self, session_id)
    }
    fn cart(&self, session_id: &str) -> Result<(Vec<CartLine>, i64, i64), String> {
        Db::cart(self, session_id)
    }
    fn faucet(&self, session_id: &str) -> Result<i64, String> {
        Db::faucet(self, session_id)
    }
    fn balance(&self, session_id: &str) -> Result<i64, String> {
        Db::balance(self, session_id)
    }
    fn checkout(
        &self,
        session_id: &str,
        method: &str,
        tx_hash: &str,
        debit: bool,
    ) -> Result<(String, i64, i64, String), String> {
        Db::checkout(self, session_id, method, tx_hash, debit)
    }
    fn orders(&self) -> Result<Vec<OrderView>, String> {
        Db::orders(self)
    }
    fn orders_today(&self) -> Result<Vec<OrderView>, String> {
        Db::orders_today(self)
    }
    fn open_orders(&self) -> Result<Vec<OrderView>, String> {
        Db::open_orders(self)
    }
    fn orders_for_session(&self, session_id: &str) -> Result<Vec<OrderView>, String> {
        Db::orders_for_session(self, session_id)
    }
    fn order(&self, id: &str) -> Result<Option<OrderView>, String> {
        Db::order(self, id)
    }
    fn order_session(&self, id: &str) -> Option<String> {
        Db::order_session(self, id)
    }
    fn set_order_status(&self, id: &str, to: OrderStatus) -> Result<OrderView, String> {
        Db::set_order_status(self, id, to)
    }
    fn takings_today(&self) -> Result<Takings, String> {
        Db::takings_today(self)
    }
    fn payments(&self) -> Result<Vec<PaymentView>, String> {
        Db::payments(self)
    }
    fn setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)
    }
    fn set_setting(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let conn = self.conn.lock();
        match value {
            Some(v) => conn
                .execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, v],
                )
                .map_err(err)?,
            None => conn
                .execute("DELETE FROM settings WHERE key = ?1", params![key])
                .map_err(err)?,
        };
        Ok(())
    }
}
