use crate::state::AppState;
use axum::{Router, routing::get};

mod health;
mod rooms;
pub mod signaling;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(rooms::router())
        .route("/ws", get(signaling::websocket))
        .with_state(state)
}
