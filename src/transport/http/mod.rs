use crate::state::AppState;
use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

mod health;
mod rooms;
pub mod signaling;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(lobby_page))
        .route("/rooms/{room_id}", get(room_page))
        .route("/assets/{asset}", get(asset))
        .merge(health::router())
        .merge(rooms::router())
        .route("/ws", get(signaling::websocket))
        .with_state(state)
}

async fn lobby_page() -> Html<&'static str> {
    Html(include_str!("../../../static/index.html"))
}

async fn room_page(Path(_room_id): Path<String>) -> Html<&'static str> {
    Html(include_str!("../../../static/room.html"))
}

async fn asset(Path(asset): Path<String>) -> Response {
    match asset.as_str() {
        "styles.css" => (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            include_str!("../../../static/styles.css"),
        )
            .into_response(),
        "lobby.js" => javascript(include_str!("../../../static/lobby.js")),
        "lobby-rooms.mjs" => javascript(include_str!("../../../static/lobby-rooms.mjs")),
        "room.js" => javascript(include_str!("../../../static/room.js")),
        "room-entry.mjs" => javascript(include_str!("../../../static/room-entry.mjs")),
        "room-state.mjs" => javascript(include_str!("../../../static/room-state.mjs")),
        "room-connection.mjs" => javascript(include_str!("../../../static/room-connection.mjs")),
        "room-controls.mjs" => javascript(include_str!("../../../static/room-controls.mjs")),
        "media-session.mjs" => javascript(include_str!("../../../static/media-session.mjs")),
        "signaling-client.mjs" => javascript(include_str!("../../../static/signaling-client.mjs")),
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
    use crate::state::AppState;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
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
        assert!(lobby_html.contains("voice-lobby"));
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
        assert!(room_html.contains("voice-room"));
        assert!(room_html.contains(r#"rel="icon""#));
    }

    #[tokio::test]
    async fn 页面静态模块可以访问() {
        let app = router(AppState::new(8).expect("创建应用状态"));

        for asset in [
            "styles.css",
            "lobby.js",
            "lobby-rooms.mjs",
            "room.js",
            "room-entry.mjs",
            "room-state.mjs",
            "room-connection.mjs",
            "room-controls.mjs",
            "media-session.mjs",
            "signaling-client.mjs",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/assets/{asset}"))
                        .body(Body::empty())
                        .expect("构造静态资源请求"),
                )
                .await
                .expect("读取静态资源响应");

            assert_eq!(response.status(), StatusCode::OK, "asset {asset}");
        }
    }
}
