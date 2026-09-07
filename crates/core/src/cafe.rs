//! Apply a parsed intent to a logged-in session. Chat and buttons share this.

use causewaybay_panda_protocol::chain::chain_id_hex;
use causewaybay_panda_protocol::intent::{parse_all_adds, parse_intent, split_qty, Intent};
use causewaybay_panda_protocol::money::{format_usdc, parse_usdc};
use causewaybay_panda_protocol::wire::{
    BigButton, DishStat, OrderStatus, OrderView, PayMethod, ServerMsg,
};
use causewaybay_panda_protocol::{Role, COIN_NAME};

use crate::settlement::is_tx_hash;
use crate::shop::Shop;
use crate::store::{SessionRow, Store};

#[derive(Debug)]
pub struct Apply {
    pub to_self: Vec<ServerMsg>,
    pub to_owners: Vec<ServerMsg>,
    pub to_guests: Vec<ServerMsg>,
    /// For one named session. An order goes to the guest who placed it, not
    /// to every table in the room.
    pub to_session: Vec<(String, ServerMsg)>,
}

impl Apply {
    fn one(msg: ServerMsg) -> Self {
        Self {
            to_self: vec![msg],
            to_owners: Vec::new(),
            to_guests: Vec::new(),
            to_session: Vec::new(),
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self::one(ServerMsg::Error {
            message: message.into(),
        })
    }
}

pub fn intents_for_chat(db: &dyn Store, text: &str) -> Vec<Intent> {
    // Read the line as a whole first. "remove latte" and "hide macaroni" both
    // name a dish, so fanning out to adds before commanding would turn either
    // one into an order.
    match parse_intent(text) {
        Intent::Add { item_id, qty } => {
            let all = parse_all_adds(text);
            if all.len() > 1 {
                all
            } else {
                vec![Intent::Add { item_id, qty }]
            }
        }
        Intent::Unknown(raw) => {
            // A dish the owner added after boot is not in the seed catalog.
            let (qty, rest) = split_qty(&raw);
            if let Ok(Some(item)) = db.find_item_named(rest) {
                vec![Intent::Add {
                    item_id: item.id,
                    qty,
                }]
            } else {
                vec![Intent::Unknown(raw)]
            }
        }
        other => vec![other],
    }
}

pub fn apply(db: &dyn Store, shop: &Shop, session: &SessionRow, intent: Intent) -> Apply {
    match intent {
        Intent::Help => Apply::one(help(session.role, shop)),
        Intent::ShowMenu => menu_msg(db, shop, session.role),
        Intent::ShowCart => cart_msg(db, shop, session),
        Intent::Add { item_id, qty } => {
            if session.role != Role::Guest {
                return Apply::err("owners write the menu; guests order from it");
            }
            match db.add_to_cart(&session.id, &item_id, qty) {
                Ok(()) => {
                    let name = db
                        .item(&item_id)
                        .ok()
                        .flatten()
                        .map(|i| i.name)
                        .unwrap_or(item_id);
                    let mut out = cart_msg(db, shop, session);
                    out.to_self.insert(
                        0,
                        ServerMsg::Assistant {
                            text: format!("Added {qty}× {name}. Pay when you are ready."),
                            buttons: vec![BigButton::cart(), BigButton::pay(), BigButton::menu()],
                        },
                    );
                    out
                }
                Err(e) => Apply::err(e),
            }
        }
        Intent::Remove { item_id, qty } => {
            if let Err(e) = db.remove_from_cart(&session.id, &item_id, qty) {
                return Apply::err(e);
            }
            cart_msg(db, shop, session)
        }
        Intent::Clear => {
            if let Err(e) = db.clear_cart(&session.id) {
                return Apply::err(e);
            }
            cart_msg(db, shop, session)
        }
        Intent::SetQty { item_id, qty } => {
            if session.role != Role::Guest {
                return Apply::err("owners write the menu; guests order from it");
            }
            if let Err(e) = db.set_cart_qty(&session.id, &item_id, qty) {
                return Apply::err(e);
            }
            cart_msg(db, shop, session)
        }
        Intent::Faucet => {
            if session.role != Role::Guest {
                return Apply::err("the faucet is for guests");
            }
            if !shop.is_simulation() {
                return Apply::err(format!(
                    "this shop takes real USDC — there is no {COIN_NAME} to hand out"
                ));
            }
            match db.faucet(&session.id) {
                Ok(balance) => {
                    let mut out = cart_msg(db, shop, session);
                    out.to_self.insert(
                        0,
                        ServerMsg::Assistant {
                            text: format!(
                                "Topped up. You have {} in {COIN_NAME}.",
                                shop.price(balance)
                            ),
                            buttons: vec![BigButton::menu(), BigButton::cart()],
                        },
                    );
                    out
                }
                Err(e) => Apply::err(e),
            }
        }
        Intent::OrderAdvance { order_id, status } => {
            if session.role != Role::Owner {
                return Apply::err("only the counter moves an order along");
            }
            let to = match OrderStatus::parse(&status) {
                Some(s) => s,
                // No status named means "the next thing", which is what the
                // one big button on each ticket does.
                None => match db.order(&order_id) {
                    Ok(Some(o)) => match o.status.next() {
                        Some(n) => n,
                        None => return Apply::err("that order is already finished"),
                    },
                    Ok(None) => return Apply::err("no such order"),
                    Err(e) => return Apply::err(e),
                },
            };
            order_moved(db, shop, &order_id, to)
        }
        Intent::OrderCancel { order_id } => {
            if session.role != Role::Owner {
                return Apply::err("only the counter can cancel an order");
            }
            order_moved(db, shop, &order_id, OrderStatus::Cancelled)
        }
        Intent::Pay { method, tx_hash } => pay(db, shop, session, method, &tx_hash),
        Intent::Dashboard => {
            // The card refreshes, and the chat says the headline too: a
            // person who typed "today" is looking at the dock, not the card.
            let card = match session.role {
                Role::Owner => dashboard_msg(db, shop),
                Role::Guest => guest_dashboard_msg(db, shop, &session.id),
            };
            let said = facts_for(db, shop, session)
                .into_iter()
                .take(if session.role == Role::Owner { 2 } else { 1 })
                .collect::<Vec<_>>()
                .join(" ");
            Apply {
                to_self: vec![
                    card,
                    ServerMsg::Assistant {
                        text: said,
                        buttons: Vec::new(),
                    },
                ],
                to_owners: Vec::new(),
                to_guests: Vec::new(),
                to_session: Vec::new(),
            }
        }
        Intent::ListPayments => {
            if session.role != Role::Owner {
                return Apply::err("only the owner can see payments");
            }
            match db.payments() {
                Ok(payments) => Apply::one(ServerMsg::Payments { payments }),
                Err(e) => Apply::err(e),
            }
        }
        Intent::ListOrders => {
            if session.role != Role::Owner {
                return Apply::err("only the owner can see orders");
            }
            match db.orders() {
                Ok(orders) => Apply::one(ServerMsg::Orders { orders }),
                Err(e) => Apply::err(e),
            }
        }
        Intent::MenuUpsert { draft } => {
            if session.role != Role::Owner {
                return Apply::err("only the owner can change the menu");
            }
            match db.upsert_item(&draft) {
                Ok(item) => menu_broadcast(
                    db,
                    format!("{} is on the menu at {} USDC.", item.name, item.price_usdc),
                ),
                Err(e) => Apply::err(e),
            }
        }
        Intent::MenuHide { item_id } => {
            if session.role != Role::Owner {
                return Apply::err("only the owner can hide a dish");
            }
            match db.hide_item(&item_id) {
                Ok(true) => {
                    let name = db
                        .item(&item_id)
                        .ok()
                        .flatten()
                        .map(|i| i.name)
                        .unwrap_or_else(|| item_id.clone());
                    menu_broadcast(db, format!("{name} is off the board."))
                }
                Ok(false) => Apply::err("no such item"),
                Err(e) => Apply::err(e),
            }
        }
        Intent::MenuShow { item_id } => {
            if session.role != Role::Owner {
                return Apply::err("only the owner can change the menu");
            }
            match db.show_item(&item_id) {
                Ok(true) => {
                    let name = db
                        .item(&item_id)
                        .ok()
                        .flatten()
                        .map(|i| i.name)
                        .unwrap_or_else(|| item_id.clone());
                    menu_broadcast(db, format!("{name} is back on the board."))
                }
                Ok(false) => Apply::err("no such item"),
                Err(e) => Apply::err(e),
            }
        }
        Intent::Auto { .. } => {
            if session.role != Role::Owner {
                return Apply::err("only the owner can set the cafe running on its own");
            }
            Apply::err("the auto switch is thrown by the host, not the till")
        }
        Intent::Kitchen { .. } => {
            if session.role != Role::Owner {
                return Apply::err("only the owner decides who works the kitchen");
            }
            Apply::err("the kitchen switch is thrown by the host, not the till")
        }
        // The model answered in words. Any dish it named that is on the board
        // becomes a button, so the answer is also a way to order.
        Intent::Say { text, suggest } => {
            let buttons = if session.role == Role::Guest {
                suggest
                    .iter()
                    .filter_map(|id| db.item(id).ok().flatten())
                    .filter(|i| i.available)
                    .map(|i| BigButton::add(&i.id, &i.name))
                    .collect()
            } else {
                Vec::new()
            };
            Apply::one(ServerMsg::Assistant { text, buttons })
        }
        Intent::Unknown(raw) => Apply::one(ServerMsg::Assistant {
            text: format!("I did not catch “{raw}”. Try a dish name, or tap a button."),
            buttons: vec![BigButton::menu(), BigButton::help()],
        }),
    }
}

/// A menu change everybody must see: the room gets the new board, the owner
/// gets a line of confirmation with it.
fn menu_broadcast(db: &dyn Store, said: String) -> Apply {
    let menu = db.menu().unwrap_or_default();
    Apply {
        to_self: vec![
            ServerMsg::Assistant {
                text: said,
                buttons: vec![BigButton::menu()],
            },
            ServerMsg::Menu {
                items: menu.clone(),
            },
        ],
        to_owners: vec![ServerMsg::Menu {
            items: menu.clone(),
        }],
        to_guests: vec![ServerMsg::Menu { items: menu }],
        to_session: Vec::new(),
    }
}

fn help(role: Role, shop: &Shop) -> ServerMsg {
    match role {
        Role::Guest => {
            let mut buttons = vec![BigButton::menu(), BigButton::cart(), BigButton::pay()];
            let money = if shop.is_simulation() {
                buttons.push(BigButton::faucet());
                format!("You are paying in {COIN_NAME}, which is test money — tap Top up for more.")
            } else {
                format!(
                    "This shop takes USDC on {}, from your own wallet.",
                    shop.settle.chain.name
                )
            };
            ServerMsg::Assistant {
                text: format!("Tap a dish, or say it — “latte”, “two pineapple buns”. {money}"),
                buttons,
            }
        }
        Role::Owner => ServerMsg::Assistant {
            text: "Tap a ticket to move it along. Say “add item mango pudding 38 dessert”, \
                   “hide macaroni”, “show macaroni”, “payments”, “orders”, “today”, or \
                   “kitchen on” to let the panda work the tickets. Tap a dish to take it \
                   off the board or put it back."
                .into(),
            buttons: vec![
                BigButton {
                    label: "Today".into(),
                    action: causewaybay_panda_protocol::ActionName::Dashboard,
                    item_id: String::new(),
                    qty: 0,
                },
                BigButton {
                    label: "Payments".into(),
                    action: causewaybay_panda_protocol::ActionName::ListPayments,
                    item_id: String::new(),
                    qty: 0,
                },
                BigButton {
                    label: "Orders".into(),
                    action: causewaybay_panda_protocol::ActionName::ListOrders,
                    item_id: String::new(),
                    qty: 0,
                },
                BigButton::menu(),
            ],
        },
    }
}

fn menu_msg(db: &dyn Store, shop: &Shop, role: Role) -> Apply {
    let items = match role {
        Role::Guest => db.menu_available().unwrap_or_default(),
        Role::Owner => db.menu().unwrap_or_default(),
    };
    let mut buttons: Vec<BigButton> = items
        .iter()
        .filter(|i| i.available)
        .take(6)
        .map(|i| BigButton::add(&i.id, &format!("{} · {}", i.name, i.price_display)))
        .collect();
    if role == Role::Guest {
        buttons.push(BigButton::pay());
    }
    let mut out = Apply::one(ServerMsg::Menu { items });
    out.to_self.push(ServerMsg::Assistant {
        text: format!(
            "Today at Causewaybay Coffee, priced in {}.",
            shop.denom.code
        ),
        buttons,
    });
    out
}

fn cart_msg(db: &dyn Store, shop: &Shop, session: &SessionRow) -> Apply {
    match db.cart(&session.id) {
        Ok((lines, total, balance)) => Apply::one(ServerMsg::Cart {
            lines,
            total_usdc: format_usdc(total),
            total_display: shop.price(total),
            balance_usdc: format_usdc(balance),
            balance_display: shop.price(balance),
            can_faucet: shop.can_faucet(balance),
        }),
        Err(e) => Apply::err(e),
    }
}

/// Open the door. A guest's session the browser still holds is picked up
/// again: they keep their name, purse and the order they are waiting on.
///
/// The owner's pin is a live shop's lock on the till. A simulation has no
/// till worth locking — it is Causewaybay Coin — so any pin, or none, opens
/// the counter, and a demo is never stuck at the door. In a live shop the
/// pin is asked for every time; a remembered id is never a key.
pub fn login(
    db: &dyn Store,
    shop: &Shop,
    role: Role,
    name: &str,
    pin: &str,
    held: &str,
) -> Result<SessionRow, String> {
    let held = held.trim();
    let resumed = if held.is_empty() {
        None
    } else {
        db.session(held)
            .ok()
            .flatten()
            .filter(|row| row.role == role)
    };
    match role {
        Role::Guest => match resumed {
            Some(row) => Ok(row),
            None => db.create_session(Role::Guest, name),
        },
        Role::Owner => {
            if !shop.is_simulation() && !db.check_pin(pin)? {
                return Err("wrong pin".into());
            }
            match resumed {
                Some(row) => Ok(row),
                None => db.create_session(Role::Owner, name),
            }
        }
    }
}

/// What a person is handed the moment they are in: the welcome, the board,
/// a word of help, and — for a guest — their purse and cart, for the owner
/// today's takings. `ai` names the model listening, or is empty.
pub fn on_login(
    db: &dyn Store,
    shop: &Shop,
    row: &SessionRow,
    ai: &str,
    demo_on: bool,
    kitchen_on: bool,
) -> Vec<ServerMsg> {
    let (cafe, cafe_zh) = crate::setup::cafe_name(db);
    let mut out = vec![ServerMsg::Welcome {
        role: row.role,
        name: row.name.clone(),
        session_id: row.id.clone(),
        cafe,
        cafe_zh,
        treasury: shop.settle.treasury_address().to_string(),
        chain_id: shop.settle.chain.chain_id,
        balance_usdc: format_usdc(row.balance_micro),
        balance_display: shop.price(row.balance_micro),
        ai: ai.to_string(),
        settlement: shop.wire(),
        orders: match row.role {
            Role::Guest => db.orders_for_session(&row.id).unwrap_or_default(),
            Role::Owner => db.open_orders().unwrap_or_default(),
        },
    }];
    out.extend(apply(db, shop, row, Intent::ShowMenu).to_self);
    out.extend(apply(db, shop, row, Intent::Help).to_self);
    match row.role {
        Role::Owner => {
            out.push(takings_msg(db, shop));
            out.push(dashboard_msg(db, shop));
            out.push(ServerMsg::Auto { on: demo_on });
            out.push(ServerMsg::Kitchen { on: kitchen_on });
        }
        // The faucet is only offered once the page knows what they hold.
        Role::Guest => {
            out.extend(apply(db, shop, row, Intent::ShowCart).to_self);
            out.push(guest_dashboard_msg(db, shop, &row.id));
        }
    }
    out
}

/// The strip at the top of the counter: today so far.
pub fn takings_msg(db: &dyn Store, shop: &Shop) -> ServerMsg {
    let t = db.takings_today().unwrap_or_default();
    ServerMsg::Takings {
        total_display: shop.price(t.total_micro),
        total_usdc: format_usdc(t.total_micro),
        orders: t.orders,
        coin_display: shop.price(t.coin_micro),
        wallet_display: shop.price(t.wallet_micro),
    }
}

/// The owner's day on one card: takings, who came, what sold, and where the
/// kitchen stands. Built from the same rows the books show, so it never
/// disagrees with them.
pub fn dashboard_msg(db: &dyn Store, shop: &Shop) -> ServerMsg {
    let t = db.takings_today().unwrap_or_default();
    let today = db.orders_today().unwrap_or_default();
    let open_now = db.open_orders().unwrap_or_default();
    let count = |s: OrderStatus| today.iter().filter(|o| o.status == s).count() as i64;
    let mut guests: Vec<&str> = today
        .iter()
        .filter(|o| o.status != OrderStatus::Cancelled)
        .map(|o| o.guest.as_str())
        .collect();
    guests.sort_unstable();
    guests.dedup();
    let average = if t.orders > 0 {
        t.total_micro / t.orders
    } else {
        0
    };
    let mut top = dish_stats(
        shop,
        today.iter().filter(|o| o.status != OrderStatus::Cancelled),
    );
    top.truncate(5);
    ServerMsg::Dashboard {
        total_display: shop.price(t.total_micro),
        total_usdc: format_usdc(t.total_micro),
        orders: t.orders,
        average_display: shop.price(average),
        guests: guests.len() as i64,
        open: open_now.len() as i64,
        placed: open_now
            .iter()
            .filter(|o| o.status == OrderStatus::Placed)
            .count() as i64,
        preparing: open_now
            .iter()
            .filter(|o| o.status == OrderStatus::Preparing)
            .count() as i64,
        ready: open_now
            .iter()
            .filter(|o| o.status == OrderStatus::Ready)
            .count() as i64,
        collected: count(OrderStatus::Collected),
        cancelled: count(OrderStatus::Cancelled),
        top,
    }
}

/// One guest's standing: every order they have placed at this table, what it
/// came to, and the dish they keep coming back for.
pub fn guest_dashboard_msg(db: &dyn Store, shop: &Shop, session_id: &str) -> ServerMsg {
    let mine = db.orders_for_session(session_id).unwrap_or_default();
    let kept: Vec<&OrderView> = mine
        .iter()
        .filter(|o| o.status != OrderStatus::Cancelled)
        .collect();
    let spent: i64 = kept
        .iter()
        .map(|o| parse_usdc(&o.total_usdc).unwrap_or(0))
        .sum();
    let favourite = dish_stats(shop, kept.iter().copied()).into_iter().next();
    // orders_for_session is newest first.
    let last_status = mine
        .first()
        .map(|o| o.status.as_str().to_string())
        .unwrap_or_default();
    ServerMsg::GuestDashboard {
        orders: kept.len() as i64,
        spent_display: shop.price(spent),
        spent_usdc: format_usdc(spent),
        favourite: favourite
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default(),
        favourite_qty: favourite.map(|d| d.qty).unwrap_or(0),
        open: mine.iter().filter(|o| o.status.is_open()).count() as i64,
        last_status,
    }
}

/// What the till knows right now, in plain lines for a model to answer
/// from: the day's figures for the owner, their own standing and cart for a
/// guest. Nothing here that the same person could not read off the page.
pub fn facts_for(db: &dyn Store, shop: &Shop, session: &SessionRow) -> Vec<String> {
    match session.role {
        Role::Owner => match dashboard_msg(db, shop) {
            ServerMsg::Dashboard {
                total_display,
                orders,
                average_display,
                guests,
                open,
                placed,
                preparing,
                ready,
                collected,
                cancelled,
                top,
                ..
            } => {
                let selling = if top.is_empty() {
                    "nothing yet".to_string()
                } else {
                    top.iter()
                        .map(|d| format!("{} ×{} ({})", d.name, d.qty, d.revenue_display))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let n = |n: i64, one: &str, many: &str| {
                    if n == 1 {
                        format!("{n} {one}")
                    } else {
                        format!("{n} {many}")
                    }
                };
                vec![
                    format!(
                        "Takings today: {total_display} from {}, average {average_display}, {} served.",
                        n(orders, "order", "orders"),
                        n(guests, "guest", "guests")
                    ),
                    format!(
                        "Kitchen now: {open} open — {placed} waiting, {preparing} being made, {ready} ready. Collected today: {collected}. Cancelled today: {cancelled}."
                    ),
                    format!("Selling today, best first: {selling}."),
                ]
            }
            _ => Vec::new(),
        },
        Role::Guest => {
            let mut facts = Vec::new();
            if let ServerMsg::GuestDashboard {
                orders,
                spent_display,
                favourite,
                favourite_qty,
                open,
                last_status,
                ..
            } = guest_dashboard_msg(db, shop, &session.id)
            {
                let usual = if favourite.is_empty() {
                    "no usual yet".to_string()
                } else {
                    format!("usual dish {favourite} (×{favourite_qty})")
                };
                facts.push(format!(
                    "This guest, {}: {orders} orders here, spent {spent_display}, {usual}.",
                    session.name
                ));
                if open > 0 {
                    facts.push(format!(
                        "Their open orders: {open}; the latest is {last_status}."
                    ));
                } else {
                    facts.push("They have no order in the kitchen right now.".into());
                }
            }
            if let Ok((lines, total, balance)) = db.cart(&session.id) {
                let cart = if lines.is_empty() {
                    "empty".to_string()
                } else {
                    lines
                        .iter()
                        .map(|l| format!("{}× {}", l.qty, l.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                facts.push(format!(
                    "Their cart now: {cart}, total {}. Balance {}.",
                    shop.price(total),
                    shop.price(balance)
                ));
            }
            facts
        }
    }
}

/// Dishes across a set of orders, most sold first; ties go to the bigger
/// take, then the name, so the order is the same on every run.
fn dish_stats<'a>(shop: &Shop, orders: impl Iterator<Item = &'a OrderView>) -> Vec<DishStat> {
    let mut by_id: Vec<(String, String, i64, i64)> = Vec::new();
    for o in orders {
        for l in &o.lines {
            let micro = parse_usdc(&l.line_usdc).unwrap_or(0);
            match by_id.iter_mut().find(|(id, ..)| *id == l.item_id) {
                Some(row) => {
                    row.2 += l.qty as i64;
                    row.3 += micro;
                }
                None => by_id.push((l.item_id.clone(), l.name.clone(), l.qty as i64, micro)),
            }
        }
    }
    by_id.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)).then(a.1.cmp(&b.1)));
    by_id
        .into_iter()
        .map(|(item_id, name, qty, micro)| DishStat {
            item_id,
            name,
            qty,
            revenue_display: shop.price(micro),
            revenue_usdc: format_usdc(micro),
        })
        .collect()
}

/// One order changed. The kitchen queue and the guest's own card both come
/// from this, so the counter and the table never disagree.
fn order_moved(db: &dyn Store, shop: &Shop, order_id: &str, to: OrderStatus) -> Apply {
    let owner_session = db.order_session(order_id).unwrap_or_default();
    match db.set_order_status(order_id, to) {
        Ok(order) => {
            let said = format!("Order #{} is {}.", order.order_no, order.status.as_str());
            Apply {
                to_self: vec![
                    ServerMsg::Assistant {
                        text: said,
                        buttons: Vec::new(),
                    },
                    ServerMsg::OrderUpdate {
                        order: order.clone(),
                    },
                ],
                to_owners: vec![
                    ServerMsg::OrderUpdate {
                        order: order.clone(),
                    },
                    dashboard_msg(db, shop),
                ],
                to_guests: Vec::new(),
                // Only the table that placed it hears about it.
                to_session: vec![
                    (owner_session.clone(), ServerMsg::OrderUpdate { order }),
                    (
                        owner_session.clone(),
                        guest_dashboard_msg(db, shop, &owner_session),
                    ),
                ],
            }
        }
        Err(e) => Apply::err(e),
    }
}

/// Two settlements share one till. In simulation the guest spends Causewaybay
/// Coin and it is done in one step. Live moves real USDC on Cronos: the first
/// call only hands back a transaction to sign, and nothing is debited or
/// recorded until the browser returns its hash.
fn pay(
    db: &dyn Store,
    shop: &Shop,
    session: &SessionRow,
    method: PayMethod,
    tx_hash: &str,
) -> Apply {
    if session.role != Role::Guest {
        return Apply::err("the owner does not pay the till");
    }
    // The plain Pay button names no method and takes whatever the shop takes.
    // Asking for a wallet specifically is a real request, and a shop that
    // cannot honour it says why rather than quietly spending test money.
    let method = match method {
        PayMethod::Wallet if !shop.onchain() => {
            return Apply::err(format!("wallet payment is off: {}", shop.onchain_reason()))
        }
        PayMethod::Wallet => PayMethod::Wallet,
        PayMethod::Coin if shop.onchain() => PayMethod::Wallet,
        PayMethod::Coin => PayMethod::Coin,
    };
    if method == PayMethod::Wallet {
        if tx_hash.trim().is_empty() {
            return prepare_wallet_payment(db, shop, session);
        }
        if !is_tx_hash(tx_hash) {
            return Apply::err("that is not a transaction hash");
        }
    }
    let (method_s, debit) = match method {
        PayMethod::Coin => ("coin", true),
        PayMethod::Wallet => ("wallet", false),
    };
    match db.checkout(&session.id, method_s, tx_hash, debit) {
        Ok((order_id, order_no, total, hash)) => {
            let payments = db.payments().unwrap_or_default();
            let (lines, _, balance) = db.cart(&session.id).unwrap_or_default();
            let order = db.order(&order_id).ok().flatten();
            let said = match method {
                PayMethod::Wallet => format!(
                    "Paid {} in USDC on {}. You are number {order_no}.",
                    shop.price(total),
                    shop.settle.chain.name
                ),
                PayMethod::Coin => format!(
                    "Paid {} in {COIN_NAME}. You are number {order_no}.",
                    shop.price(total)
                ),
            };
            let mut to_self = vec![
                ServerMsg::Paid {
                    order_id,
                    order_no,
                    amount_usdc: format_usdc(total),
                    amount_display: shop.price(total),
                    method,
                    explorer_url: shop.settle.explorer_url(&hash),
                    tx_hash: hash,
                    call_data: shop.settle.call_data(total),
                },
                ServerMsg::Cart {
                    lines,
                    total_usdc: format_usdc(0),
                    total_display: shop.price(0),
                    balance_usdc: format_usdc(balance),
                    balance_display: shop.price(balance),
                    can_faucet: shop.can_faucet(balance),
                },
                ServerMsg::Assistant {
                    text: said,
                    buttons: vec![BigButton::menu()],
                },
            ];
            let mut to_owners = vec![
                ServerMsg::Payments { payments },
                takings_msg(db, shop),
                dashboard_msg(db, shop),
            ];
            if let Some(order) = order {
                to_self.push(ServerMsg::OrderUpdate {
                    order: order.clone(),
                });
                // A new ticket lands on the counter without anyone refreshing.
                to_owners.push(ServerMsg::OrderUpdate { order });
            }
            to_self.push(guest_dashboard_msg(db, shop, &session.id));
            Apply {
                to_self,
                to_owners,
                to_guests: Vec::new(),
                to_session: Vec::new(),
            }
        }
        Err(e) => Apply::err(e),
    }
}

/// Price the cart and hand back the transfer for the wallet to sign. The cart
/// is left exactly as it was: an unsigned or failed transaction changes nothing.
fn prepare_wallet_payment(db: &dyn Store, shop: &Shop, session: &SessionRow) -> Apply {
    let (lines, total, _) = match db.cart(&session.id) {
        Ok(c) => c,
        Err(e) => return Apply::err(e),
    };
    if lines.is_empty() {
        return Apply::err("cart is empty");
    }
    Apply::one(ServerMsg::PayRequest {
        amount_usdc: format_usdc(total),
        amount_display: shop.price(total),
        amount_micro: shop.settle.atomic(total).to_string(),
        token: shop.settle.token_address().to_string(),
        treasury: shop.settle.treasury_address().to_string(),
        chain_id: shop.settle.chain.chain_id,
        chain_id_hex: chain_id_hex(shop.settle.chain.chain_id),
        call_data: shop.settle.call_data(total),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemStore;
    use causewaybay_panda_protocol::wire::MenuDraft;
    use causewaybay_panda_protocol::GUEST_GRANT;

    /// A simulation shop, the way every existing flow settles.
    fn demo() -> Shop {
        Shop::simulation()
    }

    fn guest_db() -> (MemStore, SessionRow) {
        let db = MemStore::new("panda");
        let s = db.create_session(Role::Guest, "Mei").unwrap();
        (db, s)
    }

    fn first_error(a: &Apply) -> Option<&str> {
        a.to_self.iter().find_map(|m| match m {
            ServerMsg::Error { message } => Some(message.as_str()),
            _ => None,
        })
    }

    fn cart_total(a: &Apply) -> Option<&str> {
        a.to_self.iter().find_map(|m| match m {
            ServerMsg::Cart { total_usdc, .. } => Some(total_usdc.as_str()),
            _ => None,
        })
    }

    /// What the guest actually reads, which is the figure that matters.
    fn cart_shown(a: &Apply) -> Option<&str> {
        a.to_self.iter().find_map(|m| match m {
            ServerMsg::Cart { total_display, .. } => Some(total_display.as_str()),
            _ => None,
        })
    }

    #[test]
    fn guest_order_pay_owner_sees_it() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();

        let adds = intents_for_chat(&db, "two lattes and an egg tart");
        assert_eq!(adds.len(), 2);
        for intent in adds {
            let r = apply(&db, &demo(), &guest, intent);
            assert!(first_error(&r).is_none(), "{r:?}", r = first_error(&r));
        }

        let paid = apply(
            &db,
            &demo(),
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        assert!(
            paid.to_self
                .iter()
                .any(|m| matches!(m, ServerMsg::Paid { .. })),
            "{paid:?}"
        );
        assert!(paid
            .to_owners
            .iter()
            .any(|m| matches!(m, ServerMsg::Payments { .. })));

        let after = db.session(&guest.id).unwrap().unwrap();
        assert!(after.balance_micro < GUEST_GRANT);

        let seen = apply(&db, &demo(), &owner, Intent::ListPayments);
        match &seen.to_self[0] {
            ServerMsg::Payments { payments } => {
                assert_eq!(payments.len(), 1);
                assert_eq!(payments[0].guest, "Mei");
                assert_eq!(payments[0].method, "coin", "simulation spends the coin");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_cart_cannot_pay() {
        let (db, guest) = guest_db();
        let r = apply(
            &db,
            &demo(),
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        assert!(first_error(&r).unwrap().contains("empty"));
    }

    #[test]
    fn guest_cannot_edit_menu_or_see_payments() {
        let (db, guest) = guest_db();
        let r = apply(
            &db,
            &demo(),
            &guest,
            Intent::MenuUpsert {
                draft: MenuDraft {
                    id: "x".into(),
                    name: "x".into(),
                    name_zh: String::new(),
                    description: String::new(),
                    price: "1".into(),
                    category: String::new(),
                    image: String::new(),
                },
            },
        );
        assert!(first_error(&r).unwrap().contains("owner"));
        let r = apply(&db, &demo(), &guest, Intent::ListPayments);
        assert!(first_error(&r).unwrap().contains("owner"));
    }

    #[test]
    fn owner_adds_and_hides() {
        let db = MemStore::new("panda");
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        let guest = db.create_session(Role::Guest, "Mei").unwrap();

        let r = apply(
            &db,
            &demo(),
            &owner,
            parse_intent("add item mango pudding 3.20 dessert"),
        );
        assert!(first_error(&r).is_none());
        assert!(r
            .to_self
            .iter()
            .any(|m| matches!(m, ServerMsg::Menu { .. })));
        assert!(r
            .to_guests
            .iter()
            .any(|m| matches!(m, ServerMsg::Menu { .. })));
        assert!(db.item("mango_pudding").unwrap().is_some());

        apply(&db, &demo(), &owner, parse_intent("hide macaroni"));
        let guest_menu = apply(&db, &demo(), &guest, Intent::ShowMenu);
        let hidden = guest_menu.to_self.iter().any(|m| match m {
            ServerMsg::Menu { items } => {
                items.iter().any(|i| i.id == "macaroni" && !i.available)
                    || !items.iter().any(|i| i.id == "macaroni")
            }
            _ => false,
        });
        assert!(hidden, "macaroni should be absent from the guest board");
    }

    #[test]
    fn a_command_that_names_a_dish_is_not_an_order() {
        let (db, _guest) = guest_db();
        assert_eq!(
            intents_for_chat(&db, "remove latte"),
            vec![Intent::Remove {
                item_id: "latte".into(),
                qty: 1
            }]
        );
        assert_eq!(
            intents_for_chat(&db, "hide macaroni"),
            vec![Intent::MenuHide {
                item_id: "macaroni".into()
            }]
        );
    }

    #[test]
    fn owner_dish_is_orderable_by_chat_with_a_count() {
        let db = MemStore::new("panda");
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        apply(
            &db,
            &demo(),
            &owner,
            parse_intent("add item mango pudding 3.20 dessert"),
        );
        assert_eq!(
            intents_for_chat(&db, "two mango puddings"),
            vec![Intent::Add {
                item_id: "mango_pudding".into(),
                qty: 2
            }]
        );
    }

    #[test]
    fn owner_takes_a_dish_off_and_puts_it_back() {
        let db = MemStore::new("panda");
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        let guest = db.create_session(Role::Guest, "Mei").unwrap();

        let off = apply(&db, &demo(), &owner, parse_intent("hide macaroni"));
        assert!(first_error(&off).is_none());
        assert!(!db.item("macaroni").unwrap().unwrap().available);
        assert!(db
            .menu_available()
            .unwrap()
            .iter()
            .all(|i| i.id != "macaroni"));
        // A guest cannot order what is off the board.
        let blocked = apply(
            &db,
            &demo(),
            &guest,
            Intent::Add {
                item_id: "macaroni".into(),
                qty: 1,
            },
        );
        assert!(first_error(&blocked).is_some());

        let on = apply(&db, &demo(), &owner, parse_intent("show macaroni"));
        assert!(first_error(&on).is_none());
        assert!(db.item("macaroni").unwrap().unwrap().available);
        // Everyone in the room is handed the new board.
        assert!(on
            .to_guests
            .iter()
            .any(|m| matches!(m, ServerMsg::Menu { .. })));
        assert!(first_error(&apply(
            &db,
            &demo(),
            &guest,
            Intent::Add {
                item_id: "macaroni".into(),
                qty: 1
            }
        ))
        .is_none());
    }

    #[test]
    fn only_the_owner_may_put_a_dish_back() {
        let (db, guest) = guest_db();
        let r = apply(
            &db,
            &demo(),
            &guest,
            Intent::MenuShow {
                item_id: "macaroni".into(),
            },
        );
        assert!(first_error(&r).unwrap().contains("owner"));
    }

    fn assistant_text(a: &Apply) -> Option<&str> {
        a.to_self.iter().find_map(|m| match m {
            ServerMsg::Assistant { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    fn order_from(a: &Apply) -> Option<&causewaybay_panda_protocol::wire::OrderView> {
        a.to_self.iter().find_map(|m| match m {
            ServerMsg::OrderUpdate { order } => Some(order),
            _ => None,
        })
    }

    #[test]
    fn the_faucet_tops_a_guest_up_and_then_stops() {
        let (db, guest) = guest_db();
        let shop = demo();
        // The grant, then the faucet, up to the cap and no further.
        for _ in 0..10 {
            apply(&db, &shop, &guest, Intent::Faucet);
        }
        let balance = db.balance(&guest.id).unwrap();
        assert_eq!(balance, causewaybay_panda_protocol::FAUCET_CAP);
        let refused = apply(&db, &shop, &guest, Intent::Faucet);
        assert!(first_error(&refused).unwrap().contains("faucet stops"));
    }

    #[test]
    fn a_live_shop_has_no_faucet_to_turn_on() {
        let (db, guest) = guest_db();
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
        assert!(live.onchain());
        let r = apply(&db, &live, &guest, Intent::Faucet);
        assert!(first_error(&r).unwrap().contains("real USDC"));
    }

    #[test]
    fn the_owner_has_no_purse_to_fill() {
        let db = MemStore::new("panda");
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        let r = apply(&db, &demo(), &owner, Intent::Faucet);
        assert!(first_error(&r).unwrap().contains("guests"));
    }

    #[test]
    fn setting_a_count_to_zero_takes_the_line_off() {
        let (db, guest) = guest_db();
        let shop = demo();
        apply(&db, &shop, &guest, parse_intent("three lattes"));
        let three = apply(
            &db,
            &shop,
            &guest,
            Intent::SetQty {
                item_id: "latte".into(),
                qty: 3,
            },
        );
        assert_eq!(cart_shown(&three), Some("HK$114.00"));

        let one = apply(
            &db,
            &shop,
            &guest,
            Intent::SetQty {
                item_id: "latte".into(),
                qty: 1,
            },
        );
        assert_eq!(
            cart_shown(&one),
            Some("HK$38.00"),
            "a count replaces, not adds"
        );

        let none = apply(
            &db,
            &shop,
            &guest,
            Intent::SetQty {
                item_id: "latte".into(),
                qty: 0,
            },
        );
        assert_eq!(cart_shown(&none), Some("HK$0.00"));
    }

    #[test]
    fn an_order_walks_the_counter_one_step_at_a_time() {
        let db = MemStore::new("panda");
        let shop = demo();
        let guest = db.create_session(Role::Guest, "Mei").unwrap();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();

        apply(&db, &shop, &guest, parse_intent("latte"));
        let paid = apply(
            &db,
            &shop,
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        let order = order_from(&paid).expect("a paid order lands on the counter");
        assert_eq!(order.order_no, 1, "guests are given a number to be called");
        assert_eq!(order.status, OrderStatus::Placed);
        let id = order.id.clone();
        // The counter is told without asking.
        assert!(paid
            .to_owners
            .iter()
            .any(|m| matches!(m, ServerMsg::OrderUpdate { .. })));

        for want in [
            OrderStatus::Preparing,
            OrderStatus::Ready,
            OrderStatus::Collected,
        ] {
            let step = apply(
                &db,
                &shop,
                &owner,
                Intent::OrderAdvance {
                    order_id: id.clone(),
                    status: String::new(),
                },
            );
            assert!(first_error(&step).is_none(), "{:?}", first_error(&step));
            assert_eq!(order_from(&step).unwrap().status, want);
            // The table that placed it hears about it — and only that table.
            assert!(step.to_guests.is_empty(), "no broadcast to the whole room");
            let (sid, msg) = step.to_session.first().expect("the guest is told");
            assert_eq!(sid, &guest.id);
            assert!(matches!(msg, ServerMsg::OrderUpdate { .. }));
        }

        let past_the_end = apply(
            &db,
            &shop,
            &owner,
            Intent::OrderAdvance {
                order_id: id.clone(),
                status: String::new(),
            },
        );
        assert!(first_error(&past_the_end)
            .unwrap()
            .contains("already finished"));
    }

    #[test]
    fn the_counter_cannot_skip_a_step() {
        let db = MemStore::new("panda");
        let shop = demo();
        let guest = db.create_session(Role::Guest, "Mei").unwrap();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        apply(&db, &shop, &guest, parse_intent("egg tart"));
        let paid = apply(
            &db,
            &shop,
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        let id = order_from(&paid).unwrap().id.clone();
        let jump = apply(
            &db,
            &shop,
            &owner,
            Intent::OrderAdvance {
                order_id: id,
                status: "collected".into(),
            },
        );
        assert!(first_error(&jump).unwrap().contains("cannot become"));
    }

    #[test]
    fn an_open_order_can_be_cancelled_and_a_finished_one_cannot() {
        let db = MemStore::new("panda");
        let shop = demo();
        let guest = db.create_session(Role::Guest, "Mei").unwrap();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        apply(&db, &shop, &guest, parse_intent("latte"));
        let paid = apply(
            &db,
            &shop,
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        let id = order_from(&paid).unwrap().id.clone();
        let off = apply(
            &db,
            &shop,
            &owner,
            Intent::OrderCancel {
                order_id: id.clone(),
            },
        );
        assert_eq!(order_from(&off).unwrap().status, OrderStatus::Cancelled);
        assert!(
            db.open_orders().unwrap().is_empty(),
            "cancelled leaves the queue"
        );

        // A second tap on the same button is harmless, not an error.
        let again = apply(&db, &shop, &owner, Intent::OrderCancel { order_id: id });
        assert!(first_error(&again).is_none());
        assert_eq!(order_from(&again).unwrap().status, OrderStatus::Cancelled);

        // But money already handed over cannot be un-handed.
        let second = db.create_session(Role::Guest, "Ling").unwrap();
        apply(&db, &shop, &second, parse_intent("egg tart"));
        let paid = apply(
            &db,
            &shop,
            &second,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        let done = order_from(&paid).unwrap().id.clone();
        for _ in 0..3 {
            apply(
                &db,
                &shop,
                &owner,
                Intent::OrderAdvance {
                    order_id: done.clone(),
                    status: String::new(),
                },
            );
        }
        let too_late = apply(&db, &shop, &owner, Intent::OrderCancel { order_id: done });
        assert!(first_error(&too_late).unwrap().contains("cannot become"));
    }

    #[test]
    fn only_the_counter_moves_an_order() {
        let db = MemStore::new("panda");
        let shop = demo();
        let guest = db.create_session(Role::Guest, "Mei").unwrap();
        apply(&db, &shop, &guest, parse_intent("latte"));
        let paid = apply(
            &db,
            &shop,
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        let id = order_from(&paid).unwrap().id.clone();
        let r = apply(
            &db,
            &shop,
            &guest,
            Intent::OrderAdvance {
                order_id: id,
                status: String::new(),
            },
        );
        assert!(first_error(&r).unwrap().contains("only the counter"));
    }

    #[test]
    fn the_board_reads_in_whatever_the_shop_is_set_to() {
        let krw_denom = causewaybay_panda_protocol::Denom::preset("KRW").unwrap();
        let db = MemStore::new("panda").with_denom(krw_denom.clone());
        let guest = db.create_session(Role::Guest, "Mei").unwrap();
        let shop = Shop::resolve(
            &crate::shop::Config {
                denom: Some("KRW".into()),
                ..Default::default()
            },
            crate::settlement::Settle::demo(),
        );
        apply(&db, &shop, &guest, parse_intent("latte"));
        let cart = apply(&db, &shop, &guest, Intent::ShowCart);
        // The same coffee, the same USDC, a different board.
        assert_eq!(cart_total(&cart), Some("4.871795"));
        assert_eq!(cart_shown(&cart), Some("₩6,723"));
    }

    #[test]
    fn a_guest_is_told_which_money_they_are_spending() {
        let (db, guest) = guest_db();
        let sim = apply(&db, &demo(), &guest, Intent::Help);
        assert!(assistant_text(&sim).unwrap().contains("Causewaybay Coin"));
    }

    fn live_shop() -> Shop {
        Shop::resolve(
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
        )
    }

    #[test]
    fn in_a_simulation_any_pin_opens_the_counter() {
        let db = MemStore::new("panda");
        let sim = demo();
        for pin in ["panda", "nope", "", "0000"] {
            let row = login(&db, &sim, Role::Owner, "Wing", pin, "").unwrap();
            assert_eq!(row.role, Role::Owner, "pin {pin:?}");
        }
    }

    #[test]
    fn a_live_shop_keeps_its_pin_and_a_remembered_id_is_never_a_key() {
        let db = MemStore::new("panda");
        let live = live_shop();
        assert!(login(&db, &live, Role::Owner, "Wing", "nope", "")
            .unwrap_err()
            .contains("pin"));
        let owner = login(&db, &live, Role::Owner, "Wing", "panda", "").unwrap();
        // The browser remembers the id, but not the pin: refused.
        let err = login(&db, &live, Role::Owner, "", "0000", &owner.id).unwrap_err();
        assert!(err.contains("pin"));
        // With the pin, the same session comes back rather than a new one.
        let again = login(&db, &live, Role::Owner, "", "panda", &owner.id).unwrap();
        assert_eq!(again.id, owner.id);
        // A guest's remembered id does not need a pin, and cannot become owner.
        let guest = login(&db, &live, Role::Guest, "Mei", "", "").unwrap();
        assert_eq!(
            login(&db, &live, Role::Guest, "", "", &guest.id)
                .unwrap()
                .id,
            guest.id
        );
        let other = login(&db, &live, Role::Owner, "X", "panda", &guest.id).unwrap();
        assert_ne!(other.id, guest.id);
    }

    #[test]
    fn chat_and_button_same_cart() {
        let (db, guest) = guest_db();
        apply(&db, &demo(), &guest, parse_intent("latte"));
        apply(
            &db,
            &demo(),
            &guest,
            Intent::from_action(
                causewaybay_panda_protocol::ActionName::Add,
                "latte".into(),
                1,
                None,
                None,
                String::new(),
                String::new(),
                String::new(),
                false,
            ),
        );
        let cart = apply(&db, &demo(), &guest, Intent::ShowCart);
        // A latte is HK$38 on the board; two of them read as one line.
        assert_eq!(cart_shown(&cart), Some("HK$76.00"));
        // And settle in the unit the chain would move.
        assert_eq!(cart_total(&cart), Some("9.74359"));
    }

    #[test]
    fn insufficient_funds() {
        let db = MemStore::new("panda");
        let guest = db.create_session(Role::Guest, "Mei").unwrap();
        // 50 USDC grant; add many french toasts at 5.40 until over.
        for _ in 0..10 {
            db.add_to_cart(&guest.id, "french_toast", 1).unwrap();
        }
        let r = apply(
            &db,
            &demo(),
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        assert!(first_error(&r).unwrap().contains("need"));
    }

    fn owner_dash(a: &[ServerMsg]) -> Option<&ServerMsg> {
        a.iter().find(|m| matches!(m, ServerMsg::Dashboard { .. }))
    }

    fn guest_dash(a: &[ServerMsg]) -> Option<&ServerMsg> {
        a.iter()
            .find(|m| matches!(m, ServerMsg::GuestDashboard { .. }))
    }

    #[test]
    fn the_owner_is_handed_the_day_on_login_and_after_every_move() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        let first = on_login(&db, &demo(), &owner, "", false, false);
        match owner_dash(&first).unwrap() {
            ServerMsg::Dashboard {
                orders,
                open,
                guests,
                top,
                ..
            } => {
                assert_eq!((*orders, *open, *guests), (0, 0, 0));
                assert!(top.is_empty());
            }
            _ => unreachable!(),
        }

        for i in intents_for_chat(&db, "two lattes and an egg tart") {
            apply(&db, &demo(), &guest, i);
        }
        let paid = apply(
            &db,
            &demo(),
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        // The counter's card comes with the payment, not on a refresh.
        match owner_dash(&paid.to_owners).unwrap() {
            ServerMsg::Dashboard {
                orders,
                open,
                placed,
                guests,
                average_display,
                total_display,
                top,
                ..
            } => {
                assert_eq!((*orders, *open, *placed, *guests), (1, 1, 1, 1));
                assert_eq!(total_display, "HK$86.00");
                assert_eq!(average_display, "HK$86.00");
                assert_eq!(top[0].item_id, "latte");
                assert_eq!(top[0].qty, 2);
                assert_eq!(top[0].revenue_display, "HK$76.00");
                assert_eq!(top[1].item_id, "egg_tart");
            }
            _ => unreachable!(),
        }
        // The guest's own card rides along with the receipt.
        match guest_dash(&paid.to_self).unwrap() {
            ServerMsg::GuestDashboard {
                orders,
                spent_display,
                favourite,
                favourite_qty,
                open,
                last_status,
                ..
            } => {
                assert_eq!((*orders, *open), (1, 1));
                assert_eq!(spent_display, "HK$86.00");
                assert_eq!(favourite, "Hot latte");
                assert_eq!(*favourite_qty, 2);
                assert_eq!(last_status, "placed");
            }
            _ => unreachable!(),
        }

        // Working the ticket moves the kitchen figures on both cards.
        let order_id = db.open_orders().unwrap()[0].id.clone();
        let moved = apply(
            &db,
            &demo(),
            &owner,
            Intent::OrderAdvance {
                order_id: order_id.clone(),
                status: String::new(),
            },
        );
        match owner_dash(&moved.to_owners).unwrap() {
            ServerMsg::Dashboard {
                placed, preparing, ..
            } => assert_eq!((*placed, *preparing), (0, 1)),
            _ => unreachable!(),
        }
        let to_table: Vec<ServerMsg> = moved
            .to_session
            .iter()
            .filter(|(s, _)| *s == guest.id)
            .map(|(_, m)| m.clone())
            .collect();
        match guest_dash(&to_table).unwrap() {
            ServerMsg::GuestDashboard {
                last_status, open, ..
            } => {
                assert_eq!(last_status, "preparing");
                assert_eq!(*open, 1);
            }
            _ => unreachable!(),
        }
        apply(
            &db,
            &demo(),
            &owner,
            Intent::OrderAdvance {
                order_id: order_id.clone(),
                status: "ready".into(),
            },
        );
        let done = apply(
            &db,
            &demo(),
            &owner,
            Intent::OrderAdvance {
                order_id,
                status: "collected".into(),
            },
        );
        match owner_dash(&done.to_owners).unwrap() {
            ServerMsg::Dashboard {
                open,
                collected,
                orders,
                ..
            } => assert_eq!((*open, *collected, *orders), (0, 1, 1)),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_cancelled_order_counts_for_nobody() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        apply(
            &db,
            &demo(),
            &guest,
            Intent::Add {
                item_id: "latte".into(),
                qty: 1,
            },
        );
        apply(
            &db,
            &demo(),
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        let order_id = db.open_orders().unwrap()[0].id.clone();
        let a = apply(&db, &demo(), &owner, Intent::OrderCancel { order_id });
        match owner_dash(&a.to_owners).unwrap() {
            ServerMsg::Dashboard {
                cancelled,
                guests,
                top,
                open,
                ..
            } => {
                assert_eq!((*cancelled, *guests, *open), (1, 0, 0));
                assert!(top.is_empty(), "a cancelled latte did not sell");
            }
            _ => unreachable!(),
        }
        let g = apply(&db, &demo(), &guest, Intent::Dashboard);
        match guest_dash(&g.to_self).unwrap() {
            ServerMsg::GuestDashboard {
                orders,
                spent_display,
                favourite,
                last_status,
                ..
            } => {
                assert_eq!(*orders, 0);
                assert_eq!(spent_display, "HK$0.00");
                assert_eq!(favourite, "");
                assert_eq!(last_status, "cancelled");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn today_asks_for_the_card_that_fits_the_asker() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        let g = apply(&db, &demo(), &guest, parse_intent("today"));
        assert!(guest_dash(&g.to_self).is_some());
        assert!(owner_dash(&g.to_self).is_none());
        let o = apply(&db, &demo(), &owner, parse_intent("how are we doing"));
        assert!(owner_dash(&o.to_self).is_some());
        assert!(guest_dash(&o.to_self).is_none());
    }

    #[test]
    fn what_the_model_says_becomes_a_line_and_buttons_for_dishes_on_the_board() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        apply(
            &db,
            &demo(),
            &owner,
            Intent::MenuHide {
                item_id: "macaroni".into(),
            },
        );
        let a = apply(
            &db,
            &demo(),
            &guest,
            Intent::Say {
                text: "Try the milk tea with an egg tart.".into(),
                suggest: vec![
                    "milk_tea".into(),
                    "egg_tart".into(),
                    "macaroni".into(),
                    "unicorn".into(),
                ],
            },
        );
        match &a.to_self[0] {
            ServerMsg::Assistant { text, buttons } => {
                assert_eq!(text, "Try the milk tea with an egg tart.");
                let ids: Vec<&str> = buttons.iter().map(|b| b.item_id.as_str()).collect();
                assert_eq!(
                    ids,
                    vec!["milk_tea", "egg_tart"],
                    "off the board and unknown are dropped"
                );
                assert_eq!(buttons[0].label, "Silk milk tea");
            }
            other => panic!("{other:?}"),
        }
        // The owner is answered in words only.
        let a = apply(
            &db,
            &demo(),
            &owner,
            Intent::Say {
                text: "Latte sold best.".into(),
                suggest: vec!["latte".into()],
            },
        );
        assert!(
            matches!(&a.to_self[0], ServerMsg::Assistant { buttons, .. } if buttons.is_empty())
        );
    }

    #[test]
    fn the_facts_a_model_gets_are_the_askers_own() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        for i in intents_for_chat(&db, "two lattes") {
            apply(&db, &demo(), &guest, i);
        }
        apply(
            &db,
            &demo(),
            &guest,
            Intent::Pay {
                method: PayMethod::Coin,
                tx_hash: String::new(),
            },
        );
        apply(
            &db,
            &demo(),
            &guest,
            Intent::Add {
                item_id: "egg_tart".into(),
                qty: 1,
            },
        );

        let f = facts_for(&db, &demo(), &owner).join("\n");
        assert!(f.contains("Takings today: HK$76.00 from 1 order,"), "{f}");
        assert!(f.contains("1 waiting"), "{f}");
        assert!(f.contains("Hot latte ×2 (HK$76.00)"), "{f}");
        assert!(
            !f.contains("Mei"),
            "the owner's facts are the shop's, not one table's"
        );

        let f = facts_for(&db, &demo(), &guest).join("\n");
        assert!(
            f.contains("Mei: 1 orders here, spent HK$76.00, usual dish Hot latte (×2)"),
            "{f}"
        );
        assert!(f.contains("open orders: 1; the latest is placed"), "{f}");
        assert!(f.contains("cart now: 1× Egg tart, total HK$10.00"), "{f}");
        assert!(!f.contains("Takings"), "a guest is never handed the till");
    }

    #[test]
    fn the_kitchen_switch_is_the_hosts_and_the_owners() {
        let (db, guest) = guest_db();
        let owner = db.create_session(Role::Owner, "Wing").unwrap();
        assert_eq!(
            first_error(&apply(&db, &demo(), &guest, Intent::Kitchen { on: true })),
            Some("only the owner decides who works the kitchen")
        );
        assert!(
            first_error(&apply(&db, &demo(), &owner, Intent::Kitchen { on: true }))
                .unwrap()
                .contains("host")
        );
        // And the owner is told where the switch stands on the way in.
        let f = on_login(&db, &demo(), &owner, "", false, true);
        assert!(f
            .iter()
            .any(|m| matches!(m, ServerMsg::Kitchen { on: true })));
    }
}
