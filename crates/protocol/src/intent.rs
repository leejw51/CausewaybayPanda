//! Chat → Intent. Always available, no network. Buttons emit the same intents.

use crate::money::parse_usdc;
use crate::seed::cafe_menu;
use crate::wire::{ActionName, ClientMsg, MenuDraft, PayMethod};

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Help,
    ShowMenu,
    ShowCart,
    Add { item_id: String, qty: u32 },
    Remove { item_id: String, qty: u32 },
    Clear,
    SetQty { item_id: String, qty: u32 },
    Faucet,
    Pay { method: PayMethod, tx_hash: String },
    ListPayments,
    ListOrders,
    MenuUpsert { draft: MenuDraft },
    MenuHide { item_id: String },
    MenuShow { item_id: String },
    OrderAdvance { order_id: String, status: String },
    OrderCancel { order_id: String },
    Auto { on: bool },
    Unknown(String),
}

impl Intent {
    pub fn from_action(
        name: ActionName,
        item_id: String,
        qty: u32,
        method: Option<PayMethod>,
        item: Option<MenuDraft>,
        tx_hash: String,
        order_id: String,
        status: String,
        on: bool,
    ) -> Self {
        let keep_zero = matches!(name, ActionName::SetQty);
        let qty = if qty == 0 && !keep_zero { 1 } else { qty };
        match name {
            ActionName::Add => Intent::Add { item_id, qty },
            ActionName::Remove => Intent::Remove { item_id, qty },
            // A count of zero from the cart's minus button means "take it off".
            ActionName::SetQty => Intent::SetQty { item_id, qty },
            ActionName::Faucet => Intent::Faucet,
            ActionName::OrderAdvance => Intent::OrderAdvance { order_id, status },
            ActionName::OrderCancel => Intent::OrderCancel { order_id },
            ActionName::Auto => Intent::Auto { on },
            ActionName::Cart => Intent::ShowCart,
            ActionName::Pay => Intent::Pay {
                method: method.unwrap_or(PayMethod::Coin),
                tx_hash,
            },
            ActionName::Clear => Intent::Clear,
            ActionName::Menu => Intent::ShowMenu,
            ActionName::Help => Intent::Help,
            ActionName::MenuUpsert => match item {
                Some(draft) => Intent::MenuUpsert { draft },
                None => Intent::Unknown("missing item".into()),
            },
            ActionName::MenuHide => Intent::MenuHide { item_id },
            ActionName::MenuShow => Intent::MenuShow { item_id },
            ActionName::ListPayments => Intent::ListPayments,
            ActionName::ListOrders => Intent::ListOrders,
        }
    }

    pub fn from_client(msg: &ClientMsg) -> Option<Self> {
        match msg {
            ClientMsg::Chat { text } => Some(parse_intent(text)),
            ClientMsg::Action {
                name,
                item_id,
                qty,
                method,
                item,
                tx_hash,
                order_id,
                status,
                on,
            } => Some(Self::from_action(
                *name,
                item_id.clone(),
                *qty,
                *method,
                item.clone(),
                tx_hash.clone(),
                order_id.clone(),
                status.clone(),
                *on,
            )),
            _ => None,
        }
    }
}

pub fn parse_intent(text: &str) -> Intent {
    parse_intent_with(text, &catalog())
}

fn catalog() -> Vec<Alias> {
    cafe_menu()
        .iter()
        .map(|s| Alias {
            id: s.id.to_string(),
            names: aliases_for(s.id, s.name, s.name_zh),
        })
        .collect()
}

struct Alias {
    id: String,
    names: Vec<String>,
}

fn aliases_for(id: &str, name: &str, name_zh: &str) -> Vec<String> {
    let mut v = vec![
        id.to_string(),
        id.replace('_', " "),
        name.to_lowercase(),
        name_zh.to_string(),
    ];
    match id {
        "latte" => v.extend(["hot latte".into(), "咖啡".into(), "鮮奶咖啡".into()]),
        "iced_latte" => v.extend(["ice latte".into(), "iced coffee".into(), "凍咖啡".into()]),
        "cappuccino" => v.extend(["cap".into(), "capp".into()]),
        "yuenyeung" => v.extend(["yuan yang".into(), "yuanyang".into(), "coffee tea".into()]),
        "milk_tea" => v.extend([
            "milk tea".into(),
            "hong kong milk tea".into(),
            "silk stocking".into(),
            "奶茶".into(),
        ]),
        "lemon_tea" => v.extend([
            "lemon".into(),
            "iced tea".into(),
            "檸茶".into(),
            "冻柠茶".into(),
        ]),
        "pineapple_bun" => v.extend([
            "bolo bao".into(),
            "bolo".into(),
            "pineapple".into(),
            "菠蘿".into(),
            "菠萝包".into(),
        ]),
        "egg_tart" => v.extend(["tart".into(), "eggtart".into(), "蛋挞".into()]),
        "french_toast" => v.extend(["toast".into(), "west toast".into(), "西多士".into()]),
        "macaroni" => v.extend(["mac".into(), "pasta".into(), "通粉".into()]),
        "panda_bun" => v.extend(["panda".into(), "bao".into(), "饅頭".into()]),
        _ => {}
    }
    v.sort();
    v.dedup();
    v
}

fn norm(s: &str) -> String {
    let mut t = s.to_lowercase();
    for pat in ["please", "pls", "thanks", "thank you"] {
        t = t.replace(pat, " ");
    }
    t = t.replace(['，', '。', '！', '？', ',', '!', '?'], " ");
    while t.ends_with('.') {
        t.pop();
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "two lattes" → (2, "lattes"). Public so the server can count a dish the
/// owner added after boot, which the seed catalog does not know.
pub fn split_qty(s: &str) -> (u32, &str) {
    qty_prefix(s)
}

fn qty_prefix(s: &str) -> (u32, &str) {
    let s = s.trim();
    let (num, rest) = if let Some((n, r)) = s.split_once(' ') {
        (n, r)
    } else {
        return (1, s);
    };
    let lower = num.to_lowercase();
    let q = match lower.as_str() {
        "a" | "an" | "one" | "1" | "一" => 1,
        "two" | "2" | "兩" | "两" | "二" => 2,
        "three" | "3" | "三" => 3,
        "four" | "4" | "四" => 4,
        "five" | "5" | "五" => 5,
        "six" | "6" | "六" => 6,
        other => {
            if let Ok(n) = other.parse::<u32>() {
                if (1..100).contains(&n) {
                    return (n, rest);
                }
            }
            return (1, s);
        }
    };
    (q, rest)
}

fn find_item<'a>(hay: &str, cat: &'a [Alias]) -> Option<&'a Alias> {
    let h = hay.to_lowercase();
    let mut best: Option<(&Alias, usize)> = None;
    for a in cat {
        for n in &a.names {
            if n.is_empty() {
                continue;
            }
            if h == *n || h.contains(n) {
                let score = n.chars().count();
                if best.map(|(_, s)| score > s).unwrap_or(true) {
                    best = Some((a, score));
                }
            }
        }
    }
    best.map(|(a, _)| a)
}

fn parse_intent_with(text: &str, cat: &[Alias]) -> Intent {
    let raw = text.trim();
    if raw.is_empty() {
        return Intent::Help;
    }
    let n = norm(raw);

    if is_cmd(
        &n,
        &["help", "hi", "hello", "hey", "你好", "哈囉"],
        &["what can", "how do"],
    ) {
        return Intent::Help;
    }
    if is_cmd(
        &n,
        &["menu", "餐牌", "菜單", "菜单", "list"],
        &[
            "show menu",
            "what's on",
            "what is on",
            "list menu",
            "the menu",
        ],
    ) {
        return Intent::ShowMenu;
    }
    if is_cmd(
        &n,
        &["cart", "basket", "購物車", "購物"],
        &["my order", "what do i have", "show cart", "the cart"],
    ) {
        return Intent::ShowCart;
    }
    if is_cmd(
        &n,
        &[
            "pay", "checkout", "bill", "settle", "usdc", "付款", "埋單", "买单",
        ],
        &["pay now", "pay usdc"],
    ) {
        return Intent::Pay {
            method: PayMethod::Coin,
            tx_hash: String::new(),
        };
    }
    if is_cmd(
        &n,
        &["wallet", "metamask"],
        &["pay wallet", "use wallet", "pay with wallet"],
    ) {
        return Intent::Pay {
            method: PayMethod::Wallet,
            tx_hash: String::new(),
        };
    }
    if is_cmd(
        &n,
        &["clear", "清空"],
        &["empty cart", "reset cart", "clear cart"],
    ) {
        return Intent::Clear;
    }
    if is_cmd(
        &n,
        &["auto off", "demo off", "stop demo", "stop auto", "stop"],
        &["stop running", "auto stop"],
    ) {
        return Intent::Auto { on: false };
    }
    if is_cmd(
        &n,
        &["auto", "demo", "auto on", "demo on", "run itself", "自動"],
        &["run the cafe", "run on its own", "start demo", "start auto"],
    ) {
        return Intent::Auto { on: true };
    }
    if is_cmd(
        &n,
        &["faucet", "topup", "top up", "增值", "top me up"],
        &[
            "more money",
            "more coin",
            "give me money",
            "test money",
            "top up",
        ],
    ) {
        return Intent::Faucet;
    }
    if is_cmd(
        &n,
        &["payments", "payment", "takings", "收款"],
        &["show payments", "list payments", "the payments"],
    ) {
        return Intent::ListPayments;
    }
    if is_cmd(
        &n,
        &["orders", "kitchen", "tickets"],
        &["show orders", "list orders", "the orders"],
    ) {
        return Intent::ListOrders;
    }

    // owner: "add mango pudding 3.20" / "add item mango pudding 3.20 dessert"
    if let Some(rest) = strip_cmd(&n, &["add item", "new item", "create item", "make item"]) {
        return parse_upsert(rest);
    }
    if let Some(rest) = strip_cmd(&n, &["hide", "remove item", "delete item", "unpublish"]) {
        if let Some(item) = find_item(rest, cat) {
            return Intent::MenuHide {
                item_id: item.id.clone(),
            };
        }
        let id = rest.replace(' ', "_");
        return Intent::MenuHide { item_id: id };
    }
    if let Some(rest) = strip_cmd(
        &n,
        &["unhide", "show", "republish", "bring back", "put back"],
    ) {
        if let Some(item) = find_item(rest, cat) {
            return Intent::MenuShow {
                item_id: item.id.clone(),
            };
        }
        let id = rest.replace(' ', "_");
        return Intent::MenuShow { item_id: id };
    }

    if let Some(rest) = strip_cmd(&n, &["remove", "delete", "drop", "不用", "不要"]) {
        let (qty, rest) = qty_prefix(rest);
        if let Some(item) = find_item(rest, cat) {
            return Intent::Remove {
                item_id: item.id.clone(),
                qty,
            };
        }
    }

    // "i want two lattes and an egg tart"
    let parts = split_items(&n);
    let mut adds: Vec<(String, u32)> = Vec::new();
    for part in parts {
        let part = strip_cmd(
            part,
            &[
                "i want",
                "i'd like",
                "id like",
                "give me",
                "get me",
                "can i have",
                "i'll have",
                "ill have",
                "add",
                "order",
                "要",
                "我要",
                "比我",
            ],
        )
        .unwrap_or(part);
        let (qty, rest) = qty_prefix(part);
        if let Some(item) = find_item(rest, cat) {
            adds.push((item.id.clone(), qty));
        }
    }
    if adds.len() == 1 {
        return Intent::Add {
            item_id: adds[0].0.clone(),
            qty: adds[0].1,
        };
    }
    if adds.len() > 1 {
        // Fold into the first; the server applies one intent per turn, so we
        // encode extras by returning the first and letting the caller loop
        // via parse_all.
        return Intent::Add {
            item_id: adds[0].0.clone(),
            qty: adds[0].1,
        };
    }

    // Bare item name.
    let (qty, rest) = qty_prefix(&n);
    if let Some(item) = find_item(rest, cat) {
        return Intent::Add {
            item_id: item.id.clone(),
            qty,
        };
    }

    Intent::Unknown(raw.to_string())
}

/// All add-lines in one utterance ("latte and egg tart").
pub fn parse_all_adds(text: &str) -> Vec<Intent> {
    let cat = catalog();
    let n = norm(text);
    let parts = split_items(&n);
    let mut out = Vec::new();
    for part in parts {
        let part = strip_cmd(
            part,
            &[
                "i want",
                "i'd like",
                "id like",
                "give me",
                "can i have",
                "add",
                "order",
                "要",
                "我要",
            ],
        )
        .unwrap_or(part);
        let (qty, rest) = qty_prefix(part);
        if let Some(item) = find_item(rest, &cat) {
            out.push(Intent::Add {
                item_id: item.id.clone(),
                qty,
            });
        }
    }
    if out.is_empty() {
        match parse_intent(text) {
            Intent::Unknown(_) => {}
            other => out.push(other),
        }
    }
    out
}

fn parse_upsert(rest: &str) -> Intent {
    // "mango pudding 3.20" or "mango pudding 3.20 dessert"
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let mut price_at = None;
    for (i, t) in tokens.iter().enumerate() {
        if parse_usdc(t).is_some() {
            price_at = Some(i);
            break;
        }
    }
    let Some(i) = price_at else {
        return Intent::Unknown(format!("add item {rest}"));
    };
    let name = tokens[..i].join(" ");
    let price = tokens[i].to_string();
    let category = tokens.get(i + 1).copied().unwrap_or("other").to_string();
    if name.is_empty() {
        return Intent::Unknown("need a name".into());
    }
    let id = slug(&name);
    Intent::MenuUpsert {
        draft: MenuDraft {
            id: id.clone(),
            name,
            name_zh: String::new(),
            description: String::new(),
            price,
            category,
            image: String::new(),
        },
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

fn split_items(n: &str) -> Vec<&str> {
    let mut acc = Vec::new();
    for chunk in n.split(" and ") {
        if chunk.contains('和') || chunk.contains('同') {
            for p in chunk.split(['和', '同']) {
                let p = p.trim();
                if !p.is_empty() {
                    acc.push(p);
                }
            }
        } else {
            let p = chunk.trim();
            if !p.is_empty() {
                acc.push(p);
            }
        }
    }
    if acc.is_empty() {
        vec![n]
    } else {
        acc
    }
}

fn is_cmd(n: &str, exact: &[&str], prefixes: &[&str]) -> bool {
    exact.iter().any(|k| n == *k)
        || prefixes
            .iter()
            .any(|k| n == *k || n.starts_with(&format!("{k} ")))
}

fn strip_cmd<'a>(n: &'a str, cmds: &[&str]) -> Option<&'a str> {
    for c in cmds {
        if n == *c {
            return Some("");
        }
        let p = format!("{c} ");
        if let Some(rest) = n.strip_prefix(&p) {
            return Some(rest);
        }
    }
    None
}

// Catalog is rebuilt each parse from the seed menu. Extra items the owner
// adds are matched by the server after looking up names in SQLite — the
// parser still catches the seed cafe, which is the virtual test.
pub fn aliases_from_names(id: &str, name: &str, name_zh: &str) -> Vec<String> {
    aliases_for(id, name, name_zh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guest_can_ask_for_more_test_money() {
        for text in [
            "top up",
            "topup",
            "faucet",
            "more money",
            "Top Up",
            "test money",
        ] {
            assert_eq!(parse_intent(text), Intent::Faucet, "{text}");
        }
        // And it is not confused with an order.
        assert!(matches!(parse_intent("latte"), Intent::Add { .. }));
    }

    #[test]
    fn guest_lines() {
        assert!(matches!(parse_intent("menu"), Intent::ShowMenu));
        assert!(matches!(parse_intent("pay"), Intent::Pay { .. }));
        match parse_intent("latte") {
            Intent::Add { item_id, qty } => {
                assert_eq!(item_id, "latte");
                assert_eq!(qty, 1);
            }
            other => panic!("{other:?}"),
        }
        match parse_intent("two pineapple buns") {
            Intent::Add { item_id, qty } => {
                assert_eq!(item_id, "pineapple_bun");
                assert_eq!(qty, 2);
            }
            other => panic!("{other:?}"),
        }
        match parse_intent("我要鴛鴦") {
            Intent::Add { item_id, .. } => assert_eq!(item_id, "yuenyeung"),
            other => panic!("{other:?}"),
        }
        let adds = parse_all_adds("latte and an egg tart");
        assert_eq!(adds.len(), 2);
    }

    #[test]
    fn owner_lines() {
        match parse_intent("add item mango pudding 3.20 dessert") {
            Intent::MenuUpsert { draft } => {
                assert_eq!(draft.name, "mango pudding");
                assert_eq!(draft.price, "3.20");
                assert_eq!(draft.category, "dessert");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_intent("payments"), Intent::ListPayments));
        assert!(matches!(
            parse_intent("hide macaroni"),
            Intent::MenuHide { .. }
        ));
    }

    #[test]
    fn buttons_match_chat() {
        let a = Intent::from_action(
            ActionName::Add,
            "latte".into(),
            1,
            None,
            None,
            String::new(),
            String::new(),
            String::new(),
            false,
        );
        let b = parse_intent("latte");
        assert_eq!(a, b);
        assert_eq!(
            Intent::from_action(
                ActionName::Pay,
                String::new(),
                0,
                None,
                None,
                String::new(),
                String::new(),
                String::new(),
                false,
            ),
            parse_intent("pay")
        );
    }

    #[test]
    fn order_a_latte_adds_not_cart() {
        match parse_intent("order a latte") {
            Intent::Add { item_id, qty } => {
                assert_eq!(item_id, "latte");
                assert_eq!(qty, 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn list_payments_is_not_menu() {
        assert!(matches!(
            parse_intent("list payments"),
            Intent::ListPayments
        ));
        assert!(matches!(parse_intent("show orders"), Intent::ListOrders));
        assert!(matches!(parse_intent(""), Intent::Help));
        assert!(matches!(parse_intent("blorp"), Intent::Unknown(_)));
    }

    #[test]
    fn remove_and_clear() {
        match parse_intent("remove latte") {
            Intent::Remove { item_id, qty } => {
                assert_eq!(item_id, "latte");
                assert_eq!(qty, 1);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse_intent("clear"), Intent::Clear));
        assert!(matches!(
            parse_intent("pay with wallet"),
            Intent::Pay {
                method: PayMethod::Wallet,
                ..
            }
        ));
    }

    #[test]
    fn cantonese_bakery() {
        match parse_intent("菠蘿油") {
            Intent::Add { item_id, .. } => assert_eq!(item_id, "pineapple_bun"),
            other => panic!("{other:?}"),
        }
        match parse_intent("三 蛋撻") {
            Intent::Add { item_id, qty } => {
                assert_eq!(item_id, "egg_tart");
                assert_eq!(qty, 3);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn from_client_chat_and_action() {
        let chat = ClientMsg::Chat {
            text: "menu".into(),
        };
        assert_eq!(Intent::from_client(&chat), Some(Intent::ShowMenu));
        assert_eq!(Intent::from_client(&ClientMsg::Ping), None);
    }
}
