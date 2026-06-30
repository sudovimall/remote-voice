use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde::Serialize;

mod auth;
mod health;
mod rooms;
pub mod signaling;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(lobby_page))
        .route("/rooms/{room_id}", get(room_page))
        .route("/ui/assets/{asset}", get(ui_asset))
        .route("/assets/{asset}", get(asset))
        .route("/api/client-config", get(client_config))
        .merge(health::router())
        .merge(auth::router())
        .merge(rooms::router())
        .route("/ws", get(signaling::websocket))
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct ClientConfig {
    screen_share: crate::config::settings::ScreenShareSettings,
    video_call: crate::config::settings::VideoCallSettings,
}

async fn client_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if state.auth.is_enabled() && auth::api_user(&state, &headers).is_err() {
        return crate::Error::Unauthenticated.into_response();
    }

    Json(ClientConfig {
        screen_share: state.screen_share,
        video_call: state.video_call,
    })
    .into_response()
}

async fn lobby_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let next = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if let Err(response) = auth::page_user_or_redirect(&state, &headers, next) {
        return response;
    }
    vue_page().await
}

async fn room_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Path(_room_id): Path<String>,
) -> Response {
    let next = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if let Err(response) = auth::page_user_or_redirect(&state, &headers, next) {
        return response;
    }
    vue_page().await
}

async fn vue_page() -> Response {
    match tokio::fs::read_to_string("static/dist/index.html").await {
        Ok(source) => Html(source).into_response(),
        Err(error) => crate::Error::Internal(format!("Vue 页面产物不可用: {error}")).into_response(),
    }
}

async fn ui_asset(Path(asset): Path<String>) -> Response {
    if asset.contains("..") || asset.contains('/') || asset.contains('\\') {
        return StatusCode::NOT_FOUND.into_response();
    }

    let content_type = match asset.rsplit_once('.').map(|(_, extension)| extension) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    match tokio::fs::read(format!("static/dist/assets/{asset}")).await {
        Ok(source) => ([(header::CONTENT_TYPE, content_type)], source).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn asset(Path(asset): Path<String>) -> Response {
    match asset.as_str() {
        "styles.css" => (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            include_str!("../../../static/styles.css"),
        )
            .into_response(),
        "auth-page.js" => javascript(include_str!("../../../static/auth-page.js")),
        "admin.js" => javascript(include_str!("../../../static/admin.js")),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn javascript(source: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        source,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::router;
    use crate::{config::settings::Settings, state::AppState};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn 大厅页和房间页可以访问() {
        let app = router(AppState::new(8).expect("创建应用状态"));

        let lobby = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("构造大厅页请求"),
            )
            .await
            .expect("读取大厅页响应");
        assert_eq!(lobby.status(), StatusCode::OK);
        let lobby_body = to_bytes(lobby.into_body(), 1024 * 1024)
            .await
            .expect("读取大厅页响应体");
        let lobby_html = String::from_utf8_lossy(&lobby_body);
        assert!(lobby_html.contains(r#"id="app""#));
        assert!(lobby_html.contains("remote-voice-ui"));
        assert!(lobby_html.contains(r#"/ui/assets/app.js"#));
        assert!(lobby_html.contains(r#"rel="icon""#));

        let room = app
            .oneshot(
                Request::builder()
                    .uri("/rooms/ABC123")
                    .body(Body::empty())
                    .expect("构造房间页请求"),
            )
            .await
            .expect("读取房间页响应");
        assert_eq!(room.status(), StatusCode::OK);
        let room_body = to_bytes(room.into_body(), 1024 * 1024)
            .await
            .expect("读取房间页响应体");
        let room_html = String::from_utf8_lossy(&room_body);
        assert!(room_html.contains(r#"id="app""#));
        assert!(room_html.contains("remote-voice-ui"));
        assert!(room_html.contains(r#"/ui/assets/app.js"#));
        assert!(room_html.contains(r#"rel="icon""#));
    }

    #[tokio::test]
    async fn vue_构建产物可以通过_ui_assets_访问() {
        let app = router(AppState::new(8).expect("创建应用状态"));

        for (asset, content_type) in [
            ("app.js", "text/javascript"),
            ("index.css", "text/css"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/ui/assets/{asset}"))
                        .body(Body::empty())
                        .expect("构造 Vue 静态资源请求"),
                )
                .await
                .expect("读取 Vue 静态资源响应");

            assert_eq!(response.status(), StatusCode::OK, "asset {asset}");
            let header = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            assert!(
                header.starts_with(content_type),
                "asset {asset} content type {header}"
            );
        }
    }

    #[tokio::test]
    async fn 客户端配置返回屏幕共享策略() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            screen_share:
              max_width: 1024
              max_height: 576
              max_frame_rate: 10
              bitrate_rules:
                - max_viewers: 1
                  max_bitrate_bps: 1500000
                - max_viewers: 4
                  max_bitrate_bps: 600000
            video_call:
              max_width: 800
              max_height: 450
              max_frame_rate: 18
              bitrate_rules:
                - max_publishers: 1
                  max_bitrate_bps: 900000
                - max_publishers: 4
                  max_bitrate_bps: 450000
            "#,
        )
        .expect("解析配置");
        let app = router(AppState::from_settings(&settings).expect("创建应用状态"));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/client-config")
                    .body(Body::empty())
                    .expect("构造客户端配置请求"),
            )
            .await
            .expect("读取客户端配置响应");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("读取客户端配置响应体");
        let config: serde_json::Value = serde_json::from_slice(&body).expect("客户端配置是 JSON");
        assert_eq!(config["screen_share"]["max_width"], 1024);
        assert_eq!(config["screen_share"]["max_height"], 576);
        assert_eq!(config["screen_share"]["max_frame_rate"], 10);
        assert_eq!(
            config["screen_share"]["bitrate_rules"][1]["max_bitrate_bps"],
            600000
        );
        assert_eq!(config["video_call"]["max_width"], 800);
        assert_eq!(config["video_call"]["max_height"], 450);
        assert_eq!(config["video_call"]["max_frame_rate"], 18);
        assert_eq!(
            config["video_call"]["bitrate_rules"][1]["max_bitrate_bps"],
            450000
        );
    }
}
