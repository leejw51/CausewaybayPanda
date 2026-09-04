//! Library surface so the cafe can be tested without spawning the binary.

pub mod cafe;
pub mod db;
pub mod grok;
pub mod hub;
pub mod settlement;
pub mod verify;
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
    pub grok_key: Option<String>,
    pub settle: settlement::Settle,
    /// How long to wait for a wallet payment's receipt before giving up.
    pub receipt_patience: std::time::Duration,
}

/// Cronos mines roughly every six seconds; a guest's wallet usually reports
/// the hash before the block. Ninety seconds covers a slow one.
pub const RECEIPT_PATIENCE: std::time::Duration = std::time::Duration::from_secs(90);

impl AppState {
    pub fn memory(pin: &str) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            db: Db::memory(pin)?,
            hub: Hub::new(),
            grok_key: None,
            settle: settlement::Settle::demo(),
            receipt_patience: RECEIPT_PATIENCE,
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
    let grok = state.grok_key.is_some();
    let n = state.db.menu().map(|m| m.len()).unwrap_or(0);
    let onchain = state.settle.onchain();
    let chain = state.settle.chain.key;
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        format!(
            "{{\"ok\":true,\"cafe\":\"Causewaybay Coffee\",\"grok\":{grok},\"menu\":{n},\
             \"chain\":\"{chain}\",\"onchain\":{onchain}}}\n"
        ),
    )
}
