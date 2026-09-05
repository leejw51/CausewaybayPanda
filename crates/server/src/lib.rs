//! Library surface so the cafe can be tested without spawning the binary.

pub mod ai;
pub mod db;
pub mod hub;
pub mod verify;

// The cafe itself. Same code the browser engine runs.
pub use causewaybay_panda_core::{cafe, settlement, shop};
pub mod ws;

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::db::Db;
use crate::hub::Hub;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub hub: Hub,
    pub ai: Option<ai::Ai>,
    pub shop: shop::Shop,
    /// How long to wait for a wallet payment's receipt before giving up.
    pub receipt_patience: std::time::Duration,
    /// The cafe running itself, when the owner has switched it on.
    pub demo: Arc<parking_lot::Mutex<causewaybay_panda_core::demo::Demo>>,
    /// How often the self-running cafe takes a step.
    pub demo_tick: std::time::Duration,
}

/// Three seconds reads as a busy afternoon, not a slot machine.
pub const DEMO_TICK: std::time::Duration = std::time::Duration::from_secs(3);

/// Cronos mines roughly every six seconds; a guest's wallet usually reports
/// the hash before the block. Ninety seconds covers a slow one.
pub const RECEIPT_PATIENCE: std::time::Duration = std::time::Duration::from_secs(90);

impl AppState {
    pub fn memory(pin: &str) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            db: Db::memory(pin)?,
            hub: Hub::new(),
            ai: None,
            shop: shop::Shop::simulation(),
            receipt_patience: RECEIPT_PATIENCE,
            demo: Arc::new(parking_lot::Mutex::new(
                causewaybay_panda_core::demo::Demo::new(7),
            )),
            demo_tick: DEMO_TICK,
        }))
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws::upgrade))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let grok = state.ai.is_some();
    let ai = state.ai.as_ref().map(|a| a.provider.key()).unwrap_or("off");
    let n = state.db.menu().map(|m| m.len()).unwrap_or(0);
    let onchain = state.shop.onchain();
    let chain = state.shop.settle.chain.key;
    let mode = if state.shop.is_simulation() {
        "simulation"
    } else {
        "live"
    };
    let denom = &state.shop.denom.code;
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        format!(
            "{{\"ok\":true,\"cafe\":\"Causewaybay Coffee\",\"grok\":{grok},\"ai\":\"{ai}\",\
             \"menu\":{n},\"mode\":\"{mode}\",\"denom\":\"{denom}\",\
             \"chain\":\"{chain}\",\"onchain\":{onchain}}}\n"
        ),
    )
}
