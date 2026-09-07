//! What the cafe needs from wherever it keeps things. SQLite on a shop's Mac,
//! a struct in a browser tab: the till does not care.

use causewaybay_panda_protocol::denom::Denom;
use causewaybay_panda_protocol::wire::{
    CartLine, MenuDraft, MenuItem, OrderStatus, OrderView, PaymentView, Role,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub role: Role,
    pub name: String,
    pub balance_micro: i64,
}

/// One day's takings, in the settlement unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Takings {
    pub total_micro: i64,
    pub orders: i64,
    pub coin_micro: i64,
    pub wallet_micro: i64,
}

/// Every method returns `Err(String)` for a reason a person can read; the
/// caller turns that straight into a chat line.
pub trait Store {
    fn denom(&self) -> Denom;
    fn check_pin(&self, pin: &str) -> Result<bool, String>;
    fn set_pin(&self, pin: &str) -> Result<(), String>;
    fn treasury(&self) -> Result<String, String>;

    fn create_session(&self, role: Role, name: &str) -> Result<SessionRow, String>;
    fn session(&self, id: &str) -> Result<Option<SessionRow>, String>;

    fn menu(&self) -> Result<Vec<MenuItem>, String>;
    fn menu_available(&self) -> Result<Vec<MenuItem>, String>;
    fn item(&self, id: &str) -> Result<Option<MenuItem>, String>;
    fn find_item_named(&self, hay: &str) -> Result<Option<MenuItem>, String>;
    fn upsert_item(&self, draft: &MenuDraft) -> Result<MenuItem, String>;
    fn hide_item(&self, id: &str) -> Result<bool, String>;
    fn show_item(&self, id: &str) -> Result<bool, String>;

    fn add_to_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String>;
    fn remove_from_cart(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String>;
    fn set_cart_qty(&self, session_id: &str, item_id: &str, qty: u32) -> Result<(), String>;
    fn clear_cart(&self, session_id: &str) -> Result<(), String>;
    /// Lines, total micro, balance micro.
    fn cart(&self, session_id: &str) -> Result<(Vec<CartLine>, i64, i64), String>;

    fn faucet(&self, session_id: &str) -> Result<i64, String>;
    fn balance(&self, session_id: &str) -> Result<i64, String>;
    /// Order id, order number, total micro, tx hash.
    fn checkout(
        &self,
        session_id: &str,
        method: &str,
        tx_hash: &str,
        debit: bool,
    ) -> Result<(String, i64, i64, String), String>;

    fn orders(&self) -> Result<Vec<OrderView>, String>;
    /// Every order placed today, by the shop's clock — the same "today" as
    /// the takings. Cancelled ones included; the dashboard counts them.
    fn orders_today(&self) -> Result<Vec<OrderView>, String>;
    fn open_orders(&self) -> Result<Vec<OrderView>, String>;
    fn orders_for_session(&self, session_id: &str) -> Result<Vec<OrderView>, String>;
    fn order(&self, id: &str) -> Result<Option<OrderView>, String>;
    fn order_session(&self, id: &str) -> Option<String>;
    fn set_order_status(&self, id: &str, to: OrderStatus) -> Result<OrderView, String>;

    fn takings_today(&self) -> Result<Takings, String>;
    fn payments(&self) -> Result<Vec<PaymentView>, String>;

    /// Small named values the shop keeps between runs: which model listens
    /// to the chat, and the like. `None` on write removes the key.
    fn setting(&self, key: &str) -> Result<Option<String>, String>;
    fn set_setting(&self, key: &str, value: Option<&str>) -> Result<(), String>;

    /// Back to a fresh install: sessions, carts, orders, payments and
    /// settings gone; the seed menu and the pin the shop started with.
    fn reset(&self) -> Result<(), String>;
}
