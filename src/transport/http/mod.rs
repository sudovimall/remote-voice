use crate::{
    Result,
    domain::room::{Member, Room, RoomJoin},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub mod signaling;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(signaling::websocket))
        .route("/api/rooms", post(create_room))
        .route("/api/rooms/{room_id}", get(get_room))
        .route("/api/rooms/{room_id}/join", post(join_room))
        .route(
            "/api/rooms/{room_id}/members/{member_id}/speaking",
            post(set_member_can_speak),
        )
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Debug, Deserialize)]
struct CreateRoomRequest {
    nickname: String,
}

#[derive(Debug, Deserialize)]
struct JoinRoomRequest {
    nickname: String,
}

#[derive(Debug, Deserialize)]
struct SetSpeakingRequest {
    actor_member_id: String,
    can_speak: bool,
}

#[derive(Debug, Serialize)]
struct RoomJoinResponse {
    room: Room,
    member: Member,
}

impl From<RoomJoin> for RoomJoinResponse {
    fn from(join: RoomJoin) -> Self {
        Self {
            room: join.room,
            member: join.member,
        }
    }
}

async fn create_room(
    State(state): State<AppState>,
    Json(payload): Json<CreateRoomRequest>,
) -> Result<(StatusCode, Json<RoomJoinResponse>)> {
    let join = state.rooms.create_room(payload.nickname)?;
    Ok((StatusCode::CREATED, Json(join.into())))
}

async fn join_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(payload): Json<JoinRoomRequest>,
) -> Result<Json<RoomJoinResponse>> {
    let join = state.rooms.join_room(&room_id, payload.nickname)?;
    Ok(Json(join.into()))
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
        let app = router(AppState::new(8).expect("创建应用状态"));

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rooms")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"nickname":"房主"}"#))
                    .expect("构造创建房间请求"),
            )
            .await
            .expect("创建房间响应");

        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = to_bytes(create_response.into_body(), 1024 * 1024)
            .await
            .expect("读取创建房间响应体");
        let created: serde_json::Value =
            serde_json::from_slice(&create_body).expect("创建房间响应是 JSON");
        let room_id = created["room"]["id"].as_str().expect("响应包含房间 ID");

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
        assert_eq!(
            room["members"][created["member"]["id"].as_str().unwrap()]["nickname"],
            "房主"
        );
    }
}
