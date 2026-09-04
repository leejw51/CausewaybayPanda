//! JSON WebSocket at `/ws`. Login, then chat or action frames.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::State;
use axum::response::IntoResponse;
use causewaybay_panda_protocol::wire::{ClientMsg, PayMethod, ServerMsg};
use causewaybay_panda_protocol::{Intent, Role, CAFE_NAME, CAFE_NAME_ZH, CHAIN_ID};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::cafe;
use crate::db::SessionRow;
use crate::settlement::is_tx_hash;
use crate::verify::{self, Verdict};
use crate::AppState;

pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle(socket, state))
}

async fn handle(socket: WebSocket, state: Arc<AppState>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();

    async fn push(sink: &mut futures_util::stream::SplitSink<WebSocket, Message>, msg: &ServerMsg) {
        if let Ok(text) = serde_json::to_string(msg) {
            let _ = sink.send(Message::Text(text.into())).await;
        }
    }

    let mut session: Option<SessionRow> = None;

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break; };
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Ping(p) => {
                        let _ = sink.send(Message::Pong(p)).await;
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };
                let parsed: ClientMsg = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                match parsed {
                    ClientMsg::Ping => {
                        push(&mut sink, &ServerMsg::Pong).await;
                    }
                    ClientMsg::Login { role, name, pin } => {
                        match login(&state, role, &name, &pin) {
                            Ok(row) => {
                                match row.role {
                                    Role::Guest => state.hub.register_guest(row.id.clone(), tx.clone()),
                                    Role::Owner => state.hub.register_owner(row.id.clone(), tx.clone()),
                                }
                                let welcome = welcome(&state, &row);
                                session = Some(row);
                                push(&mut sink, &welcome).await;
                                let s = session.as_ref().unwrap();
                                let applied = cafe::apply(&state.db, &state.shop, s, Intent::ShowMenu);
                                for m in applied.to_self {
                                    push(&mut sink, &m).await;
                                }
                                let applied = cafe::apply(&state.db, &state.shop, s, Intent::Help);
                                // A guest needs their purse and cart from the
                                // first frame: the faucet is only offered once
                                // the page knows what they are holding.
                                if s.role == Role::Guest {
                                    let cart =
                                        cafe::apply(&state.db, &state.shop, s, Intent::ShowCart);
                                    for m in cart.to_self {
                                        push(&mut sink, &m).await;
                                    }
                                }
                                for m in applied.to_self {
                                    push(&mut sink, &m).await;
                                }
                            }
                            Err(message) => {
                                push(&mut sink, &ServerMsg::Error { message }).await;
                            }
                        }
                    }
                    other => {
                        let Some(s) = session.as_ref() else {
                            push(&mut sink, &ServerMsg::Error {
                                message: "login first".into(),
                            }).await;
                            continue;
                        };
                        let intents = resolve_intents(&state, s, &other).await;
                        for intent in intents {
                            // A wallet hash is a claim until the chain agrees.
                            let intent = match intent {
                                Intent::Pay {
                                    method: PayMethod::Wallet,
                                    tx_hash,
                                } if !tx_hash.trim().is_empty() => {
                                    let tx_hash = tx_hash.trim().to_lowercase();
                                    push(&mut sink, &ServerMsg::Assistant {
                                        text: format!("Checking {} for your payment…", state.shop.settle.chain.name),
                                        buttons: Vec::new(),
                                    }).await;
                                    match confirm_on_chain(&state, s, &tx_hash).await {
                                        Ok(()) => Intent::Pay {
                                            method: PayMethod::Wallet,
                                            tx_hash,
                                        },
                                        Err(message) => {
                                            push(&mut sink, &ServerMsg::Error { message }).await;
                                            continue;
                                        }
                                    }
                                }
                                other => other,
                            };
                            let applied = cafe::apply(&state.db, &state.shop, s, intent);
                            for m in &applied.to_owners {
                                state.hub.to_owners(m.clone());
                            }
                            for m in &applied.to_guests {
                                state.hub.to_guests(m.clone());
                            }
                            for (sid, m) in &applied.to_session {
                                state.hub.to_session(sid, m.clone());
                            }
                            for m in applied.to_self {
                                push(&mut sink, &m).await;
                            }
                        }
                    }
                }
            }
            Some(broadcast) = rx.recv() => {
                push(&mut sink, &broadcast).await;
            }
        }
    }

    if let Some(s) = session {
        state.hub.drop_session(&s.id);
    }
}

/// Read the receipt for `tx_hash` from the chain and hold it against the
/// guest's cart as it stands now. Ok means the till may book the order.
async fn confirm_on_chain(
    state: &AppState,
    session: &SessionRow,
    tx_hash: &str,
) -> Result<(), String> {
    let settle = &state.shop.settle;
    if !state.shop.onchain() {
        return Err(format!(
            "wallet payment is off: {}",
            state.shop.onchain_reason()
        ));
    }
    if !is_tx_hash(tx_hash) {
        return Err("that is not a transaction hash".into());
    }
    let (lines, total, _) = state.db.cart(&session.id)?;
    if lines.is_empty() {
        return Err("cart is empty".into());
    }
    // The bill is the cart now, not whatever was quoted earlier: adding a
    // dish after signing does not get it for free.
    let verdict = verify::confirm(
        settle.rpc_url(),
        tx_hash,
        settle.token_address(),
        settle.treasury_address(),
        settle.atomic(total),
        state.receipt_patience,
    )
    .await;
    match verdict {
        Verdict::Paid(_) => Ok(()),
        Verdict::Pending => Err(format!(
            "{} has not confirmed that transaction yet — give it a moment and tap pay again",
            settle.chain.name
        )),
        Verdict::Rejected(why) => Err(format!("payment not accepted: {why}")),
        Verdict::Underpaid { paid, needed } => Err(format!(
            "payment not accepted: that transaction paid {} but the bill is {}",
            state.shop.price(verify::as_micro(paid)),
            state.shop.price(verify::as_micro(needed))
        )),
    }
}

fn login(state: &AppState, role: Role, name: &str, pin: &str) -> Result<SessionRow, String> {
    match role {
        Role::Guest => state.db.create_session(Role::Guest, name),
        Role::Owner => {
            if !state.db.check_pin(pin)? {
                return Err("wrong pin".into());
            }
            state.db.create_session(Role::Owner, name)
        }
    }
}

fn welcome(state: &AppState, row: &SessionRow) -> ServerMsg {
    ServerMsg::Welcome {
        role: row.role,
        name: row.name.clone(),
        cafe: CAFE_NAME.into(),
        cafe_zh: CAFE_NAME_ZH.into(),
        treasury: state.shop.settle.treasury_address().to_string(),
        chain_id: state.shop.settle.chain.chain_id,
        balance_usdc: causewaybay_panda_protocol::format_usdc(row.balance_micro),
        balance_display: state.shop.price(row.balance_micro),
        ai: state.ai.as_ref().map(|a| a.describe()).unwrap_or_default(),
        settlement: state.shop.wire(),
        // A reload should not lose the order somebody is waiting on.
        orders: match row.role {
            Role::Guest => state.db.orders_for_session(&row.id).unwrap_or_default(),
            Role::Owner => state.db.open_orders().unwrap_or_default(),
        },
    }
}

async fn resolve_intents(state: &AppState, session: &SessionRow, msg: &ClientMsg) -> Vec<Intent> {
    match msg {
        ClientMsg::Chat { text } => {
            let mut intents = cafe::intents_for_chat(&state.db, text);
            let unknown = matches!(intents.first(), Some(Intent::Unknown(_)));
            if unknown {
                if let Some(ai) = &state.ai {
                    // Give the model the whole board, not bare ids: it has to
                    // pick an id, and it writes the Chinese name for new dishes.
                    let board: Vec<String> = state
                        .db
                        .menu()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|i| {
                            let on = if i.available { "on" } else { "off" };
                            format!(
                                "{} — {} / {} — {} — {on}",
                                i.id, i.name, i.name_zh, i.price_display
                            )
                        })
                        .collect();
                    let role = match session.role {
                        Role::Owner => "owner",
                        Role::Guest => "guest",
                    };
                    if let Some(better) = ai.interpret(text, &board, role).await {
                        intents = vec![better];
                    }
                }
            }
            intents
        }
        ClientMsg::Action {
            name,
            item_id,
            qty,
            method,
            item,
            tx_hash,
            order_id,
            status,
        } => {
            vec![Intent::from_action(
                *name,
                item_id.clone(),
                *qty,
                *method,
                item.clone(),
                tx_hash.clone(),
                order_id.clone(),
                status.clone(),
            )]
        }
        _ => Vec::new(),
    }
}
