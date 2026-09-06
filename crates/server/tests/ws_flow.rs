//! WebSocket guest → order → pay, owner sees the payment.

use std::net::SocketAddr;
use std::time::Duration;

use std::sync::Arc;

use causewaybay_panda_protocol::wire::{ActionName, ClientMsg, PayMethod, Role, ServerMsg};
use causewaybay_panda_server::settlement::{Config, Settle};
use causewaybay_panda_server::shop::{Config as ShopConfig, Shop};
use causewaybay_panda_server::{router, AppState};

/// A real-looking payee, never the seeded 0xC0FFEE… placeholder.
const TREASURY: &str = "0x1111111111111111111111111111111111111111";
const USDC: &str = "0xc21223249CA28397B4B6541dfFaEcC539BfF0c59";

/// Hashes the mock chain knows. The first byte says what happened to them.
fn hash(prefix: &str) -> String {
    format!("0x{prefix}{}", "ab".repeat(31))
}
const PAID: &str = "aa"; // mined, paid the treasury 100 USDC
const PENDING: &str = "bb"; // never seen
const REVERTED: &str = "cc"; // mined, status 0
const ELSEWHERE: &str = "dd"; // mined, paid someone else
const SHORT: &str = "ee"; // mined, paid 1 USDC

fn topic(addr: &str) -> String {
    format!("0x{}{}", "0".repeat(24), addr[2..].to_lowercase())
}

/// What the mock chain says the treasury holds: 250 USDC.
const TREASURY_ATOMIC: u128 = 250_000_000;

/// The two JSON-RPC methods the till ever calls: a receipt, and the
/// treasury's balance.
async fn mock_rpc(axum::Json(req): axum::Json<serde_json::Value>) -> axum::Json<serde_json::Value> {
    use causewaybay_panda_server::verify::TRANSFER_TOPIC;
    if req["method"] == "eth_call" {
        let data = req["params"][0]["data"].as_str().unwrap_or("");
        assert!(data.starts_with("0x70a08231"), "balanceOf, got {data}");
        assert!(data.ends_with(&TREASURY[2..].to_lowercase()));
        return axum::Json(serde_json::json!({
            "jsonrpc": "2.0", "id": req["id"], "result": format!("0x{TREASURY_ATOMIC:064x}")
        }));
    }
    assert_eq!(req["method"], "eth_getTransactionReceipt");
    let h = req["params"][0].as_str().unwrap_or("").to_lowercase();
    let receipt = |status: &str, to: &str, amount: u128| {
        serde_json::json!({
            "status": status,
            "blockNumber": "0x10",
            "to": USDC,
            "logs": [{
                "address": USDC,
                "topics": [TRANSFER_TOPIC, topic("0x3333333333333333333333333333333333333333"), topic(to)],
                "data": format!("0x{amount:064x}"),
            }],
        })
    };
    let result = match &h[2..4] {
        PAID => receipt("0x1", TREASURY, 100_000_000),
        REVERTED => receipt("0x0", TREASURY, 100_000_000),
        ELSEWHERE => receipt(
            "0x1",
            "0x4444444444444444444444444444444444444444",
            100_000_000,
        ),
        SHORT => receipt("0x1", TREASURY, 1_000_000),
        _ => serde_json::Value::Null,
    };
    axum::Json(serde_json::json!({ "jsonrpc": "2.0", "id": req["id"], "result": result }))
}

async fn spawn_mock_rpc() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().route("/", axum::routing::post(mock_rpc));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/")
}
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn health_ok() {
    let addr = spawn().await;
    let body = reqwest::get(format!("http://{addr}/health"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("\"ok\":true"), "{body}");
    assert!(body.contains("Causewaybay Coffee"), "{body}");
}

#[tokio::test]
async fn guest_chats_and_pays_owner_sees_payment() {
    let addr = spawn().await;

    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    let welcome = recv_type(&mut guest, "welcome").await;
    match welcome {
        ServerMsg::Welcome {
            role, name, cafe, ..
        } => {
            assert_eq!(role, Role::Guest);
            assert_eq!(name, "Mei");
            assert_eq!(cafe, "Causewaybay Coffee");
        }
        other => panic!("{other:?}"),
    }

    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "two lattes".into(),
        },
    )
    .await;
    let cart = recv_cart_with_lines(&mut guest, 1).await;
    match cart {
        ServerMsg::Cart {
            lines, total_usdc, ..
        } => {
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].item_id, "latte");
            assert_eq!(lines[0].qty, 2);
            assert_eq!(total_usdc, "9.74359");
        }
        other => panic!("{other:?}"),
    }

    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;

    send(&mut guest, &ClientMsg::Chat { text: "pay".into() }).await;
    let paid = recv_type(&mut guest, "paid").await;
    match paid {
        ServerMsg::Paid {
            amount_usdc,
            method,
            call_data,
            ..
        } => {
            assert_eq!(amount_usdc, "9.74359");
            assert!(matches!(
                method,
                causewaybay_panda_protocol::PayMethod::Coin
            ));
            // Play money moves no tokens, so there is no transfer to show.
            assert_eq!(call_data, "", "a demo till must not hand out calldata");
        }
        other => panic!("{other:?}"),
    }

    let payments = recv_type(&mut owner, "payments").await;
    match payments {
        ServerMsg::Payments { payments } => {
            assert_eq!(payments.len(), 1);
            assert_eq!(payments[0].guest, "Mei");
            assert_eq!(payments[0].amount_usdc, "9.74359");
        }
        other => panic!("{other:?}"),
    }
}

/// A simulation has no till worth locking: any pin opens the counter, so a
/// demo is never stuck at the door.
#[tokio::test]
async fn in_a_simulation_any_pin_opens_the_counter() {
    let addr = spawn().await;
    for pin in ["nope", ""] {
        let mut ws = connect(addr).await;
        send(
            &mut ws,
            &ClientMsg::Login {
                role: Role::Owner,
                name: "Wing".into(),
                pin: pin.into(),
                session: String::new(),
            },
        )
        .await;
        match recv_type(&mut ws, "welcome").await {
            ServerMsg::Welcome { role, .. } => assert_eq!(role, Role::Owner, "pin {pin:?}"),
            other => panic!("{other:?}"),
        }
    }
}

/// A live shop is real money; the pin is the lock on it.
#[tokio::test]
async fn a_live_shop_refuses_a_wrong_pin() {
    let addr = spawn_onchain().await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "nope".into(),
            session: String::new(),
        },
    )
    .await;
    match recv_type(&mut ws, "error").await {
        ServerMsg::Error { message } => assert!(message.contains("pin"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn action_button_matches_chat() {
    let addr = spawn().await;
    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    send(
        &mut guest,
        &ClientMsg::Action {
            name: causewaybay_panda_protocol::ActionName::Add,
            item_id: "panda_bun".into(),
            qty: 1,
            method: None,
            item: None,
            tx_hash: String::new(),
            order_id: String::new(),
            status: String::new(),
            on: false,
        },
    )
    .await;
    match recv_cart_with_lines(&mut guest, 1).await {
        ServerMsg::Cart { lines, .. } => {
            assert_eq!(lines[0].item_id, "panda_bun");
            assert_eq!(lines[0].qty, 1);
        }
        other => panic!("{other:?}"),
    }
}

/// A till wired to real USDC on Cronos, with a treasury that is not the
/// placeholder. No network is touched: the browser would sign, not the server.
async fn spawn_onchain() -> SocketAddr {
    let mut state = AppState::memory("panda").expect("db");
    let cfg = Config {
        chain_key: Some("cronos_mainnet".into()),
        treasury: Some(TREASURY.into()),
        token: None,
        decimals: None,
        rpc_url: Some(spawn_mock_rpc().await),
    };
    let st = Arc::get_mut(&mut state).unwrap();
    st.shop = Shop::resolve(
        &ShopConfig {
            mode: Some("live".into()),
            ..Default::default()
        },
        Settle::resolve(&cfg, TREASURY),
    );
    // The mock answers at once; do not sit out the real chain's patience.
    st.receipt_patience = Duration::from_millis(50);
    serve(state).await
}

/// A live shop's counter is handed what the treasury holds on chain, on the
/// way in and again after real USDC lands; a simulation never mentions one.
#[tokio::test]
async fn the_counter_reads_the_treasury_off_the_chain() {
    let addr = spawn_onchain().await;
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    match recv_type(&mut owner, "treasury").await {
        ServerMsg::Treasury {
            address,
            usdc,
            display,
            explorer_url,
            token,
            ..
        } => {
            assert_eq!(address, TREASURY);
            assert_eq!(usdc, "250");
            assert_eq!(display, "HK$1,950.00");
            assert_eq!(
                explorer_url,
                format!("https://cronoscan.com/address/{TREASURY}")
            );
            assert_eq!(token, USDC);
        }
        other => panic!("{other:?}"),
    }
    // A wallet payment lands: the counter is handed a fresh reading.
    let mut guest = wallet_pay(addr, &hash(PAID)).await;
    recv_type(&mut guest, "paid").await;
    assert!(matches!(
        recv_type(&mut owner, "treasury").await,
        ServerMsg::Treasury { .. }
    ));

    // The simulation counter is never told about a treasury.
    let sim = spawn().await;
    let mut o2 = connect(sim).await;
    send(
        &mut o2,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut o2, "welcome").await;
    send(&mut o2, &ClientMsg::Ping).await;
    // Everything up to the pong comes with the welcome; no treasury among it.
    loop {
        let m = recv_type_any(&mut o2).await;
        assert!(!matches!(m, ServerMsg::Treasury { .. }));
        if matches!(m, ServerMsg::Pong) {
            break;
        }
    }
}

/// The next frame of any type.
async fn recv_type_any(ws: &mut Ws) -> ServerMsg {
    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!("timed out"),
            frame = ws.next() => {
                let Some(Ok(Message::Text(text))) = frame else { continue; };
                return serde_json::from_str(&text).expect("server msg");
            }
        }
    }
}

/// Log in, order two lattes, and ask to pay by wallet with `tx_hash`.
async fn wallet_pay(addr: SocketAddr, tx_hash: &str) -> Ws {
    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "two lattes".into(),
        },
    )
    .await;
    recv_cart_with_lines(&mut guest, 1).await;
    send(
        &mut guest,
        &ClientMsg::Action {
            name: ActionName::Pay,
            item_id: String::new(),
            qty: 0,
            method: Some(PayMethod::Wallet),
            item: None,
            tx_hash: tx_hash.into(),
            order_id: String::new(),
            status: String::new(),
            on: false,
        },
    )
    .await;
    guest
}

/// A guest is sent an empty cart at login so the page knows their purse.
/// Skip past it to the cart a test actually ordered.
async fn recv_cart_with_lines(ws: &mut Ws, want: usize) -> ServerMsg {
    for _ in 0..8 {
        let msg = recv_type(ws, "cart").await;
        if let ServerMsg::Cart { ref lines, .. } = msg {
            if lines.len() == want {
                return msg;
            }
        }
    }
    panic!("no cart with {want} line(s) arrived");
}

fn error_text(msg: ServerMsg) -> String {
    match msg {
        ServerMsg::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

async fn spawn() -> SocketAddr {
    let state = AppState::memory("panda").expect("db");
    serve(state).await
}

async fn serve(state: std::sync::Arc<AppState>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: SocketAddr) -> Ws {
    let url = format!("ws://{addr}/ws");
    let (ws, _) = tokio_tungstenite::connect_async(url).await.expect("ws");
    ws
}

async fn send(ws: &mut Ws, msg: &ClientMsg) {
    let text = serde_json::to_string(msg).unwrap();
    ws.send(Message::Text(text.into())).await.unwrap();
}

async fn recv_type(ws: &mut Ws, want: &str) -> ServerMsg {
    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!("timed out waiting for {want}"),
            frame = ws.next() => {
                let Some(Ok(Message::Text(text))) = frame else { continue; };
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if typ == want {
                    return serde_json::from_value(v).expect("server msg");
                }
            }
        }
    }
}

/// The wallet settlement in full: prepare, sign in the browser, hand the hash
/// back. Only the second call moves the order into the books.
#[tokio::test]
async fn wallet_payment_is_prepared_then_settled_by_hash() {
    let addr = spawn_onchain().await;
    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    match recv_type(&mut guest, "welcome").await {
        ServerMsg::Welcome { settlement, .. } => {
            assert!(settlement.onchain, "{}", settlement.reason);
            assert_eq!(settlement.chain_id, 25);
            assert_eq!(settlement.chain_id_hex, "0x19");
            assert_eq!(
                settlement.usdc_address,
                causewaybay_panda_protocol::CRONOS_MAINNET.usdc.unwrap()
            );
        }
        other => panic!("{other:?}"),
    }

    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "two lattes".into(),
        },
    )
    .await;
    recv_type(&mut guest, "cart").await;

    // Step one: ask to pay. Nothing is recorded yet.
    send(
        &mut guest,
        &ClientMsg::Action {
            name: ActionName::Pay,
            item_id: String::new(),
            qty: 0,
            method: Some(PayMethod::Wallet),
            item: None,
            tx_hash: String::new(),
            order_id: String::new(),
            status: String::new(),
            on: false,
        },
    )
    .await;
    let call_data = match recv_type(&mut guest, "pay_request").await {
        ServerMsg::PayRequest {
            amount_usdc,
            amount_micro,
            token,
            treasury,
            chain_id,
            call_data,
            ..
        } => {
            assert_eq!(amount_usdc, "9.74359");
            assert_eq!(amount_micro, "9743590");
            assert_eq!(chain_id, 25);
            assert_eq!(treasury, TREASURY);
            assert_eq!(
                token,
                causewaybay_panda_protocol::CRONOS_MAINNET.usdc.unwrap()
            );
            assert!(call_data.starts_with("0xa9059cbb"), "{call_data}");
            call_data
        }
        other => panic!("{other:?}"),
    };
    // The payee and the amount are both readable in the bytes the guest signs.
    assert!(call_data.contains(&TREASURY[2..].to_lowercase()));
    assert!(call_data.ends_with(&format!("{:064x}", 9_743_590u128)));

    // A hash that is not a hash is refused.
    send(
        &mut guest,
        &ClientMsg::Action {
            name: ActionName::Pay,
            item_id: String::new(),
            qty: 0,
            method: Some(PayMethod::Wallet),
            item: None,
            tx_hash: "i-paid-honest".into(),
            order_id: String::new(),
            status: String::new(),
            on: false,
        },
    )
    .await;
    match recv_type(&mut guest, "error").await {
        ServerMsg::Error { message } => assert!(message.contains("transaction hash"), "{message}"),
        other => panic!("{other:?}"),
    }

    // Step two: a hash the chain confirms settles it.
    let hash = hash(PAID);
    send(
        &mut guest,
        &ClientMsg::Action {
            name: ActionName::Pay,
            item_id: String::new(),
            qty: 0,
            method: Some(PayMethod::Wallet),
            item: None,
            tx_hash: hash.clone(),
            order_id: String::new(),
            status: String::new(),
            on: false,
        },
    )
    .await;
    // The till says it is looking before it says it is paid.
    match recv_type(&mut guest, "assistant").await {
        ServerMsg::Assistant { text, .. } => {
            assert!(text.contains("Checking Cronos Mainnet"), "{text}")
        }
        other => panic!("{other:?}"),
    }
    match recv_type(&mut guest, "paid").await {
        ServerMsg::Paid {
            amount_usdc,
            method,
            tx_hash,
            explorer_url,
            ..
        } => {
            assert_eq!(amount_usdc, "9.74359");
            assert!(matches!(method, PayMethod::Wallet));
            assert_eq!(tx_hash, hash, "the chain's hash is what gets stored");
            assert_eq!(explorer_url, format!("https://cronoscan.com/tx/{hash}"));
        }
        other => panic!("{other:?}"),
    }

    // Real USDC moved, so the play-money grant is untouched.
    match recv_type(&mut guest, "cart").await {
        ServerMsg::Cart {
            balance_usdc,
            total_usdc,
            ..
        } => {
            assert_eq!(
                balance_usdc, "50",
                "an on-chain payment must not debit the grant"
            );
            assert_eq!(total_usdc, "0");
        }
        other => panic!("{other:?}"),
    }
}

/// With nothing configured, asking to pay by wallet explains itself instead of
/// producing a transaction to nowhere.
#[tokio::test]
async fn wallet_payment_is_refused_when_the_till_is_play_money() {
    let addr = spawn().await;
    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    match recv_type(&mut guest, "welcome").await {
        ServerMsg::Welcome { settlement, .. } => {
            assert!(!settlement.onchain);
            assert_eq!(settlement.usdc_address, "");
            assert!(!settlement.reason.is_empty());
        }
        other => panic!("{other:?}"),
    }
    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "latte".into(),
        },
    )
    .await;
    recv_cart_with_lines(&mut guest, 1).await;
    send(
        &mut guest,
        &ClientMsg::Action {
            name: ActionName::Pay,
            item_id: String::new(),
            qty: 0,
            method: Some(PayMethod::Wallet),
            item: None,
            tx_hash: String::new(),
            order_id: String::new(),
            status: String::new(),
            on: false,
        },
    )
    .await;
    match recv_type(&mut guest, "error").await {
        ServerMsg::Error { message } => {
            assert!(message.contains("wallet payment is off"), "{message}")
        }
        other => panic!("{other:?}"),
    }
}

/// The claims the chain does not back. Each leaves the cart untouched and the
/// books empty.
#[tokio::test]
async fn a_hash_is_only_a_claim_until_the_chain_agrees() {
    let addr = spawn_onchain().await;

    let mut g = wallet_pay(addr, &hash(PENDING)).await;
    recv_type(&mut g, "assistant").await;
    let m = error_text(recv_type(&mut g, "error").await);
    assert!(m.contains("not confirmed"), "{m}");

    let mut g = wallet_pay(addr, &hash(REVERTED)).await;
    recv_type(&mut g, "assistant").await;
    let m = error_text(recv_type(&mut g, "error").await);
    assert!(m.contains("reverted"), "{m}");

    let mut g = wallet_pay(addr, &hash(ELSEWHERE)).await;
    recv_type(&mut g, "assistant").await;
    let m = error_text(recv_type(&mut g, "error").await);
    assert!(m.contains("did not pay this shop"), "{m}");

    let mut g = wallet_pay(addr, &hash(SHORT)).await;
    recv_type(&mut g, "assistant").await;
    let m = error_text(recv_type(&mut g, "error").await);
    assert!(m.contains("bill is HK$76.00"), "{m}");

    // The owner's book has none of them.
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    send(
        &mut owner,
        &ClientMsg::Chat {
            text: "payments".into(),
        },
    )
    .await;
    match recv_type(&mut owner, "payments").await {
        ServerMsg::Payments { payments } => assert!(payments.is_empty(), "{payments:?}"),
        other => panic!("{other:?}"),
    }
}

/// One transaction pays one order, even if it was generous.
#[tokio::test]
async fn a_confirmed_hash_cannot_pay_twice() {
    let addr = spawn_onchain().await;
    let mut first = wallet_pay(addr, &hash(PAID)).await;
    recv_type(&mut first, "assistant").await;
    recv_type(&mut first, "paid").await;

    // A second guest presents the same hash. The mock chain still says it
    // paid 100 USDC, more than enough — but it is spent.
    let mut second = wallet_pay(addr, &hash(PAID).to_uppercase().replace("0X", "0x")).await;
    recv_type(&mut second, "assistant").await;
    let m = error_text(recv_type(&mut second, "error").await);
    assert!(m.contains("already paid"), "{m}");
    // The refusal rolled back inside the till: the cart is still theirs to pay.
    send(
        &mut second,
        &ClientMsg::Chat {
            text: "cart".into(),
        },
    )
    .await;
    match recv_type(&mut second, "cart").await {
        ServerMsg::Cart { total_usdc, .. } => {
            assert_eq!(total_usdc, "9.74359", "cart must survive")
        }
        other => panic!("{other:?}"),
    }
}

/// A reload hands back the session id and gets the same person: same name,
/// same purse, same cart.
#[tokio::test]
async fn a_reload_walks_back_in_as_the_same_guest() {
    let addr = spawn().await;
    let mut first = connect(addr).await;
    send(
        &mut first,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    let sid = match recv_type(&mut first, "welcome").await {
        ServerMsg::Welcome { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut first,
        &ClientMsg::Chat {
            text: "latte".into(),
        },
    )
    .await;
    recv_cart_with_lines(&mut first, 1).await;
    drop(first);

    // The browser comes back with what it remembered.
    let mut again = connect(addr).await;
    send(
        &mut again,
        &ClientMsg::Login {
            role: Role::Guest,
            name: String::new(),
            pin: String::new(),
            session: sid.clone(),
        },
    )
    .await;
    match recv_type(&mut again, "welcome").await {
        ServerMsg::Welcome {
            session_id, name, ..
        } => {
            assert_eq!(session_id, sid, "the same session, not a new one");
            assert_eq!(name, "Mei", "and the same name");
        }
        other => panic!("{other:?}"),
    }
    // The cart built before the reload is still theirs.
    recv_cart_with_lines(&mut again, 1).await;

    // A guest's id presented at the owner's door opens a fresh owner session.
    let mut stranger = connect(addr).await;
    send(
        &mut stranger,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: sid.clone(),
        },
    )
    .await;
    match recv_type(&mut stranger, "welcome").await {
        ServerMsg::Welcome {
            session_id, role, ..
        } => {
            assert_ne!(session_id, sid, "a guest session cannot become the owner");
            assert_eq!(role, Role::Owner);
        }
        other => panic!("{other:?}"),
    }
}

/// A cafe whose panda looks at the queue every 50 ms, so a test can watch
/// a ticket be picked up and called ready without waiting a minute.
async fn spawn_quick_kitchen() -> SocketAddr {
    let state = Arc::new(AppState {
        db: causewaybay_panda_server::db::Db::memory("panda").expect("db"),
        hub: causewaybay_panda_server::hub::Hub::new(),
        ai: Arc::new(parking_lot::RwLock::new(None)),
        shop: Shop::simulation(),
        receipt_patience: Duration::from_secs(1),
        demo: Arc::new(parking_lot::Mutex::new(
            causewaybay_panda_core::demo::Demo::new(7),
        )),
        demo_tick: Duration::from_secs(3),
        kitchen: Arc::new(parking_lot::Mutex::new(
            causewaybay_panda_core::kitchen::Kitchen::new(),
        )),
        kitchen_tick: Duration::from_millis(50),
    });
    serve(state).await
}

/// The owner lets the panda work the kitchen: a paid ticket is picked up and
/// called ready with nobody tapping; every owner hears the switch; a guest
/// may not throw it; handing over is still a person's.
#[tokio::test]
async fn the_panda_works_the_kitchen_when_the_owner_says_so() {
    let addr = spawn_quick_kitchen().await;
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    assert!(matches!(
        recv_type(&mut owner, "kitchen").await,
        ServerMsg::Kitchen { on: false }
    ));

    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    // A guest asking is refused.
    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "kitchen on".into(),
        },
    )
    .await;
    match recv_type(&mut guest, "error").await {
        ServerMsg::Error { message } => assert!(message.contains("only the owner"), "{message}"),
        other => panic!("{other:?}"),
    }

    send(
        &mut owner,
        &ClientMsg::Chat {
            text: "let the panda work the kitchen".into(),
        },
    )
    .await;
    assert!(matches!(
        recv_type(&mut owner, "kitchen").await,
        ServerMsg::Kitchen { on: true }
    ));

    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "latte".into(),
        },
    )
    .await;
    recv_cart_with_lines(&mut guest, 1).await;
    send(&mut guest, &ClientMsg::Chat { text: "pay".into() }).await;
    recv_type(&mut guest, "paid").await;
    // Without another frame from anyone, the card moves: made, then ready.
    let mut seen = Vec::new();
    while seen.len() < 3 {
        if let ServerMsg::OrderUpdate { order } = recv_type(&mut guest, "order_update").await {
            seen.push(order.status);
        }
    }
    use causewaybay_panda_protocol::wire::OrderStatus::*;
    assert_eq!(seen, vec![Placed, Preparing, Ready]);
    // And it stays ready: nobody hands it over but a person.
    tokio::time::sleep(Duration::from_millis(400)).await;
    send(
        &mut owner,
        &ClientMsg::Chat {
            text: "orders".into(),
        },
    )
    .await;
    match recv_type(&mut owner, "orders").await {
        ServerMsg::Orders { orders } => assert_eq!(orders[0].status, Ready),
        other => panic!("{other:?}"),
    }

    // Off again: a second owner opening now is told so.
    send(
        &mut owner,
        &ClientMsg::Chat {
            text: "kitchen off".into(),
        },
    )
    .await;
    assert!(matches!(
        recv_type(&mut owner, "kitchen").await,
        ServerMsg::Kitchen { on: false }
    ));
    let mut second = connect(addr).await;
    send(
        &mut second,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut second, "welcome").await;
    assert!(matches!(
        recv_type(&mut second, "kitchen").await,
        ServerMsg::Kitchen { on: false }
    ));
}

/// The owner's card and the guest's card both arrive without anyone asking:
/// on login, and again with the payment that changes them.
#[tokio::test]
async fn both_dashboards_arrive_with_the_payment() {
    let addr = spawn().await;
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    match recv_type(&mut owner, "dashboard").await {
        ServerMsg::Dashboard {
            orders, open, top, ..
        } => {
            assert_eq!((orders, open), (0, 0));
            assert!(top.is_empty());
        }
        other => panic!("{other:?}"),
    }

    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    match recv_type(&mut guest, "guest_dashboard").await {
        ServerMsg::GuestDashboard {
            orders, favourite, ..
        } => {
            assert_eq!(orders, 0);
            assert_eq!(favourite, "");
        }
        other => panic!("{other:?}"),
    }
    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "two lattes".into(),
        },
    )
    .await;
    recv_cart_with_lines(&mut guest, 1).await;
    send(&mut guest, &ClientMsg::Chat { text: "pay".into() }).await;
    recv_type(&mut guest, "paid").await;
    match recv_type(&mut guest, "guest_dashboard").await {
        ServerMsg::GuestDashboard {
            orders,
            spent_display,
            favourite,
            favourite_qty,
            open,
            ..
        } => {
            assert_eq!((orders, open, favourite_qty), (1, 1, 2));
            assert_eq!(spent_display, "HK$76.00");
            assert_eq!(favourite, "Hot latte");
        }
        other => panic!("{other:?}"),
    }
    match recv_type(&mut owner, "dashboard").await {
        ServerMsg::Dashboard {
            orders,
            open,
            placed,
            guests,
            total_display,
            top,
            ..
        } => {
            assert_eq!((orders, open, placed, guests), (1, 1, 1, 1));
            assert_eq!(total_display, "HK$76.00");
            assert_eq!(top[0].item_id, "latte");
            assert_eq!(top[0].qty, 2);
        }
        other => panic!("{other:?}"),
    }

    // Asking in words gets the same card, shaped by who asks.
    send(
        &mut owner,
        &ClientMsg::Chat {
            text: "today".into(),
        },
    )
    .await;
    assert!(matches!(
        recv_type(&mut owner, "dashboard").await,
        ServerMsg::Dashboard { orders: 1, .. }
    ));
    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "today".into(),
        },
    )
    .await;
    assert!(matches!(
        recv_type(&mut guest, "guest_dashboard").await,
        ServerMsg::GuestDashboard { orders: 1, .. }
    ));
}

/// The counter is told today's takings when it opens and after every payment.
#[tokio::test]
async fn the_counter_is_handed_todays_takings() {
    let addr = spawn().await;
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    match recv_type(&mut owner, "takings").await {
        ServerMsg::Takings {
            total_display,
            orders,
            ..
        } => {
            assert_eq!(total_display, "HK$0.00");
            assert_eq!(orders, 0);
        }
        other => panic!("{other:?}"),
    }

    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    send(
        &mut guest,
        &ClientMsg::Chat {
            text: "two lattes".into(),
        },
    )
    .await;
    recv_cart_with_lines(&mut guest, 1).await;
    send(&mut guest, &ClientMsg::Chat { text: "pay".into() }).await;
    recv_type(&mut guest, "paid").await;

    match recv_type(&mut owner, "takings").await {
        ServerMsg::Takings {
            total_display,
            orders,
            coin_display,
            wallet_display,
            ..
        } => {
            assert_eq!(total_display, "HK$76.00");
            assert_eq!(orders, 1);
            assert_eq!(coin_display, "HK$76.00", "simulation money");
            assert_eq!(wallet_display, "HK$0.00");
        }
        other => panic!("{other:?}"),
    }
}

/// The owner chooses who listens from the counter. It is kept by the shop,
/// applied at once, never echoed back, and not a guest's to change.
#[tokio::test]
async fn the_owner_chooses_who_listens_and_the_shop_keeps_it() {
    let addr = spawn().await;
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    match recv_type(&mut owner, "ai_status").await {
        ServerMsg::AiStatus {
            ready, providers, ..
        } => {
            assert!(!ready, "a fresh shop is parser-only");
            assert!(providers.iter().any(|p| p.key == "openrouter"));
            assert!(providers.iter().any(|p| p.key == "anthropic"));
        }
        other => panic!("{other:?}"),
    }

    // A provider that needs a key, given none: refused.
    send(
        &mut owner,
        &ClientMsg::AiSetup {
            provider: "openai".into(),
            key: String::new(),
            model: String::new(),
        },
    )
    .await;
    match recv_type(&mut owner, "error").await {
        ServerMsg::Error { message } => assert!(message.contains("key"), "{message}"),
        other => panic!("{other:?}"),
    }

    // With a key: on, with the default model, and the key stays home.
    send(
        &mut owner,
        &ClientMsg::AiSetup {
            provider: "openrouter".into(),
            key: "or-secret".into(),
            model: String::new(),
        },
    )
    .await;
    let raw = match recv_type(&mut owner, "ai_status").await {
        ServerMsg::AiStatus {
            provider,
            model,
            ready,
            ..
        } => {
            assert!(ready);
            assert_eq!(provider, "openrouter");
            assert_eq!(model, "openai/gpt-4o-mini");
            format!("{provider}{model}")
        }
        other => panic!("{other:?}"),
    };
    assert!(!raw.contains("or-secret"));

    // A model change with an empty key keeps the key already held.
    send(
        &mut owner,
        &ClientMsg::AiSetup {
            provider: "openrouter".into(),
            key: String::new(),
            model: "anthropic/claude-sonnet-4".into(),
        },
    )
    .await;
    match recv_type(&mut owner, "ai_status").await {
        ServerMsg::AiStatus { ready, model, .. } => {
            assert!(ready);
            assert_eq!(model, "anthropic/claude-sonnet-4");
        }
        other => panic!("{other:?}"),
    }

    // A second owner opening the counter sees the same choice.
    let mut again = connect(addr).await;
    send(
        &mut again,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut again, "welcome").await;
    match recv_type(&mut again, "ai_status").await {
        ServerMsg::AiStatus { provider, .. } => assert_eq!(provider, "openrouter"),
        other => panic!("{other:?}"),
    }

    // A guest may not touch it.
    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    send(
        &mut guest,
        &ClientMsg::AiSetup {
            provider: "off".into(),
            key: String::new(),
            model: String::new(),
        },
    )
    .await;
    match recv_type(&mut guest, "error").await {
        ServerMsg::Error { message } => assert!(message.contains("owner"), "{message}"),
        other => panic!("{other:?}"),
    }

    // Off hands the chat back to the parser.
    send(
        &mut owner,
        &ClientMsg::AiSetup {
            provider: "off".into(),
            key: String::new(),
            model: String::new(),
        },
    )
    .await;
    match recv_type(&mut owner, "ai_status").await {
        ServerMsg::AiStatus {
            ready, provider, ..
        } => {
            assert!(!ready);
            assert_eq!(provider, "off");
        }
        other => panic!("{other:?}"),
    }
}

/// The switch answers in both directions: a word to the one who threw it and
/// the new state to every owner. Pinned here because it is easy to lose in a
/// browser race and hard to lose over a raw socket.
#[tokio::test]
async fn the_switch_answers_on_and_off() {
    let addr = spawn().await;
    let mut owner = connect(addr).await;
    send(
        &mut owner,
        &ClientMsg::Login {
            role: Role::Owner,
            name: "Wing".into(),
            pin: "panda".into(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut owner, "welcome").await;
    match recv_type(&mut owner, "auto").await {
        ServerMsg::Auto { on } => assert!(!on, "a fresh shop is not running itself"),
        other => panic!("{other:?}"),
    }
    let throw = |on: bool| ClientMsg::Action {
        name: ActionName::Auto,
        item_id: String::new(),
        qty: 0,
        method: None,
        item: None,
        tx_hash: String::new(),
        order_id: String::new(),
        status: String::new(),
        on,
    };
    send(&mut owner, &throw(true)).await;
    match recv_type(&mut owner, "assistant").await {
        ServerMsg::Assistant { text, .. } => assert!(text.contains("running on its own"), "{text}"),
        other => panic!("{other:?}"),
    }
    match recv_type(&mut owner, "auto").await {
        ServerMsg::Auto { on } => assert!(on),
        other => panic!("{other:?}"),
    }
    send(&mut owner, &throw(false)).await;
    match recv_type(&mut owner, "assistant").await {
        ServerMsg::Assistant { text, .. } => assert!(text.contains("Stopped"), "{text}"),
        other => panic!("{other:?}"),
    }
    match recv_type(&mut owner, "auto").await {
        ServerMsg::Auto { on } => assert!(!on),
        other => panic!("{other:?}"),
    }
    // A guest asking is refused.
    let mut guest = connect(addr).await;
    send(
        &mut guest,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut guest, "welcome").await;
    send(&mut guest, &throw(true)).await;
    match recv_type(&mut guest, "error").await {
        ServerMsg::Error { message } => assert!(message.contains("owner"), "{message}"),
        other => panic!("{other:?}"),
    }
}

/// A web page from anywhere else, open on a device on the cafe wifi, must not
/// be able to drive the till. Only the shop's own page — or a client that is
/// not a browser at all — gets a socket.
#[tokio::test]
async fn a_socket_is_only_for_the_shops_own_page() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let addr = spawn().await;
    let url = format!("ws://{addr}/ws");

    // Another site's page: refused at the door.
    let mut foreign = url.as_str().into_client_request().unwrap();
    foreign
        .headers_mut()
        .insert("Origin", "https://evil.example".parse().unwrap());
    let refused = tokio_tungstenite::connect_async(foreign).await;
    assert!(refused.is_err(), "a foreign origin must not get a socket");

    // The shop's own page: the address it was served from.
    let mut own = url.as_str().into_client_request().unwrap();
    own.headers_mut()
        .insert("Origin", format!("http://{addr}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(own)
        .await
        .expect("the shop's own page gets in");
    send(
        &mut ws,
        &ClientMsg::Login {
            role: Role::Guest,
            name: "Mei".into(),
            pin: String::new(),
            session: String::new(),
        },
    )
    .await;
    recv_type(&mut ws, "welcome").await;
}
