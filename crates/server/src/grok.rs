//! Optional Grok NLU. The local parser always runs; this maps a model JSON
//! blob onto the same `Intent` so tests do not need a live key.

use causewaybay_panda_protocol::intent::Intent;
use causewaybay_panda_protocol::wire::{MenuDraft, PayMethod};
use serde_json::Value;

/// Pull the first JSON object out of a model reply (raw or fenced) and turn it
/// into an intent. `None` means "fall back to the local parser".
pub fn intent_from_model_json(raw: &str) -> Option<Intent> {
    let json = extract_json(raw)?;
    let v: Value = serde_json::from_str(&json).ok()?;
    let intent = v.get("intent")?.as_str()?.to_lowercase();
    match intent.as_str() {
        "help" => Some(Intent::Help),
        "menu" | "show_menu" => Some(Intent::ShowMenu),
        "cart" | "show_cart" => Some(Intent::ShowCart),
        "clear" => Some(Intent::Clear),
        "pay" => {
            let method = match v.get("method").and_then(|m| m.as_str()).unwrap_or("usdc") {
                "wallet" => PayMethod::Wallet,
                _ => PayMethod::Usdc,
            };
            Some(Intent::Pay {
                method,
                tx_hash: String::new(),
            })
        }
        "add" => {
            let item_id = v.get("item_id")?.as_str()?.to_string();
            let qty = v.get("qty").and_then(|q| q.as_u64()).unwrap_or(1) as u32;
            Some(Intent::Add {
                item_id,
                qty: qty.max(1),
            })
        }
        "remove" => Some(Intent::Remove {
            item_id: v.get("item_id")?.as_str()?.to_string(),
            qty: v.get("qty").and_then(|q| q.as_u64()).unwrap_or(1) as u32,
        }),
        "payments" | "list_payments" => Some(Intent::ListPayments),
        "orders" | "list_orders" => Some(Intent::ListOrders),
        "menu_upsert" | "add_item" => {
            let draft = MenuDraft {
                id: v
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: v.get("name")?.as_str()?.to_string(),
                name_zh: v
                    .get("name_zh")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: String::new(),
                price: v.get("price")?.as_str()?.to_string(),
                category: v
                    .get("category")
                    .and_then(|x| x.as_str())
                    .unwrap_or("other")
                    .to_string(),
                image: String::new(),
            };
            Some(Intent::MenuUpsert { draft })
        }
        "menu_hide" | "hide" => Some(Intent::MenuHide {
            item_id: v.get("item_id")?.as_str()?.to_string(),
        }),
        "menu_show" | "show" | "unhide" => Some(Intent::MenuShow {
            item_id: v.get("item_id")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

fn extract_json(raw: &str) -> Option<String> {
    let t = raw.trim();
    if let Some(start) = t.find('{') {
        if let Some(end) = t.rfind('}') {
            if end >= start {
                return Some(t[start..=end].to_string());
            }
        }
    }
    None
}

/// Live call. Failures return `None` so the websocket path keeps the parser.
/// `menu` is one `id — name` line per dish; `role` is "guest" or "owner", so
/// the model does not read an owner's "hide the macaroni" as an order.
pub async fn interpret(api_key: &str, text: &str, menu: &[String], role: &str) -> Option<Intent> {
    let verbs = if role == "owner" {
        "help, menu, payments, orders, add_item, hide, show"
    } else {
        "help, menu, cart, add, remove, pay, clear"
    };
    let body = serde_json::json!({
        "model": std::env::var("GROK_TEXT_MODEL").unwrap_or_else(|_| "grok-4-fast".into()),
        "temperature": 0,
        "messages": [
            {
                "role": "system",
                "content": format!(
                    "You turn Causewaybay Coffee chat into one JSON object, nothing else.\n\
                     The speaker is the {role}. Allowed intent values: {verbs}.\n\
                     Keys: intent, item_id (an id from the menu below), qty, \
                     method (usdc|wallet), name, name_zh, price, category.\n\
                     For add_item always fill name, price, category, and name_zh \
                     with Traditional Chinese for the dish.\n\
                     Menu:\n{}",
                    menu.join("\n")
                )
            },
            { "role": "user", "content": text }
        ]
    });
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.x.ai/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .ok()?;
    let v: Value = resp.json().await.ok()?;
    let content = v["choices"][0]["message"]["content"].as_str()?;
    intent_from_model_json(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_add() {
        let raw = "```json\n{\"intent\":\"add\",\"item_id\":\"latte\",\"qty\":2}\n```";
        match intent_from_model_json(raw) {
            Some(Intent::Add { item_id, qty }) => {
                assert_eq!(item_id, "latte");
                assert_eq!(qty, 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_pay_and_owner() {
        assert!(matches!(
            intent_from_model_json("{\"intent\":\"pay\",\"method\":\"wallet\"}"),
            Some(Intent::Pay {
                method: PayMethod::Wallet,
                ..
            })
        ));
        match intent_from_model_json(
            "{\"intent\":\"add_item\",\"name\":\"mango pudding\",\"price\":\"3.20\",\"category\":\"dessert\"}",
        ) {
            Some(Intent::MenuUpsert { draft }) => {
                assert_eq!(draft.name, "mango pudding");
                assert_eq!(draft.price, "3.20");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn garbage_is_none() {
        assert!(intent_from_model_json("sorry I cannot").is_none());
        assert!(intent_from_model_json("{\"intent\":\"nope\"}").is_none());
    }
}
