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

/// Open the socket — but only for a page this shop served. A browser sends
/// the page's Origin with the upgrade; a web page open on some other site, on
/// any device on the cafe wifi, could otherwise drive the till (and in a
/// simulation, open the counter). Clients that send no Origin are not
/// browsers and are let through.
pub async fn upgrade(
    ws: WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    State(state): State<Arc<AppState>>,
) -> axum::response::Response {
    if !same_origin(&headers) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "this shop only talks to its own page",
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| handle(socket, state))
        .into_response()
}

/// True when there is no Origin, or its host:port is the one this request
/// was addressed to.
fn same_origin(headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let origin_host = origin
        .trim()
        .split("://")
        .nth(1)
        .unwrap_or("")
        .trim_end_matches('/');
    !origin_host.is_empty() && origin_host.eq_ignore_ascii_case(host.trim())
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
                    } => match cafe::login(&state.db, &state.shop(), role, &name, &pin, &held) {
                        Ok(row) => {
                            let shop = state.shop();
                            match row.role {
                                Role::Guest => state.hub.register_guest(row.id.clone(), tx.clone()),
                                Role::Owner => state.hub.register_owner(row.id.clone(), tx.clone()),
                            }
                            let ai = state.ai.read().as_ref().map(|a| a.describe()).unwrap_or_default();
                            let demo_on = state.demo.lock().on;
                            let kitchen_on = state.kitchen.lock().on;
                            for m in cafe::on_login(&state.db, &shop, &row, &ai, demo_on, kitchen_on) {
                                push(&mut sink, &m).await;
                            }
                            if row.role == Role::Owner {
                                push(&mut sink, &ai_status(&state)).await;
                                push(&mut sink, &causewaybay_panda_core::setup::setup_msg(&state.db, &shop)).await;
                                // What the treasury holds, read from the chain
                                // as the answer comes back, not before.
                                spawn_treasury(state.clone(), Some(row.id.clone()));
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
                    ClientMsg::Reset { pin } => {
                        // From the door, so nobody is logged in. A live shop
                        // is real money and real books: the owner pin. A
                        // simulation is anyone's to wipe, as it is to open.
                        let shop = state.shop();
                        let allowed = shop.is_simulation()
                            || state.db.check_pin(&pin).unwrap_or(false);
                        if !allowed {
                            push(&mut sink, &ServerMsg::Error { message: "wrong pin".into() }).await;
                            continue;
                        }
                        match wipe(&state) {
                            Ok(()) => {
                                // Every open page, this one included, starts
                                // again at the door.
                                state.hub.to_owners(ServerMsg::Reset);
                                state.hub.to_guests(ServerMsg::Reset);
                                push(&mut sink, &ServerMsg::Reset).await;
                            }
                            Err(message) => push(&mut sink, &ServerMsg::Error { message }).await,
                        }
                    }
                    ClientMsg::Setup { setup, pin } => {
                        let Some(s) = session.as_ref() else {
                            push(&mut sink, &ServerMsg::Error { message: "login first".into() }).await;
                            continue;
                        };
                        if s.role != Role::Owner {
                            push(&mut sink, &ServerMsg::Error {
                                message: "only the owner changes the shop".into(),
                            }).await;
                            continue;
                        }
                        match apply_setup(&state, &setup, &pin) {
                            Ok(said) => {
                                refresh_everyone(&state);
                                push(&mut sink, &ServerMsg::Assistant { text: said, buttons: Vec::new() }).await;
                            }
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
                                // Decide and claim the ticker under one lock, so
                                // two owners pressing at once still get one beat.
                                let switched = {
                                    let mut demo = state.demo.lock();
                                    demo.set(on, &state.shop()).map(|now_on| {
                                        let start = now_on && !demo.ticking;
                                        if start {
                                            demo.ticking = true;
                                        }
                                        (now_on, start)
                                    })
                                };
                                match switched {
                                    Ok((now_on, start)) => {
                                        if start {
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
                            // The kitchen switch is the host's too, for the
                            // same reason: it owns the beat.
                            if let Intent::Kitchen { on } = intent {
                                if s.role != Role::Owner {
                                    push(&mut sink, &ServerMsg::Error {
                                        message: "only the owner decides who works the kitchen".into(),
                                    }).await;
                                    continue;
                                }
                                let switched = {
                                    let mut k = state.kitchen.lock();
                                    k.set(on, &state.db).map(|now_on| {
                                        let start = now_on && !k.ticking;
                                        if start {
                                            k.ticking = true;
                                        }
                                        (now_on, start)
                                    })
                                };
                                match switched {
                                    Ok((now_on, start)) => {
                                        if start {
                                            spawn_kitchen(state.clone());
                                        }
                                        state.hub.to_owners(ServerMsg::Kitchen { on: now_on });
                                        push(&mut sink, &ServerMsg::Assistant {
                                            text: if now_on {
                                                "The panda is working the kitchen: tickets will be picked up and called ready on their own. Handing over is still yours.".into()
                                            } else {
                                                "The kitchen is yours again.".into()
                                            },
                                            buttons: Vec::new(),
                                        }).await;
                                    }
                                    Err(message) => push(&mut sink, &ServerMsg::Error { message }).await,
                                }
                                continue;
                            }
                            // A wallet hash is a claim until the chain agrees.
                            let mut landed_on_chain = false;
                            let intent = match intent {
                                Intent::Pay {
                                    method: PayMethod::Wallet,
                                    tx_hash,
                                } if !tx_hash.trim().is_empty() => {
                                    let tx_hash = tx_hash.trim().to_lowercase();
                                    push(&mut sink, &ServerMsg::Assistant {
                                        text: format!("Checking {} for your payment…", state.shop().settle.chain.name),
                                        buttons: Vec::new(),
                                    }).await;
                                    match confirm_on_chain(&state, s, &tx_hash).await {
                                        Ok(()) => {
                                            landed_on_chain = true;
                                            Intent::Pay {
                                                method: PayMethod::Wallet,
                                                tx_hash,
                                            }
                                        }
                                        Err(message) => {
                                            push(&mut sink, &ServerMsg::Error { message }).await;
                                            continue;
                                        }
                                    }
                                }
                                other => other,
                            };
                            let applied = cafe::apply(&state.db, &state.shop(), s, intent);
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
                            // Real USDC just reached the treasury: every
                            // counter reads the new balance off the chain.
                            if landed_on_chain {
                                spawn_treasury(state.clone(), None);
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
    // A key that arrived through the environment is held too: keep it in the
    // shop's settings before switching off, so it can be taken up again.
    let active = state.ai.read().clone();
    if let Some(a) = &active {
        if state.db.setting(SETTING_KEY)?.is_none() && !a.key().is_empty() {
            state.db.set_setting(SETTING_KEY, Some(a.key()))?;
            state
                .db
                .set_setting(SETTING_PROVIDER, Some(a.provider.key()))?;
        }
    }
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
                    // The switch is off: this ticker ends, and the next "on"
                    // is free to start a new one.
                    demo.ticking = false;
                    break;
                }
                demo.tick(&state.db, &state.shop())
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

/// The shop back to a fresh install: the store wiped and reseeded, and
/// everything held in memory over it — the running shop, who listens, the
/// two switches — read again from the environment as at first boot.
fn wipe(state: &AppState) -> Result<(), String> {
    use causewaybay_panda_core::Store;
    state.db.reset()?;
    let shop =
        causewaybay_panda_core::setup::resolve_shop(&state.db, &state.env_shop, &state.env_settle);
    state.db.set_denom(shop.denom.clone());
    *state.shop.write() = shop;
    *state.ai.write() = crate::ai::Ai::from_env();
    // A running ticker sees its switch off on its next beat and ends.
    state.demo.lock().on = false;
    state.kitchen.lock().on = false;
    Ok(())
}

/// The owner's new setup: checked, kept, and made the running shop. The
/// pin, when one is given, is changed with it.
fn apply_setup(
    state: &AppState,
    setup: &causewaybay_panda_protocol::wire::Setup,
    pin: &str,
) -> Result<String, String> {
    use causewaybay_panda_core::Store;
    let pin = pin.trim();
    if !pin.is_empty() && pin.chars().count() < 4 {
        return Err("a pin is at least four characters".into());
    }
    let shop =
        causewaybay_panda_core::setup::apply(&state.db, setup, &state.env_shop, &state.env_settle)?;
    state.db.set_denom(shop.denom.clone());
    let live = !shop.is_simulation();
    *state.shop.write() = shop;
    let mut said = if live {
        "The shop is live: real USDC, and the pin locks the till.".to_string()
    } else {
        "The shop is set up. It is a simulation: test money, any pin.".to_string()
    };
    if !pin.is_empty() {
        state.db.set_pin(pin)?;
        said.push_str(" The pin is changed.");
    }
    Ok(said)
}

/// The shop changed: every open page gets the new shop, the board priced in
/// its money, and — each to their own — a fresh cart or fresh figures.
fn refresh_everyone(state: &AppState) {
    use causewaybay_panda_core::setup;
    let shop = state.shop();
    let all = setup::shop_msg(&state.db, &shop);
    state.hub.to_guests(all.clone());
    state.hub.to_owners(all);
    if let Ok(items) = state.db.menu_available() {
        state.hub.to_guests(ServerMsg::Menu { items });
    }
    if let Ok(items) = state.db.menu() {
        state.hub.to_owners(ServerMsg::Menu { items });
    }
    state.hub.to_owners(setup::setup_msg(&state.db, &shop));
    state.hub.to_owners(cafe::takings_msg(&state.db, &shop));
    state.hub.to_owners(cafe::dashboard_msg(&state.db, &shop));
    for (sid, role) in state.hub.sessions() {
        let Ok(Some(row)) = state.db.session(&sid) else {
            continue;
        };
        match role {
            Role::Guest => {
                for m in cafe::apply(&state.db, &shop, &row, Intent::ShowCart).to_self {
                    state.hub.to_session(&sid, m);
                }
                state
                    .hub
                    .to_session(&sid, cafe::guest_dashboard_msg(&state.db, &shop, &sid));
            }
            Role::Owner => {}
        }
    }
    spawn_treasury(Arc::new(state.clone()), None);
}

/// Read what the treasury holds and hand it to one owner, or to every owner
/// when `to` is `None`. A simulation has no treasury on any chain and sends
/// nothing; a chain that does not answer sends nothing either — the counter
/// keeps its last reading rather than showing a zero that is not true.
fn spawn_treasury(state: Arc<AppState>, to: Option<String>) {
    let shop = state.shop();
    if !shop.onchain() {
        return;
    }
    tokio::spawn(async move {
        let settle = &shop.settle;
        let call = settle.treasury_balance_call();
        let Ok(atomic) = verify::balance_of(settle.rpc_url(), settle.token_address(), &call).await
        else {
            return;
        };
        let micro = settle.micro_from_atomic(atomic);
        let msg = ServerMsg::Treasury {
            address: settle.treasury_address().to_string(),
            chain_name: settle.chain.name.to_string(),
            token: settle.token_address().to_string(),
            usdc: causewaybay_panda_protocol::money::format_usdc(micro),
            display: shop.price(micro),
            explorer_url: settle.treasury_url(),
        };
        match to {
            Some(sid) => state.hub.to_session(&sid, msg),
            None => state.hub.to_owners(msg),
        }
    });
}

/// The panda at the pass: a timer that looks at the queue and moves what is
/// due, fanning the frames out to whoever is connected.
fn spawn_kitchen(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(state.kitchen_tick).await;
            let applies = {
                let mut k = state.kitchen.lock();
                if !k.on {
                    k.ticking = false;
                    break;
                }
                k.tick(&state.db, &state.shop())
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
            }
        }
    });
}

/// At boot: if the shop was left with the panda working the kitchen, it
/// picks the tongs straight back up.
pub fn resume_kitchen(state: Arc<AppState>) {
    let start = {
        let mut k = state.kitchen.lock();
        if k.on && !k.ticking {
            k.ticking = true;
            true
        } else {
            false
        }
    };
    if start {
        spawn_kitchen(state);
    }
}

/// Read the receipt for `tx_hash` from the chain and hold it against the
/// guest's cart as it stands now. Ok means the till may book the order.
async fn confirm_on_chain(
    state: &AppState,
    session: &SessionRow,
    tx_hash: &str,
) -> Result<(), String> {
    let shop = state.shop();
    let settle = &shop.settle;
    if !shop.onchain() {
        return Err(format!("wallet payment is off: {}", shop.onchain_reason()));
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
            shop.price(verify::as_micro(paid)),
            shop.price(verify::as_micro(needed))
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
                    let facts = cafe::facts_for(&state.db, &state.shop(), session);
                    if let Some(better) = ai.interpret(text, &board, role, &facts).await {
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
