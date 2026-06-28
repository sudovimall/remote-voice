use crate::{
    Error, Result,
    domain::room::{Room, RoomSummary},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rooms", get(list_rooms))
        .route("/api/rooms/{room_id}", get(get_room))
}

async fn get_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
) -> Result<Json<Room>> {
    if state.auth.is_enabled() {
        super::auth::api_user(&state, &headers)?;
    }
    Ok(Json(state.rooms.get_room(&room_id)?))
}

async fn list_rooms(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<RoomSummary>>> {
    if state.auth.is_enabled() {
        super::auth::api_user(&state, &headers)?;
        let mut summaries = state.rooms.list_room_summaries()?;
        let service = state.auth.service().ok_or(Error::AuthDisabled)?;
        for persistent in service.store().list_open_persistent_rooms()? {
            if summaries
                .iter()
                .any(|summary| summary.id == persistent.room_id)
            {
                continue;
            }
            summaries.push(RoomSummary {
                id: persistent.room_id,
                member_count: 0,
            });
        }
        summaries.sort_by(|left, right| left.id.cmp(&right.id));
        return Ok(Json(summaries));
    }
    Ok(Json(state.rooms.list_room_summaries()?))
}

#[cfg(test)]
mod tests {
    use super::router;
    use crate::{
        auth::{
            AuthRuntime, password::hash_password, service::AuthService, session::now_epoch_seconds,
        },
        state::AppState,
        storage::sqlite::SqliteStore,
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    fn auth_state() -> (AppState, Arc<AuthService>) {
        let store = Arc::new(SqliteStore::open_in_memory().expect("打开内存数据库"));
        let service = Arc::new(AuthService::new_for_test(store, 24));
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        let state =
            AppState::new_with_auth(8, AuthRuntime::Enabled(service.clone())).expect("创建状态");
        (state, service)
    }

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
    async fn 认证开启时房间列表包含持久化空房间() {
        let (state, service) = auth_state();
        let login = service
            .login_at("admin", "secret", now_epoch_seconds())
            .expect("管理员登录");
        service
            .store()
            .create_persistent_room("ABC123", login.user.id, now_epoch_seconds())
            .expect("创建持久房间");
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/rooms")
                    .header(
                        header::COOKIE,
                        format!("remote_voice_session={}", login.token),
                    )
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
            room["id"] == "ABC123" && room["member_count"] == 0 && room.get("members").is_none()
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

    #[tokio::test]
    async fn http_发言权限接口不再接受客户端伪造_actor() {
        let state = AppState::new(8).expect("创建应用状态");
        let owner = state.rooms.create_room("房主").expect("创建房间");
        let member = state
            .rooms
            .join_room(&owner.room.id, "成员")
            .expect("成员加入");
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/rooms/{}/members/{}/speaking",
                        owner.room.id, member.member.id
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "actor_member_id": owner.member.id,
                            "can_speak": false
                        })
                        .to_string(),
                    ))
                    .expect("构造发言权限请求"),
            )
            .await
            .expect("读取发言权限响应");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
