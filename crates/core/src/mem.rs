//! The cafe kept in memory. This is what a browser tab runs the whole shop on:
//! the same `Store` the server fills from SQLite, held in a struct instead.
//! It serialises whole, so a tab can put it in `localStorage` and pick up
//! where it left off.
//!
//! There is no clock in WebAssembly worth trusting, so the host tells the
//! store what time it is (`set_clock`). "Today" for the takings is whatever
//! day-key the host hands over — the browser's local date, not UTC's.

use std::cell::RefCell;
use std::collections::BTreeMap;

use causewaybay_panda_protocol::denom::Denom;
use causewaybay_panda_protocol::intent::aliases_from_names;
use causewaybay_panda_protocol::money::format_usdc;
use causewaybay_panda_protocol::seed::{self, TREASURY};
use causewaybay_panda_protocol::wire::{
    CartLine, MenuDraft, MenuItem, OrderStatus, OrderView, PaymentView, Role,
};
use causewaybay_panda_protocol::{FAUCET_CAP, FAUCET_GRANT, GUEST_GRANT};
use serde::{Deserialize, Serialize};

use crate::store::{SessionRow, Store, Takings};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Item {
    id: String,
    name: String,
    name_zh: String,
    description: String,
    price_micro: i64,
    category: String,
    image: String,
    available: bool,
    sort: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    id: String,
    role: Role,
    name: String,
    balance_micro: i64,
    /// item id → qty
    cart: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Line {
    item_id: String,
    name: String,
    qty: u32,
    unit_micro: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Order {
    id: String,
    order_no: i64,
    session_id: String,
    guest: String,
    total_micro: i64,
    status: OrderStatus,
    created_ms: i64,
    day: String,
    lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Payment {
    id: String,
    order_id: String,
    session_id: String,
    guest: String,
    amount_micro: i64,
    method: String,
    tx_hash: String,
    created_ms: i64,
    day: String,
}

/// Everything, in one serialisable piece.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    pin: String,
    treasury: String,
    menu: Vec<Item>,
    sessions: Vec<Session>,
    orders: Vec<Order>,
    payments: Vec<Payment>,
    next_id: u64,
    #[serde(default)]
    now_ms: i64,
    #[serde(default)]
    today: String,
    #[serde(default)]
    settings: BTreeMap<String, String>,
}

/// A whole cafe in a struct. `&self` methods with interior mutability, like
/// the SQLite store, so the trait is the same shape on both.
#[derive(Debug)]
pub struct MemStore {
    denom: RefCell<Denom>,
    st: RefCell<State>,
}

impl MemStore {
    /// A fresh shop with the seed menu and the given owner pin.
    pub fn new(pin: &str) -> Self {
        let menu = seed::cafe_menu()
            .iter()
            .enumerate()
            .map(|(i, s)| Item {
                id: s.id.into(),
                name: s.name.into(),
                name_zh: s.name_zh.into(),
                description: s.description.into(),
                price_micro: s.price_micro(),
                category: s.category.into(),
                image: s.image.into(),
                available: true,
                sort: i as i64,
            })
            .collect();
        Self {
            denom: RefCell::new(Denom::default()),
            st: RefCell::new(State {
                pin: pin.to_string(),
                treasury: TREASURY.into(),
                menu,
                sessions: Vec::new(),
                orders: Vec::new(),
                payments: Vec::new(),
                next_id: 1,
                now_ms: 0,
                today: String::new(),
                settings: BTreeMap::new(),
            }),
        }
    }

    pub fn with_denom(self, denom: Denom) -> Self {
        self.set_denom(denom);
        self
    }

    /// Board the shop in a different money from now on.
    pub fn set_denom(&self, denom: Denom) {
        *self.denom.borrow_mut() = denom;
    }

    /// The host says what time it is. `today` is any stable key for the
    /// local day, e.g. "2026-09-05".
    pub fn set_clock(&self, now_ms: i64, today: &str) {
        let mut st = self.st.borrow_mut();
        st.now_ms = now_ms;
        st.today = today.to_string();
    }

    /// The whole shop as JSON, for a tab to keep.
    pub fn snapshot(&self) -> String {
        serde_json::to_string(&*self.st.borrow()).unwrap_or_else(|_| "{}".into())
    }

    /// A shop back from `snapshot`. Anything unreadable starts fresh.
    pub fn restore(json: &str, pin: &str) -> Self {
        match serde_json::from_str::<State>(json) {
            Ok(st) if !st.menu.is_empty() => Self {
                denom: RefCell::new(Denom::default()),
                st: RefCell::new(st),
            },
            _ => Self::new(pin),
        }
    }

    fn id(&self, prefix: &str) -> String {
        let mut st = self.st.borrow_mut();
        let n = st.next_id;
        st.next_id += 1;
        format!("{prefix}-{n:06}")
    }

    fn view(&self, it: &Item) -> MenuItem {
        MenuItem {
            id: it.id.clone(),
            name: it.name.clone(),
            name_zh: it.name_zh.clone(),
            description: it.description.clone(),
            price_usdc: format_usdc(it.price_micro),
            price_micro: it.price_micro,
            price_display: self.denom().price(it.price_micro),
            category: it.category.clone(),
            image: it.image.clone(),
            available: it.available,
        }
    }

    fn line_view(&self, l: &Line) -> CartLine {
        let line = l.unit_micro.saturating_mul(l.qty as i64);
        CartLine {
            item_id: l.item_id.clone(),
            name: l.name.clone(),
            qty: l.qty,
            unit_usdc: format_usdc(l.unit_micro),
            line_usdc: format_usdc(line),
            unit_display: self.denom().price(l.unit_micro),
            line_display: self.denom().price(line),
        }
    }

    fn order_view(&self, o: &Order) -> OrderView {
        OrderView {
            id: o.id.clone(),
            order_no: o.order_no,
            guest: o.guest.clone(),
            total_usdc: format_usdc(o.total_micro),
            total_display: self.denom().price(o.total_micro),
            status: o.status,
            created_at: format!("{}", o.created_ms),
            lines: o.lines.iter().map(|l| self.line_view(l)).collect(),
        }
    }

    fn with_session<T>(
        &self,
        session_id: &str,
        f: impl FnOnce(&mut Session, &[Item]) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut st = self.st.borrow_mut();
        let menu = st.menu.clone();
        let s = st
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .ok_or("no such session")?;
        f(s, &menu)
    }
}

impl Store for MemStore {
    fn denom(&self) -> Denom {
        self.denom.borrow().clone()
    }

    fn check_pin(&self, pin: &str) -> Result<bool, String> {
        Ok(self.st.borrow().pin == pin)
    }

    fn set_pin(&self, pin: &str) -> Result<(), String> {
        self.st.borrow_mut().pin = pin.to_string();
        Ok(())
    }

    fn treasury(&self) -> Result<String, String> {
        Ok(self.st.borrow().treasury.clone())
    }

    fn create_session(&self, role: Role, name: &str) -> Result<SessionRow, String> {
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
        let id = self.id(match role {
            Role::Guest => "g",
            Role::Owner => "o",
        });
        self.st.borrow_mut().sessions.push(Session {
            id: id.clone(),
            role,
            name: name.into(),
            balance_micro: balance,
            cart: BTreeMap::new(),
        });
        Ok(SessionRow {
            id,
            role,
            name: name.into(),
            balance_micro: balance,
        })
    }

    fn session(&self, id: &str) -> Result<Option<SessionRow>, String> {
        Ok(self
            .st
            .borrow()
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| SessionRow {
                id: s.id.clone(),
                role: s.role,
                name: s.name.clone(),
                balance_micro: s.balance_micro,
            }))
    }

    fn menu(&self) -> Result<Vec<MenuItem>, String> {
        let st = self.st.borrow();
        let mut items: Vec<&Item> = st.menu.iter().collect();
        items.sort_by(|a, b| a.sort.cmp(&b.sort).then(a.name.cmp(&b.name)));
        Ok(items.into_iter().map(|i| self.view(i)).collect())
    }

    fn menu_available(&self) -> Result<Vec<MenuItem>, String> {
        Ok(self.menu()?.into_iter().filter(|i| i.available).collect())
    }

    fn item(&self, id: &str) -> Result<Option<MenuItem>, String> {
        Ok(self
            .st
            .borrow()
            .menu
            .iter()
            .find(|i| i.id == id)
            .map(|i| self.view(i)))
    }

    fn find_item_named(&self, hay: &str) -> Result<Option<MenuItem>, String> {
        let hay = hay.trim().to_lowercase();
        if hay.is_empty() {
            return Ok(None);
        }
        let mut best: Option<(MenuItem, usize)> = None;
        for item in self.menu()? {
            for n in aliases_from_names(&item.id, &item.name, &item.name_zh) {
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

    fn upsert_item(&self, draft: &MenuDraft) -> Result<MenuItem, String> {
        // The owner writes what the board will say, in the shop's denomination.
        let price = self
            .denom()
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
        let category = if draft.category.trim().is_empty() {
            "other"
        } else {
            draft.category.trim()
        };
        {
            let mut st = self.st.borrow_mut();
            let sort = st.menu.iter().map(|i| i.sort).max().unwrap_or(0) + 1;
            match st.menu.iter_mut().find(|i| i.id == id) {
                Some(it) => {
                    it.name = draft.name.trim().into();
                    it.name_zh = draft.name_zh.trim().into();
                    it.description = draft.description.trim().into();
                    it.price_micro = price;
                    it.category = category.into();
                    if !draft.image.trim().is_empty() {
                        it.image = draft.image.trim().into();
                    }
                    it.available = true;
                }
                None => st.menu.push(Item {
                    id: id.clone(),
                    name: draft.name.trim().into(),
                    name_zh: draft.name_zh.trim().into(),
                    description: draft.description.trim().into(),
                    price_micro: price,
                    category: category.into(),
                    image: draft.image.trim().into(),
                    available: true,
                    sort,
                }),
            }
        }
        self.item(&id)?.ok_or_else(|| "upsert vanished".into())
    }

    fn hide_item(&self, id: &str) -> Result<bool, String> {
        let mut st = self.st.borrow_mut();
        Ok(match st.menu.iter_mut().find(|i| i.id == id) {
            Some(it) => {
                it.available = false;
                true
            }
            None => false,
        })
    }

    fn show_item(&self, id: &str) -> Result<bool, String> {
        let mut st = self.st.borrow_mut();
        Ok(match st.menu.iter_mut().find(|i| i.id == id) {
            Some(it) => {
                it.available = true;
                true
            }
            None => false,
        })
    }

    fn add_to_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        let qty = qty.max(1);
        self.with_session(session_id, |s, menu| {
            let it = menu
                .iter()
                .find(|i| i.id == item_id)
                .ok_or_else(|| format!("no such item {item_id}"))?;
            if !it.available {
                return Err(format!("{} is not on today", it.name));
            }
            *s.cart.entry(item_id.into()).or_insert(0) += qty;
            Ok(())
        })
    }

    fn remove_from_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        let qty = qty.max(1);
        self.with_session(session_id, |s, _| {
            if let Some(n) = s.cart.get_mut(item_id) {
                if *n <= qty {
                    s.cart.remove(item_id);
                } else {
                    *n -= qty;
                }
            }
            Ok(())
        })
    }

    fn set_cart_qty(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String> {
        self.with_session(session_id, |s, menu| {
            if qty == 0 {
                s.cart.remove(item_id);
                return Ok(());
            }
            let it = menu
                .iter()
                .find(|i| i.id == item_id)
                .ok_or_else(|| format!("no such item {item_id}"))?;
            if !it.available {
                return Err(format!("{} is not on today", it.name));
            }
            s.cart.insert(item_id.into(), qty);
            Ok(())
        })
    }

    fn clear_cart(&self, session_id: &str) -> Result<(), String> {
        self.with_session(session_id, |s, _| {
            s.cart.clear();
            Ok(())
        })
    }

    fn cart(&self, session_id: &str) -> Result<(Vec<CartLine>, i64, i64), String> {
        let st = self.st.borrow();
        let s = st
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .ok_or("no such session")?;
        let mut lines = Vec::new();
        let mut total = 0i64;
        // In the board's order, like the SQLite store.
        let mut items: Vec<&Item> = st
            .menu
            .iter()
            .filter(|i| s.cart.contains_key(&i.id))
            .collect();
        items.sort_by_key(|i| i.sort);
        for it in items {
            let qty = s.cart[&it.id];
            let l = Line {
                item_id: it.id.clone(),
                name: it.name.clone(),
                qty,
                unit_micro: it.price_micro,
            };
            total = total.saturating_add(it.price_micro.saturating_mul(qty as i64));
            lines.push(self.line_view(&l));
        }
        Ok((lines, total, s.balance_micro))
    }

    fn faucet(&self, session_id: &str) -> Result<i64, String> {
        let denom = self.denom();
        self.with_session(session_id, |s, _| {
            if s.balance_micro >= FAUCET_CAP {
                return Err(format!(
                    "you already have {}; the faucet stops there",
                    denom.price(s.balance_micro)
                ));
            }
            s.balance_micro = (s.balance_micro + FAUCET_GRANT).min(FAUCET_CAP);
            Ok(s.balance_micro)
        })
    }

    fn balance(&self, session_id: &str) -> Result<i64, String> {
        self.with_session(session_id, |s, _| Ok(s.balance_micro))
    }

    fn checkout(
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
                self.denom().price(total),
                self.denom().price(balance)
            ));
        }
        let order_id = self.id("order");
        let pay_id = self.id("pay");
        let hash = if tx_hash.trim().is_empty() {
            format!("demo-{}", &order_id[order_id.len() - 6..])
        } else {
            tx_hash.trim().to_lowercase()
        };
        let mut st = self.st.borrow_mut();
        // A chain transaction pays exactly one order.
        if st
            .payments
            .iter()
            .any(|p| p.tx_hash.eq_ignore_ascii_case(&hash))
        {
            return Err("that transaction has already paid for an order".into());
        }
        let order_no = st.orders.iter().map(|o| o.order_no).max().unwrap_or(0) + 1;
        let (now, day) = (st.now_ms, st.today.clone());
        let s = st
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .ok_or("no such session")?;
        let guest = s.name.clone();
        if debit {
            s.balance_micro -= total;
        }
        let order_lines: Vec<Line> = lines
            .iter()
            .map(|l| Line {
                item_id: l.item_id.clone(),
                name: l.name.clone(),
                qty: l.qty,
                unit_micro: causewaybay_panda_protocol::parse_usdc(&l.unit_usdc).unwrap_or(0),
            })
            .collect();
        s.cart.clear();
        st.orders.push(Order {
            id: order_id.clone(),
            order_no,
            session_id: session_id.into(),
            guest: guest.clone(),
            total_micro: total,
            status: OrderStatus::Placed,
            created_ms: now,
            day: day.clone(),
            lines: order_lines,
        });
        st.payments.push(Payment {
            id: pay_id,
            order_id: order_id.clone(),
            session_id: session_id.into(),
            guest,
            amount_micro: total,
            method: method.into(),
            tx_hash: hash.clone(),
            created_ms: now,
            day,
        });
        Ok((order_id, order_no, total, hash))
    }

    fn orders(&self) -> Result<Vec<OrderView>, String> {
        let st = self.st.borrow();
        let mut v: Vec<&Order> = st.orders.iter().collect();
        v.sort_by(|a, b| b.order_no.cmp(&a.order_no));
        Ok(v.into_iter().map(|o| self.order_view(o)).collect())
    }

    fn orders_today(&self) -> Result<Vec<OrderView>, String> {
        let st = self.st.borrow();
        let mut v: Vec<&Order> = st.orders.iter().filter(|o| o.day == st.today).collect();
        v.sort_by(|a, b| b.order_no.cmp(&a.order_no));
        Ok(v.into_iter().map(|o| self.order_view(o)).collect())
    }

    fn open_orders(&self) -> Result<Vec<OrderView>, String> {
        let st = self.st.borrow();
        let mut v: Vec<&Order> = st.orders.iter().filter(|o| o.status.is_open()).collect();
        v.sort_by_key(|o| o.order_no);
        Ok(v.into_iter().map(|o| self.order_view(o)).collect())
    }

    fn orders_for_session(&self, session_id: &str) -> Result<Vec<OrderView>, String> {
        let st = self.st.borrow();
        let mut v: Vec<&Order> = st
            .orders
            .iter()
            .filter(|o| o.session_id == session_id)
            .collect();
        v.sort_by(|a, b| b.order_no.cmp(&a.order_no));
        Ok(v.into_iter().map(|o| self.order_view(o)).collect())
    }

    fn order(&self, id: &str) -> Result<Option<OrderView>, String> {
        Ok(self
            .st
            .borrow()
            .orders
            .iter()
            .find(|o| o.id == id)
            .map(|o| self.order_view(o)))
    }

    fn order_session(&self, id: &str) -> Option<String> {
        self.st
            .borrow()
            .orders
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.session_id.clone())
    }

    fn set_order_status(&self, id: &str, to: OrderStatus) -> Result<OrderView, String> {
        {
            let mut st = self.st.borrow_mut();
            let o = st
                .orders
                .iter_mut()
                .find(|o| o.id == id)
                .ok_or("no such order")?;
            if o.status != to {
                let allowed = o.status.next() == Some(to)
                    || (to == OrderStatus::Cancelled && o.status.is_open());
                if !allowed {
                    return Err(format!(
                        "an order that is {} cannot become {}",
                        o.status.as_str(),
                        to.as_str()
                    ));
                }
                o.status = to;
            }
        }
        self.order(id)?.ok_or_else(|| "no such order".into())
    }

    fn takings_today(&self) -> Result<Takings, String> {
        let st = self.st.borrow();
        let mut t = Takings::default();
        for p in st.payments.iter().filter(|p| p.day == st.today) {
            t.total_micro += p.amount_micro;
            t.orders += 1;
            if p.method == "wallet" {
                t.wallet_micro += p.amount_micro;
            } else {
                t.coin_micro += p.amount_micro;
            }
        }
        Ok(t)
    }

    fn payments(&self) -> Result<Vec<PaymentView>, String> {
        let st = self.st.borrow();
        let mut v: Vec<&Payment> = st.payments.iter().collect();
        v.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
        Ok(v.into_iter()
            .map(|p| PaymentView {
                id: p.id.clone(),
                order_id: p.order_id.clone(),
                amount_display: self.denom().price(p.amount_micro),
                guest: p.guest.clone(),
                amount_usdc: format_usdc(p.amount_micro),
                method: p.method.clone(),
                tx_hash: p.tx_hash.clone(),
                status: "confirmed".into(),
                created_at: format!("{}", p.created_ms),
            })
            .collect())
    }

    fn setting(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.settings_impl(key))
    }

    fn set_setting(&self, key: &str, value: Option<&str>) -> Result<(), String> {
        let mut st = self.st.borrow_mut();
        match value {
            Some(v) => {
                st.settings.insert(key.into(), v.into());
            }
            None => {
                st.settings.remove(key);
            }
        }
        Ok(())
    }
}

impl MemStore {
    fn settings_impl(&self, key: &str) -> Option<String> {
        self.st.borrow().settings.get(key).cloned()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_shop_has_the_seed_board_in_hong_kong_dollars() {
        let s = MemStore::new("panda");
        let menu = s.menu().unwrap();
        assert_eq!(menu.len(), 11);
        assert_eq!(menu[0].id, "latte");
        assert_eq!(menu[0].price_display, "HK$38.00");
        assert!(s.check_pin("panda").unwrap());
        assert!(!s.check_pin("nope").unwrap());
    }

    #[test]
    fn a_snapshot_comes_back_whole() {
        let s = MemStore::new("panda");
        s.set_clock(1_000, "2026-09-05");
        let g = s.create_session(Role::Guest, "Mei").unwrap();
        s.add_to_cart(&g.id, "latte", 2).unwrap();
        let (_, no, total, _) = s.checkout(&g.id, "coin", "", true).unwrap();
        assert_eq!(no, 1);

        let back = MemStore::restore(&s.snapshot(), "panda");
        let again = back
            .session(&g.id)
            .unwrap()
            .expect("the guest is still here");
        assert_eq!(again.balance_micro, GUEST_GRANT - total);
        assert_eq!(back.open_orders().unwrap().len(), 1);
        assert_eq!(back.takings_today().unwrap().orders, 1);
        assert_eq!(back.takings_today().unwrap().total_micro, total);
    }

    #[test]
    fn garbage_restores_to_a_fresh_shop_rather_than_a_broken_one() {
        let s = MemStore::restore("not json", "panda");
        assert_eq!(s.menu().unwrap().len(), 11);
        let s = MemStore::restore("{}", "panda");
        assert_eq!(s.menu().unwrap().len(), 11);
    }

    #[test]
    fn today_is_the_hosts_day_not_the_stores() {
        let s = MemStore::new("panda");
        s.set_clock(1, "2026-09-04");
        let g = s.create_session(Role::Guest, "Mei").unwrap();
        s.add_to_cart(&g.id, "egg_tart", 1).unwrap();
        s.checkout(&g.id, "coin", "", true).unwrap();
        assert_eq!(s.takings_today().unwrap().orders, 1);
        // Midnight passes on the host.
        s.set_clock(2, "2026-09-05");
        assert_eq!(s.takings_today().unwrap().orders, 0);
    }

    #[test]
    fn settings_survive_a_snapshot() {
        let s = MemStore::new("panda");
        s.set_setting("ai.provider", Some("grok")).unwrap();
        s.set_setting("ai.key", Some("xai-secret")).unwrap();
        let back = MemStore::restore(&s.snapshot(), "panda");
        assert_eq!(
            back.setting("ai.provider").unwrap().as_deref(),
            Some("grok")
        );
        back.set_setting("ai.key", None).unwrap();
        assert_eq!(back.setting("ai.key").unwrap(), None);
        assert_eq!(back.setting("nothing").unwrap(), None);
    }

    #[test]
    fn one_hash_pays_one_order_here_too() {
        let s = MemStore::new("panda");
        let g = s.create_session(Role::Guest, "Mei").unwrap();
        let h = format!("0x{}", "ab".repeat(32));
        s.add_to_cart(&g.id, "latte", 1).unwrap();
        s.checkout(&g.id, "wallet", &h, false).unwrap();
        s.add_to_cart(&g.id, "latte", 1).unwrap();
        let again = s.checkout(
            &g.id,
            "wallet",
            &h.to_uppercase().replace("0X", "0x"),
            false,
        );
        assert!(again.unwrap_err().contains("already paid"));
    }
}
