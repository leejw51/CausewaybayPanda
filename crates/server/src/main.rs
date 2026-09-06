use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::get_service;
use axum::Router;
use causewaybay_panda_server::{
    ai::Ai, db::Db, hub::Hub, router as api_router, settlement::Settle, shop::Shop, AppState,
};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let root = detect_root();
    let data_dir = data_home();
    std::fs::create_dir_all(&data_dir).expect("data dir");
    let db_path = data_dir.join("panda.sqlite");
    let pin = std::env::var("PANDA_OWNER_PIN").unwrap_or_else(|_| "panda".into());
    let db = Db::open(&db_path, &pin).expect("sqlite");
    // The owner's choice from the counter outlives a restart and wins over
    // whatever the environment says.
    let ai = {
        use causewaybay_panda_core::Store;
        let saved = Ai::from_settings(
            db.setting(causewaybay_panda_ai::SETTING_PROVIDER)
                .ok()
                .flatten()
                .as_deref(),
            db.setting(causewaybay_panda_ai::SETTING_KEY)
                .ok()
                .flatten()
                .as_deref(),
            db.setting(causewaybay_panda_ai::SETTING_MODEL)
                .ok()
                .flatten()
                .as_deref(),
        );
        match db
            .setting(causewaybay_panda_ai::SETTING_PROVIDER)
            .ok()
            .flatten()
            .as_deref()
        {
            Some("off") => None,
            Some(_) => saved,
            None => Ai::from_env(),
        }
    };
    let settle = Settle::from_env(&db.treasury().unwrap_or_default());
    let shop = Shop::from_env(settle);
    let db = db.with_denom(shop.denom.clone());
    // The kitchen switch is kept by the shop: a restart finds it as left.
    let kitchen = causewaybay_panda_core::kitchen::Kitchen::from_store(&db);
    let state = Arc::new(AppState {
        db,
        hub: Hub::new(),
        ai: Arc::new(parking_lot::RwLock::new(ai)),
        shop,
        // How long a wallet payment may take to land before the guest is told
        // to try again. Ops can shorten it; the test harness does.
        receipt_patience: std::env::var("PANDA_RECEIPT_WAIT_SECS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(std::time::Duration::from_secs)
            .unwrap_or(causewaybay_panda_server::RECEIPT_PATIENCE),
        demo: Arc::new(parking_lot::Mutex::new(
            causewaybay_panda_core::demo::Demo::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(7),
            ),
        )),
        demo_tick: std::env::var("PANDA_DEMO_TICK_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(std::time::Duration::from_millis)
            .unwrap_or(causewaybay_panda_server::DEMO_TICK),
        kitchen: Arc::new(parking_lot::Mutex::new(kitchen)),
        kitchen_tick: std::env::var("PANDA_KITCHEN_TICK_MS")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(std::time::Duration::from_millis)
            .unwrap_or(causewaybay_panda_server::KITCHEN_TICK),
    });
    causewaybay_panda_server::ws::resume_kitchen(state.clone());

    let port: u16 = std::env::var("PANDA_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);

    let static_dir = root.join("static");
    let dist = root.join("crates/web/dist");
    let files = if dist.join("index.html").exists() {
        dist
    } else {
        static_dir.clone()
    };

    let app = Router::new()
        .merge(api_router(state.clone()))
        .route_service("/", get_service(ServeFile::new(files.join("index.html"))))
        .nest_service("/assets", ServeDir::new(static_dir.join("assets")))
        .nest_service("/vendor", ServeDir::new(static_dir.join("vendor")))
        .nest_service("/js", ServeDir::new(static_dir.join("js")))
        .nest_service("/css", ServeDir::new(static_dir.join("css")))
        .fallback_service(ServeDir::new(files))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("CAUSEWAYBAY PANDA  ·  Causewaybay Coffee");
    println!("owner pin          {pin}");
    println!("sqlite             {}", db_path.display());
    println!("local              http://127.0.0.1:{port}");
    for ip in local_ips() {
        println!("lan                http://{ip}:{port}");
    }
    match state.ai.read().as_ref() {
        Some(ai) => println!(
            "chat               {} (falls back to the local parser)",
            ai.describe()
        ),
        None => println!("chat               local parser only"),
    }
    let shop = &state.shop;
    println!(
        "board              {} at {} per USDC",
        shop.denom.code,
        shop.wire().denom.rate
    );
    if shop.onchain() {
        let settle = &shop.settle;
        println!(
            "mode               LIVE — real USDC on {}",
            settle.chain.name
        );
        println!("usdc               {}", settle.token_address());
        println!("treasury           {}", settle.treasury_address());
        println!("rpc                {}", settle.rpc_url());
    } else {
        println!(
            "mode               SIMULATION — {} test money, faucet on",
            causewaybay_panda_protocol::COIN_NAME
        );
        if !shop.mode_reason.is_empty() {
            println!("                   {}", shop.mode_reason);
        }
    }
    axum::serve(listener, app).await.expect("serve");
}

fn detect_root() -> PathBuf {
    if let Ok(explicit) = std::env::var("PANDA_ROOT") {
        return PathBuf::from(explicit);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.ancestors()
        .find(|p| p.join("crates/server").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or(cwd)
}

fn data_home() -> PathBuf {
    if let Ok(p) = std::env::var("PANDA_HOME") {
        return PathBuf::from(p);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".causewaybaypanda"),
        None => PathBuf::from(".").join(".causewaybaypanda"),
    }
}

fn local_ips() -> Vec<std::net::IpAddr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.ip())
        .filter(|ip| match ip {
            std::net::IpAddr::V4(v) => !v.is_loopback() && !v.is_link_local(),
            _ => false,
        })
        .collect()
}
