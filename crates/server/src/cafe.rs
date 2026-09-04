//! Apply a parsed intent to a logged-in session. Chat and buttons share this.

use causewaybay_panda_protocol::chain::chain_id_hex;
use causewaybay_panda_protocol::intent::{parse_all_adds, parse_intent, split_qty, Intent};
use causewaybay_panda_protocol::money::format_usdc;
use causewaybay_panda_protocol::wire::{BigButton, PayMethod, ServerMsg};
use causewaybay_panda_protocol::Role;

use crate::db::{Db, SessionRow};
use crate::settlement::{is_tx_hash, Settle};

#[derive(Debug)]
pub struct Apply {
    pub to_self: Vec<ServerMsg>,
    pub to_owners: Vec<ServerMsg>,
    pub to_guests: Vec<ServerMsg>,
}

impl Apply {
    fn one(msg: ServerMsg) -> Self {
        Self {
            to_self: vec![msg],
            to_owners: Vec::new(),
            to_guests: Vec::new(),
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self::one(ServerMsg::Error {
            message: message.into(),
        })
    }
}

pub fn intents_for_chat(db: &Db, text: &str) -> Vec<Intent> {
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

pub fn apply(db: &Db, settle: &Settle, session: &SessionRow, intent: Intent) -> Apply {
    match intent {
        Intent::Help => Apply::one(help(session.role)),
        Intent::ShowMenu => menu_msg(db, session.role),
        Intent::ShowCart => cart_msg(db, session),
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
                    let mut out = cart_msg(db, session);
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
            cart_msg(db, session)
        }
        Intent::Clear => {
            if let Err(e) = db.clear_cart(&session.id) {
                return Apply::err(e);
            }
            cart_msg(db, session)
        }
        Intent::Pay { method, tx_hash } => pay(db, settle, session, method, &tx_hash),
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
        Intent::Unknown(raw) => Apply::one(ServerMsg::Assistant {
            text: format!("I did not catch “{raw}”. Try a dish name, or tap a button."),
            buttons: vec![BigButton::menu(), BigButton::help()],
        }),
    }
}

/// A menu change everybody must see: the room gets the new board, the owner
/// gets a line of confirmation with it.
fn menu_broadcast(db: &Db, said: String) -> Apply {
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
    }
}

fn help(role: Role) -> ServerMsg {
    match role {
        Role::Guest => ServerMsg::Assistant {
            text: "Say a dish — “latte”, “two pineapple buns” — or tap a button. Pay in USDC when the cart looks right.".into(),
            buttons: vec![BigButton::menu(), BigButton::cart(), BigButton::pay()],
        },
        Role::Owner => ServerMsg::Assistant {
            text: "Say “add item mango pudding 3.20 dessert”, “hide macaroni”, “show macaroni”, “payments”, or “orders”. Tap a dish to take it off the board or put it back.".into(),
            buttons: vec![
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

fn menu_msg(db: &Db, role: Role) -> Apply {
    let items = match role {
        Role::Guest => db.menu_available().unwrap_or_default(),
        Role::Owner => db.menu().unwrap_or_default(),
    };
    let mut buttons: Vec<BigButton> = items
        .iter()
        .filter(|i| i.available)
        .take(6)
        .map(|i| BigButton::add(&i.id, &format!("{} · {}", i.name, i.price_usdc)))
        .collect();
    if role == Role::Guest {
        buttons.push(BigButton::pay());
    }
    let mut out = Apply::one(ServerMsg::Menu { items });
    out.to_self.push(ServerMsg::Assistant {
        text: "Today at Causewaybay Coffee.".into(),
        buttons,
    });
    out
}

fn cart_msg(db: &Db, session: &SessionRow) -> Apply {
    match db.cart(&session.id) {
        Ok((lines, total, balance)) => Apply::one(ServerMsg::Cart {
            lines,
            total_usdc: format_usdc(total),
            balance_usdc: format_usdc(balance),
        }),
        Err(e) => Apply::err(e),
    }
}

/// Two settlements share one till. `Usdc` draws the guest's play-money grant
/// and is done in one step. `Wallet` moves real USDC on Cronos: the first call
/// only hands back a transaction to sign, and nothing is debited or recorded
/// until the browser returns its hash.
fn pay(db: &Db, settle: &Settle, session: &SessionRow, method: PayMethod, tx_hash: &str) -> Apply {
    if session.role != Role::Guest {
        return Apply::err("the owner does not pay the till");
    }
    if method == PayMethod::Wallet {
        if !settle.onchain() {
            return Apply::err(format!("wallet payment is off: {}", settle.reason()));
        }
        if tx_hash.trim().is_empty() {
            return prepare_wallet_payment(db, settle, session);
        }
        if !is_tx_hash(tx_hash) {
            return Apply::err("that is not a transaction hash");
        }
    }
    let (method_s, debit) = match method {
        PayMethod::Usdc => ("usdc", true),
        PayMethod::Wallet => ("wallet", false),
    };
    match db.checkout(&session.id, method_s, tx_hash, debit) {
        Ok((order_id, total, hash)) => {
            let payments = db.payments().unwrap_or_default();
            let (lines, _, balance) = db.cart(&session.id).unwrap_or_default();
            let said = match method {
                PayMethod::Wallet => format!(
                    "Paid {} USDC in USDC on {}. The panda is on it.",
                    format_usdc(total),
                    settle.chain.name
                ),
                PayMethod::Usdc => format!(
                    "Paid {} USDC from the house grant. The panda is on it.",
                    format_usdc(total)
                ),
            };
            Apply {
                to_self: vec![
                    ServerMsg::Paid {
                        order_id,
                        amount_usdc: format_usdc(total),
                        method,
                        explorer_url: settle.explorer_url(&hash),
                        tx_hash: hash,
                        call_data: settle.call_data(total),
                    },
                    ServerMsg::Cart {
                        lines,
                        total_usdc: format_usdc(0),
                        balance_usdc: format_usdc(balance),
                    },
                    ServerMsg::Assistant {
                        text: said,
                        buttons: vec![BigButton::menu()],
                    },
                ],
                to_owners: vec![ServerMsg::Payments { payments }],
                to_guests: Vec::new(),
            }
        }
        Err(e) => Apply::err(e),
    }
}

/// Price the cart and hand back the transfer for the wallet to sign. The cart
/// is left exactly as it was: an unsigned or failed transaction changes nothing.
fn prepare_wallet_payment(db: &Db, settle: &Settle, session: &SessionRow) -> Apply {
    let (lines, total, _) = match db.cart(&session.id) {
        Ok(c) => c,
        Err(e) => return Apply::err(e),
    };
    if lines.is_empty() {
        return Apply::err("cart is empty");
    }
    Apply::one(ServerMsg::PayRequest {
        amount_usdc: format_usdc(total),
        amount_micro: settle.atomic(total).to_string(),
        token: settle.token_address().to_string(),
        treasury: settle.treasury_address().to_string(),
        chain_id: settle.chain.chain_id,
        chain_id_hex: chain_id_hex(settle.chain.chain_id),
        call_data: settle.call_data(total),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use causewaybay_panda_protocol::wire::MenuDraft;
    use causewaybay_panda_protocol::GUEST_GRANT;

    /// Play money, the way every existing flow settles.
    fn demo() -> Settle {
        Settle::demo()
    }

    fn guest_db() -> (Db, SessionRow) {
        let db = Db::memory("panda").unwrap();
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
                method: PayMethod::Usdc,
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
                assert_eq!(payments[0].method, "usdc");
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
                method: PayMethod::Usdc,
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
        let db = Db::memory("panda").unwrap();
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
        let db = Db::memory("panda").unwrap();
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
        let db = Db::memory("panda").unwrap();
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
            ),
        );
        let cart = apply(&db, &demo(), &guest, Intent::ShowCart);
        assert_eq!(cart_total(&cart), Some("9.6")); // 4.80 × 2
    }

    #[test]
    fn insufficient_funds() {
        let db = Db::memory("panda").unwrap();
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
                method: PayMethod::Usdc,
                tx_hash: String::new(),
            },
        );
        assert!(first_error(&r).unwrap().contains("need"));
    }
}
