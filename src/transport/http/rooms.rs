use crate::{
    Result,
    domain::room::{Room, RoomSummary},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rooms", get(list_rooms))
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

async fn list_rooms(State(state): State<AppState>) -> Result<Json<Vec<RoomSummary>>> {
    Ok(Json(state.rooms.list_room_summaries()?))
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
    async fn 可以查询房间列表和人数() {
        let state = AppState::new(8).expect("创建应用状态");
        let first = state.rooms.create_room("房主 1").expect("创建房间");
        state
            .rooms
            .join_room(&first.room.id, "成员 1")
            .expect("加入房间");
        let second = state.rooms.create_room("房主 2").expect("创建第二个房间");
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/rooms")
                    .body(Body::empty())
                    .expect("构造查询房间列表请求"),
            )
            .await
            .expect("查询房间列表响应");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("读取房间列表响应体");
        let rooms: serde_json::Value = serde_json::from_slice(&body).expect("列表响应是 JSON");
        let rooms = rooms.as_array().expect("列表响应是数组");

        assert!(rooms.iter().any(|room| {
            room["id"] == first.room.id
                && room["member_count"] == 2
                && room.get("members").is_none()
        }));
        assert!(rooms.iter().any(|room| {
            room["id"] == second.room.id
                && room["member_count"] == 1
                && room.get("members").is_none()
        }));
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
        assert_eq!(create_response.status(), StatusCode::METHOD_NOT_ALLOWED);

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
