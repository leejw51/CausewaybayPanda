//! JSON WebSocket at `/ws`. Login, then chat or action frames.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::State;
use axum::response::IntoResponse;
use causewaybay_panda_protocol::wire::{ClientMsg, PayMethod, ServerMsg};
use causewaybay_panda_protocol::{Intent, Role};
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
                    ClientMsg::Login {
                        role,
                        name,
                        pin,
                        session: held,
                    } => match cafe::login(&state.db, &state.shop, role, &name, &pin, &held) {
                        Ok(row) => {
                            match row.role {
                                Role::Guest => state.hub.register_guest(row.id.clone(), tx.clone()),
                                Role::Owner => state.hub.register_owner(row.id.clone(), tx.clone()),
                            }
                            let ai = state.ai.read().as_ref().map(|a| a.describe()).unwrap_or_default();
                            let demo_on = state.demo.lock().on;
                            for m in cafe::on_login(&state.db, &state.shop, &row, &ai, demo_on) {
                                push(&mut sink, &m).await;
                            }
                            if row.role == Role::Owner {
                                push(&mut sink, &ai_status(&state)).await;
                            }
                            session = Some(row);
                        }
                        Err(message) => {
                            push(&mut sink, &ServerMsg::Error { message }).await;
                        }
                    },
                    ClientMsg::AiSetup {
                        provider,
                        key,
                        model,
                    } => {
                        let Some(s) = session.as_ref() else {
                            push(&mut sink, &ServerMsg::Error { message: "login first".into() }).await;
                            continue;
                        };
                        if s.role != Role::Owner {
                            push(&mut sink, &ServerMsg::Error {
                                message: "only the owner chooses who listens".into(),
                            }).await;
                            continue;
                        }
                        match set_ai(&state, &provider, &key, &model) {
                            // Every owner, this one included, hears it through
                            // the hub; a direct push here would say it twice.
                            Ok(()) => state.hub.to_owners(ai_status(&state)),
                            Err(message) => push(&mut sink, &ServerMsg::Error { message }).await,
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
                            // The auto switch belongs to the host: it owns the
                            // timer. The cafe only checks who is asking.
                            if let Intent::Auto { on } = intent {
                                if s.role != Role::Owner {
                                    push(&mut sink, &ServerMsg::Error {
                                        message: "only the owner can set the cafe running on its own".into(),
                                    }).await;
                                    continue;
                                }
                                let switched = state.demo.lock().set(on, &state.shop);
                                match switched {
                                    Ok(now_on) => {
                                        if now_on {
                                            spawn_demo(state.clone());
                                        }
                                        state.hub.to_owners(ServerMsg::Auto { on: now_on });
                                        push(&mut sink, &ServerMsg::Assistant {
                                            text: if now_on {
                                                "The cafe is running on its own. Watch the counter.".into()
                                            } else {
                                                "Stopped. The counter is yours again.".into()
                                            },
                                            buttons: Vec::new(),
                                        }).await;
                                    }
                                    Err(message) => push(&mut sink, &ServerMsg::Error { message }).await,
                                }
                                continue;
                            }
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

/// Who is listening, for the owner's form. The key never leaves the server.
fn ai_status(state: &AppState) -> ServerMsg {
    let (provider, model, ready) = crate::ai::Ai::status(state.ai.read().as_ref());
    ServerMsg::AiStatus {
        provider,
        model,
        ready,
        providers: crate::ai::Provider::infos(),
    }
}

/// The owner's choice from the counter, kept in the shop and applied at once.
/// An empty key keeps the one already held; "off" hands the chat back to the
/// local parser.
fn set_ai(state: &AppState, provider: &str, key: &str, model: &str) -> Result<(), String> {
    use crate::ai::{Ai, Provider, SETTING_KEY, SETTING_MODEL, SETTING_PROVIDER};
    use causewaybay_panda_core::Store;
    let provider = provider.trim().to_lowercase();
    if provider == "off" || provider.is_empty() {
        state.db.set_setting(SETTING_PROVIDER, Some("off"))?;
        *state.ai.write() = None;
        return Ok(());
    }
    let p = Provider::from_key(&provider).ok_or_else(|| format!("no such provider: {provider}"))?;
    let key = if key.trim().is_empty() {
        state.db.setting(SETTING_KEY)?.unwrap_or_default()
    } else {
        key.trim().to_string()
    };
    if p.needs_key() && key.is_empty() {
        return Err(format!("{} needs an API key", p.label()));
    }
    let ai = Ai::from_settings(Some(p.key()), Some(&key), Some(model))
        .ok_or_else(|| format!("{} could not be set up", p.label()))?;
    state.db.set_setting(SETTING_PROVIDER, Some(p.key()))?;
    state
        .db
        .set_setting(SETTING_KEY, if key.is_empty() { None } else { Some(&key) })?;
    state.db.set_setting(
        SETTING_MODEL,
        if model.trim().is_empty() {
            None
        } else {
            Some(model.trim())
        },
    )?;
    *state.ai.write() = Some(ai);
    Ok(())
}

/// The cafe running itself: a timer that takes one step and fans the frames
/// out to whoever is connected. Stops itself when the switch is thrown.
fn spawn_demo(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(state.demo_tick).await;
            let applies = {
                let mut demo = state.demo.lock();
                if !demo.on {
                    break;
                }
                demo.tick(&state.db, &state.shop)
            };
            for a in applies {
                for m in a.to_owners {
                    state.hub.to_owners(m);
                }
                for m in a.to_guests {
                    state.hub.to_guests(m);
                }
                for (sid, m) in a.to_session {
                    state.hub.to_session(&sid, m);
                }
                // to_self belongs to a simulated guest with no socket.
            }
        }
    });
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

async fn resolve_intents(state: &AppState, session: &SessionRow, msg: &ClientMsg) -> Vec<Intent> {
    match msg {
        ClientMsg::Chat { text } => {
            let mut intents = cafe::intents_for_chat(&state.db, text);
            let unknown = matches!(intents.first(), Some(Intent::Unknown(_)));
            if unknown {
                let listening = state.ai.read().clone();
                if let Some(ai) = listening.as_ref() {
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
            on,
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
                *on,
            )]
        }
        _ => Vec::new(),
    }
}
