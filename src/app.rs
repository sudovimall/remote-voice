use crate::{Result, config::settings::Settings, state::AppState, transport::http};
use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

pub fn build_router(state: AppState) -> Router {
    http::router(state)
}

pub async fn serve(settings: Settings) -> Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], settings.port));
    let state = AppState::from_settings(&settings)?;
    let app = build_router(state);
    let listener = TcpListener::bind(addr).await?;

    info!("HTTP 服务已启动，监听地址：{addr}");
    axum::serve(listener, app).await?;

    Ok(())
}
