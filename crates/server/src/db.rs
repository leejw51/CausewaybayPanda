//! One SQLite file per restaurant.

use std::path::Path;
use std::sync::Arc;

use causewaybay_panda_protocol::money::format_usdc;
use causewaybay_panda_protocol::seed::{self, TREASURY};
use causewaybay_panda_protocol::wire::{CartLine, MenuDraft, MenuItem, OrderView, PaymentView};
use causewaybay_panda_protocol::{Role, CAFE_NAME, CAFE_NAME_ZH, GUEST_GRANT};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub role: Role,
    pub name: String,
    pub balance_micro: i64,
}

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
                created_at TEXT NOT NULL
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

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
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
                        item.price_micro,
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
        let rows = stmt.query_map([], |r| Ok(row_item(r))).map_err(err)?;
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
            |r| Ok(row_item(r)),
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
        let price = causewaybay_panda_protocol::parse_usdc(&draft.price)
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
    ) -> Result<(String, i64, String), String> {
        let (lines, total, balance) = self.cart(session_id)?;
        if lines.is_empty() {
            return Err("cart is empty".into());
        }
        if debit && total > balance {
            return Err(format!(
                "need {} USDC, you have {}",
                format_usdc(total),
                format_usdc(balance)
            ));
        }
        let order_id = Uuid::new_v4().to_string();
        let pay_id = Uuid::new_v4().to_string();
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
            "INSERT INTO orders (id, session_id, guest, total_micro, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'paid', ?5)",
            params![order_id, session_id, guest, total, now],
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
        Ok((order_id, total, hash))
    }

    pub fn orders(&self) -> Result<Vec<OrderView>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, guest, total_micro, status, created_at FROM orders ORDER BY created_at DESC",
            )
            .map_err(err)?;
        let rows: Vec<(String, String, i64, String, String)> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let mut out = Vec::new();
        for (id, guest, total, status, created_at) in rows {
            let mut ls = conn
                .prepare(
                    "SELECT item_id, name, qty, unit_micro FROM order_lines WHERE order_id = ?1",
                )
                .map_err(err)?;
            let lines = ls
                .query_map(params![id], |r| {
                    let qty: i64 = r.get(2)?;
                    let unit: i64 = r.get(3)?;
                    Ok(CartLine {
                        item_id: r.get(0)?,
                        name: r.get(1)?,
                        qty: qty as u32,
                        unit_usdc: format_usdc(unit),
                        line_usdc: format_usdc(unit.saturating_mul(qty)),
                    })
                })
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?;
            out.push(OrderView {
                id,
                guest,
                total_usdc: format_usdc(total),
                status,
                created_at,
                lines,
            });
        }
        Ok(out)
    }

    pub fn payments(&self) -> Result<Vec<PaymentView>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, order_id, guest, amount_micro, method, tx_hash, status, created_at
                 FROM payments ORDER BY created_at DESC",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                let amount: i64 = r.get(3)?;
                Ok(PaymentView {
                    id: r.get(0)?,
                    order_id: r.get(1)?,
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

fn row_item(r: &rusqlite::Row<'_>) -> MenuItem {
    let price: i64 = r.get(4).unwrap_or(0);
    let avail: i64 = r.get(7).unwrap_or(1);
    MenuItem {
        id: r.get(0).unwrap_or_default(),
        name: r.get(1).unwrap_or_default(),
        name_zh: r.get(2).unwrap_or_default(),
        description: r.get(3).unwrap_or_default(),
        price_usdc: format_usdc(price),
        price_micro: price,
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
