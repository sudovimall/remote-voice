use crate::{Result, domain::room::Room, state::AppState};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rooms/{room_id}", get(get_room))
        .route(
            "/api/rooms/{room_id}/members/{member_id}/speaking",
            post(set_member_can_speak),
        )
}

#[derive(Debug, Deserialize)]
struct SetSpeakingRequest {
    actor_member_id: String,
    can_speak: bool,
}

async fn get_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<Room>> {
    Ok(Json(state.rooms.get_room(&room_id)?))
}

async fn set_member_can_speak(
    State(state): State<AppState>,
    Path((room_id, member_id)): Path<(String, String)>,
    Json(payload): Json<SetSpeakingRequest>,
) -> Result<Json<Room>> {
    let room = state.rooms.set_member_can_speak(
        &room_id,
        &payload.actor_member_id,
        &member_id,
        payload.can_speak,
    )?;
    Ok(Json(room))
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
    async fn 创建房间后可以查询房间() {
        let state = AppState::new(8).expect("创建应用状态");
        let created = state.rooms.create_room("房主").expect("创建房间");
        let room_id = created.room.id.clone();
        let app = router().with_state(state);

        let room_response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/rooms/{room_id}"))
                    .body(Body::empty())
                    .expect("构造查询房间请求"),
            )
            .await
            .expect("查询房间响应");

        assert_eq!(room_response.status(), StatusCode::OK);
        let room_body = to_bytes(room_response.into_body(), 1024 * 1024)
            .await
            .expect("读取查询房间响应体");
        let room: serde_json::Value = serde_json::from_slice(&room_body).expect("房间响应是 JSON");

        assert_eq!(room["id"], room_id);
        assert_eq!(room["members"][created.member.id]["nickname"], "房主");
    }

    #[tokio::test]
    async fn http_不再创建或加入房间() {
        let state = AppState::new(8).expect("创建应用状态");
        let created = state.rooms.create_room("房主").expect("创建房间");
        let app = router().with_state(state);

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rooms")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"nickname":"另一个房主"}"#))
                    .expect("构造创建房间请求"),
            )
            .await
            .expect("读取创建房间响应");
        assert_eq!(create_response.status(), StatusCode::NOT_FOUND);

        let join_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/rooms/{}/join", created.room.id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"nickname":"队友"}"#))
                    .expect("构造加入房间请求"),
            )
            .await
            .expect("读取加入房间响应");
        assert_eq!(join_response.status(), StatusCode::NOT_FOUND);
    }
}
