use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use voice::{
    app::build_router,
    auth::{
        AuthRuntime, password::hash_password, service::AuthService, session::now_epoch_seconds,
    },
    state::AppState,
    storage::sqlite::SqliteStore,
};
use webrtc::{
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
    },
    interceptor::registry::Registry,
    peer_connection::configuration::RTCConfiguration,
    rtp_transceiver::rtp_codec::RTPCodecType,
};

type TestWebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn auth_state() -> (AppState, Arc<AuthService>) {
    let store = Arc::new(SqliteStore::open_in_memory().expect("打开内存数据库"));
    let service = Arc::new(AuthService::new_for_test(store, 24));
    let admin_hash = hash_password("secret").expect("生成密码 hash");
    service
        .sync_admin("admin", &admin_hash, "管理员", 10)
        .expect("同步管理员");
    let state =
        AppState::new_with_auth(8, AuthRuntime::Enabled(service.clone())).expect("创建认证状态");
    (state, service)
}

async fn spawn_app(state: AppState) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口");
    let addr = listener.local_addr().expect("读取测试地址");
    let app = build_router(state);

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("测试服务运行");
    });

    format!("ws://{addr}/ws")
}

async fn connect_join(
    ws_url: &str,
    room_id: &str,
    request_id: &str,
    nickname: &str,
) -> (TestWebSocket, String) {
    let (mut ws, _) = connect_async(ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "join_room",
            "request_id": request_id,
            "room_id": room_id,
            "nickname": nickname,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 join_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    assert_eq!(joined["request_id"], request_id);
    assert_eq!(joined["room"]["id"], room_id);
    let member_id = joined["member_id"]
        .as_str()
        .expect("joined_room 包含成员 ID")
        .to_string();

    (ws, member_id)
}

async fn connect_create(
    ws_url: &str,
    request_id: &str,
    nickname: &str,
) -> (TestWebSocket, String, String) {
    let (mut ws, _) = connect_async(ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "create_room",
            "request_id": request_id,
            "nickname": nickname,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 create_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    assert_eq!(joined["request_id"], request_id);
    assert_eq!(joined["room"]["owner_member_id"], joined["member_id"]);
    let room_id = joined["room"]["id"]
        .as_str()
        .expect("joined_room 包含房间 ID")
        .to_string();
    let member_id = joined["member_id"]
        .as_str()
        .expect("joined_room 包含成员 ID")
        .to_string();
    assert_eq!(joined["room"]["members"][&member_id]["nickname"], nickname);

    (ws, room_id, member_id)
}

async fn connect_create_with_cookie(
    ws_url: &str,
    cookie: &str,
    request_id: &str,
    nickname: &str,
) -> (TestWebSocket, String, String) {
    let mut request = ws_url.into_client_request().expect("构造 ws 请求");
    request
        .headers_mut()
        .insert("cookie", cookie.parse().expect("cookie header"));
    let (mut ws, _) = connect_async(request).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "create_room",
            "request_id": request_id,
            "nickname": nickname,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 create_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    let room_id = joined["room"]["id"].as_str().expect("房间 ID").to_string();
    let member_id = joined["member_id"].as_str().expect("成员 ID").to_string();
    (ws, room_id, member_id)
}

async fn connect_join_with_cookie(
    ws_url: &str,
    cookie: &str,
    request_id: &str,
    room_id: &str,
    nickname: &str,
) -> (TestWebSocket, Value) {
    let mut request = ws_url.into_client_request().expect("构造 ws 请求");
    request
        .headers_mut()
        .insert("cookie", cookie.parse().expect("cookie header"));
    let (mut ws, _) = connect_async(request).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "join_room",
            "request_id": request_id,
            "room_id": room_id,
            "nickname": nickname,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 join_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    (ws, joined)
}

async fn connect_create_with_resume(
    ws_url: &str,
    request_id: &str,
    nickname: &str,
) -> (TestWebSocket, String, String, String) {
    let (mut ws, _) = connect_async(ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "create_room",
            "request_id": request_id,
            "nickname": nickname,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 create_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    let room_id = joined["room"]["id"].as_str().expect("房间 ID").to_string();
    let member_id = joined["member_id"].as_str().expect("成员 ID").to_string();
    let resume_token = joined["resume_token"]
        .as_str()
        .expect("恢复凭据")
        .to_string();

    (ws, room_id, member_id, resume_token)
}

async fn connect_existing_member(
    ws_url: &str,
    room_id: &str,
    request_id: &str,
    member_id: &str,
    resume_token: &str,
) -> TestWebSocket {
    let (mut ws, _) = connect_async(ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "resume_room",
            "request_id": request_id,
            "room_id": room_id,
            "member_id": member_id,
            "resume_token": resume_token,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 resume_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    assert_eq!(joined["request_id"], request_id);
    assert_eq!(joined["room"]["id"], room_id);
    assert_eq!(joined["member_id"], member_id);

    ws
}

async fn read_json(ws: &mut TestWebSocket) -> Value {
    let message = timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("等待 ws 消息未超时")
        .expect("收到 ws 消息")
        .expect("ws 消息有效");

    serde_json::from_str(message.to_text().expect("ws 文本消息")).expect("ws 消息是 JSON")
}

async fn read_until_type(ws: &mut TestWebSocket, expected_type: &str) -> Value {
    loop {
        let body = read_json(ws).await;
        if body["type"] == expected_type {
            return body;
        }
    }
}

async fn create_audio_offer() -> String {
    let mut media_engine = MediaEngine::default();
    media_engine
        .register_default_codecs()
        .expect("注册默认 codecs");
    let registry = register_default_interceptors(Registry::new(), &mut media_engine)
        .expect("注册默认 interceptors");
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();
    let peer_connection = api
        .new_peer_connection(RTCConfiguration::default())
        .await
        .expect("创建测试 PeerConnection");
    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Audio, None)
        .await
        .expect("添加 audio transceiver");
    let offer = peer_connection
        .create_offer(None)
        .await
        .expect("创建测试 offer");
    peer_connection
        .set_local_description(offer.clone())
        .await
        .expect("设置测试 local description");
    peer_connection
        .close()
        .await
        .expect("关闭测试 PeerConnection");
    offer.sdp
}

#[tokio::test]
async fn websocket_认证开启时未登录访问_ws_被拒绝() {
    let (state, _service) = auth_state();
    let ws_url = spawn_app(state).await;

    let error = connect_async(ws_url).await.expect_err("未登录不能升级 ws");

    assert!(error.to_string().contains("401"));
}

#[tokio::test]
async fn websocket_认证用户创建房间会写入持久房间() {
    let (state, service) = auth_state();
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    let ws_url = spawn_app(state).await;
    let cookie = format!("remote_voice_session={}", login.token);

    let (_ws, room_id, _member_id) =
        connect_create_with_cookie(&ws_url, &cookie, "create-auth", "管理员").await;

    let persistent = service
        .store()
        .find_open_persistent_room(&room_id)
        .expect("查询持久房间")
        .expect("持久房间存在");
    assert_eq!(persistent.owner_user_id, login.user.id);
}

#[tokio::test]
async fn websocket_认证房主断开会关闭持久房间() {
    let (state, service) = auth_state();
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    let ws_url = spawn_app(state).await;
    let cookie = format!("remote_voice_session={}", login.token);
    let (mut owner_ws, room_id, _owner_id) =
        connect_create_with_cookie(&ws_url, &cookie, "create-auth", "管理员").await;

    owner_ws.close(None).await.expect("关闭房主 ws");
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        service
            .store()
            .find_open_persistent_room(&room_id)
            .expect("查询持久房间")
            .is_none()
    );
}

#[tokio::test]
async fn websocket_认证用户可以加入持久化空房间并恢复运行时房间() {
    let (state, service) = auth_state();
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    service
        .store()
        .create_persistent_room("ABC123", login.user.id, now_epoch_seconds())
        .expect("创建持久房间");
    let ws_url = spawn_app(state).await;
    let mut request = ws_url.into_client_request().expect("构造 ws 请求");
    request.headers_mut().insert(
        "cookie",
        format!("remote_voice_session={}", login.token)
            .parse()
            .expect("cookie header"),
    );
    let (mut ws, _) = connect_async(request).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "join_room",
            "request_id": "join-persistent",
            "room_id": "ABC123",
            "nickname": "管理员",
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 join_room");

    let joined = read_until_type(&mut ws, "joined_room").await;
    assert_eq!(joined["request_id"], "join-persistent");
    assert_eq!(joined["room"]["id"], "ABC123");
    assert_eq!(joined["room"]["owner_member_id"], joined["member_id"]);
}

#[tokio::test]
async fn websocket_持久房间普通用户先加入后房主加入会成为运行时房主() {
    let (state, service) = auth_state();
    let admin = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    let invite = service
        .create_invite_at(&admin.user, 24, now_epoch_seconds())
        .expect("创建邀请码");
    let user = service
        .register_with_invite_at(
            &invite.code,
            "alice",
            "password",
            "Alice",
            now_epoch_seconds(),
        )
        .expect("普通用户注册");
    service
        .store()
        .create_persistent_room("ABC123", admin.user.id, now_epoch_seconds())
        .expect("创建持久房间");
    let ws_url = spawn_app(state).await;
    let user_cookie = format!("remote_voice_session={}", user.token);
    let admin_cookie = format!("remote_voice_session={}", admin.token);

    let (_user_ws, user_joined) =
        connect_join_with_cookie(&ws_url, &user_cookie, "join-user", "ABC123", "Alice").await;
    assert_eq!(user_joined["room"]["owner_member_id"], "");

    let (_admin_ws, admin_joined) =
        connect_join_with_cookie(&ws_url, &admin_cookie, "join-owner", "ABC123", "管理员").await;

    assert_eq!(admin_joined["request_id"], "join-owner");
    assert_eq!(
        admin_joined["room"]["owner_member_id"],
        admin_joined["member_id"]
    );
    assert_eq!(
        admin_joined["room"]["members"][admin_joined["member_id"].as_str().expect("成员 ID")]["role"],
        "owner"
    );
}

#[tokio::test]
async fn websocket_认证用户不能加入已关闭持久房间() {
    let (state, service) = auth_state();
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    service
        .store()
        .create_persistent_room("ABC123", login.user.id, now_epoch_seconds())
        .expect("创建持久房间");
    service
        .store()
        .close_persistent_room("ABC123", now_epoch_seconds())
        .expect("关闭持久房间");
    let ws_url = spawn_app(state).await;
    let mut request = ws_url.into_client_request().expect("构造 ws 请求");
    request.headers_mut().insert(
        "cookie",
        format!("remote_voice_session={}", login.token)
            .parse()
            .expect("cookie header"),
    );
    let (mut ws, _) = connect_async(request).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "join_room",
            "request_id": "join-closed",
            "room_id": "ABC123",
            "nickname": "管理员",
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 join_room");

    let error = read_until_type(&mut ws, "error").await;
    assert_eq!(error["request_id"], "join-closed");
    assert_eq!(error["code"], "room_closed");
}

#[tokio::test]
async fn websocket_加入房间后收到_joined_room() {
    let state = AppState::new(8).expect("创建应用状态");
    let created = state.rooms.create_room("房主").expect("创建房间");
    let ws_url = spawn_app(state).await;

    let (mut ws, _) = connect_async(ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "join_room",
            "request_id": "req-1",
            "room_id": created.room.id,
            "nickname": "队友"
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送 join_room");

    let body = read_json(&mut ws).await;

    assert_eq!(body["type"], "joined_room");
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["room"]["id"], created.room.id);
}

#[tokio::test]
async fn websocket_创建房间后收到_joined_room() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;

    let (_ws, room_id, owner_id) = connect_create(&ws_url, "create-1", "房主").await;

    assert_eq!(room_id.len(), 6);
    assert!(owner_id.starts_with("m_"));
}

#[tokio::test]
async fn websocket_聊天消息会确认给发送者并广播给其他成员() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "send_chat_message",
                "request_id": "chat-1",
                "content": " @队友 晚上打哪张图？ ",
                "mentions": [
                    {
                        "member_id": member_id,
                        "nickname": "伪造昵称"
                    }
                ]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送聊天消息");

    let sent = read_until_type(&mut owner_ws, "chat_message_sent").await;
    assert_eq!(sent["request_id"], "chat-1");
    assert_eq!(sent["message"]["room_id"], room_id);
    assert_eq!(sent["message"]["member_id"], owner_id);
    assert_eq!(sent["message"]["nickname"], "房主");
    assert_eq!(sent["message"]["content"], "@队友 晚上打哪张图？");
    assert_eq!(sent["message"]["mentions"][0]["member_id"], member_id);
    assert_eq!(sent["message"]["mentions"][0]["nickname"], "队友");

    let received = read_until_type(&mut member_ws, "chat_message").await;
    assert_eq!(received["message"], sent["message"]);
    assert_eq!(received["message"]["member_id"], owner_id);
    assert_ne!(received["message"]["member_id"], member_id);
}

#[tokio::test]
async fn websocket_joined_room_返回聊天历史() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "send_chat_message",
                "request_id": "chat-1",
                "content": "历史消息"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送聊天消息");
    let _ = read_until_type(&mut owner_ws, "chat_message_sent").await;

    let (mut member_ws, _) = connect_async(&ws_url).await.expect("连接 ws");
    member_ws
        .send(Message::Text(
            json!({
                "type": "join_room",
                "request_id": "join-member",
                "room_id": room_id,
                "nickname": "队友",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 join_room");
    let joined = read_until_type(&mut member_ws, "joined_room").await;
    assert_eq!(joined["chat_messages"][0]["content"], "历史消息");
}

#[tokio::test]
async fn websocket_开始屏幕共享会广播共享状态() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    member_ws
        .send(Message::Text(
            json!({
                "type": "start_screen_share",
                "request_id": "screen-start",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 start_screen_share");

    let owner_started = read_until_type(&mut owner_ws, "screen_share_started").await;
    assert_eq!(owner_started["member_id"], member_id);
    assert_eq!(owner_started["nickname"], "队友");

    let member_started = read_until_type(&mut member_ws, "screen_share_started").await;
    assert_eq!(member_started["member_id"], member_id);
    assert_eq!(member_started["nickname"], "队友");
}

#[tokio::test]
async fn websocket_屏幕观看状态会广播观看人数() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut sharer_ws, sharer_id) = connect_join(&ws_url, &room_id, "join-sharer", "共享者").await;
    let (mut viewer_ws, _viewer_id) = connect_join(&ws_url, &room_id, "join-viewer", "观众").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut sharer_ws, "member_joined").await;

    sharer_ws
        .send(Message::Text(
            json!({
                "type": "start_screen_share",
                "request_id": "screen-start",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("共享者开始共享");
    let _ = read_until_type(&mut viewer_ws, "screen_share_started").await;
    let _ = read_until_type(&mut sharer_ws, "screen_share_started").await;

    viewer_ws
        .send(Message::Text(
            json!({
                "type": "set_screen_viewing",
                "request_id": "view-start",
                "viewing": true,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("观众开始观看");

    let count = read_until_type(&mut sharer_ws, "screen_share_viewer_count_updated").await;
    assert_eq!(count["member_id"], sharer_id);
    assert_eq!(count["viewer_count"], 1);

    viewer_ws
        .send(Message::Text(
            json!({
                "type": "set_screen_viewing",
                "request_id": "view-stop",
                "viewing": false,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("观众停止观看");

    let count = read_until_type(&mut sharer_ws, "screen_share_viewer_count_updated").await;
    assert_eq!(count["member_id"], sharer_id);
    assert_eq!(count["viewer_count"], 0);
}

#[tokio::test]
async fn websocket_第二个成员开始屏幕共享会失败() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut first_ws, _first_id) = connect_join(&ws_url, &room_id, "join-first", "共享者").await;
    let (mut second_ws, _second_id) = connect_join(&ws_url, &room_id, "join-second", "观众").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut first_ws, "member_joined").await;

    first_ws
        .send(Message::Text(
            json!({
                "type": "start_screen_share",
                "request_id": "screen-first",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("第一个成员开始共享");
    let _ = read_until_type(&mut first_ws, "screen_share_started").await;

    second_ws
        .send(Message::Text(
            json!({
                "type": "start_screen_share",
                "request_id": "screen-second",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("第二个成员开始共享");

    let error = read_until_type(&mut second_ws, "error").await;
    assert_eq!(error["request_id"], "screen-second");
    assert_eq!(error["code"], "invalid_message");
    assert_eq!(error["message"], "消息格式无效: 当前已有成员正在共享屏幕。");
}

#[tokio::test]
async fn websocket_房主可以强制停止屏幕共享() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    member_ws
        .send(Message::Text(
            json!({
                "type": "start_screen_share",
                "request_id": "screen-start",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("成员开始共享");
    let _ = read_until_type(&mut owner_ws, "screen_share_started").await;
    let _ = read_until_type(&mut member_ws, "screen_share_started").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "stop_screen_share",
                "request_id": "screen-stop",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("房主停止共享");

    let owner_stopped = read_until_type(&mut owner_ws, "screen_share_stopped").await;
    assert_eq!(owner_stopped["member_id"], member_id);
    let member_stopped = read_until_type(&mut member_ws, "screen_share_stopped").await;
    assert_eq!(member_stopped["member_id"], member_id);
}

#[tokio::test]
async fn websocket_joined_room_返回屏幕共享状态() {
    let state = AppState::new(8).expect("创建应用状态");
    let owner = state.rooms.create_room("房主").expect("创建房间");
    let sharer = state
        .rooms
        .join_room(&owner.room.id, "共享者")
        .expect("成员加入");
    state
        .rooms
        .start_screen_share(&owner.room.id, &sharer.member.id)
        .expect("成员开始共享");
    let ws_url = spawn_app(state).await;

    let (mut viewer_ws, _) = connect_async(&ws_url).await.expect("连接 ws");
    viewer_ws
        .send(Message::Text(
            json!({
                "type": "join_room",
                "request_id": "join-viewer",
                "room_id": owner.room.id,
                "nickname": "观众",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 join_room");
    let joined = read_until_type(&mut viewer_ws, "joined_room").await;
    assert_eq!(
        joined["room"]["screen_share"]["member_id"],
        sharer.member.id
    );
    assert_eq!(joined["room"]["screen_share"]["nickname"], "共享者");
}

#[tokio::test]
async fn websocket_joined_room_返回恢复凭据() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;

    let (_ws, _room_id, _owner_id, resume_token) =
        connect_create_with_resume(&ws_url, "create-resume", "房主").await;

    assert!(resume_token.starts_with("r_"));
}

#[tokio::test]
async fn websocket_普通成员断线后恢复凭据不能恢复且重新加入生成新成员() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, _) = connect_async(&ws_url).await.expect("连接成员 ws");

    member_ws
        .send(Message::Text(
            json!({
                "type": "join_room",
                "request_id": "join-member",
                "room_id": room_id,
                "nickname": "队友",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 join_room");

    let joined = read_until_type(&mut member_ws, "joined_room").await;
    let member_id = joined["member_id"].as_str().expect("成员 ID").to_string();
    let resume_token = joined["resume_token"]
        .as_str()
        .expect("恢复凭据")
        .to_string();
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    member_ws.close(None).await.expect("关闭成员 ws");

    let (mut resumed_ws, _) = connect_async(&ws_url).await.expect("连接恢复 ws");
    resumed_ws
        .send(Message::Text(
            json!({
                "type": "resume_room",
                "request_id": "resume-member",
                "room_id": room_id,
                "member_id": member_id,
                "resume_token": resume_token,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 resume_room");

    let error = read_until_type(&mut resumed_ws, "error").await;
    assert_eq!(error["request_id"], "resume-member");
    assert_eq!(error["code"], "member_not_found");

    resumed_ws
        .send(Message::Text(
            json!({
                "type": "join_room",
                "request_id": "rejoin-member",
                "room_id": room_id,
                "nickname": "队友",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送重新加入");

    let rejoined = read_until_type(&mut resumed_ws, "joined_room").await;
    assert_eq!(rejoined["request_id"], "rejoin-member");
    assert_ne!(rejoined["member_id"], member_id);
}

#[tokio::test]
async fn websocket_恢复凭据错误时拒绝恢复() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (_owner_ws, room_id, owner_id, _) =
        connect_create_with_resume(&ws_url, "create-owner", "房主").await;
    let (mut resume_ws, _) = connect_async(&ws_url).await.expect("连接恢复 ws");

    resume_ws
        .send(Message::Text(
            json!({
                "type": "resume_room",
                "request_id": "resume-bad",
                "room_id": room_id,
                "member_id": owner_id,
                "resume_token": "bad-token",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送错误 resume_room");

    let error = read_until_type(&mut resume_ws, "error").await;
    assert_eq!(error["request_id"], "resume-bad");
    assert_eq!(error["code"], "invalid_resume_token");
}

#[tokio::test]
async fn websocket_房主断线时立即关闭房间() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state.clone()).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, _) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws.close(None).await.expect("关闭房主 ws");

    let closed = read_until_type(&mut member_ws, "room_closed").await;
    assert_eq!(closed["room_id"], room_id);
    assert!(matches!(
        state.rooms.get_room(&room_id),
        Err(voice::Error::RoomNotFound)
    ));
    assert!(!owner_id.is_empty());
}

#[tokio::test]
async fn websocket_房主显式离开时立即关闭房间() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state.clone()).await;
    let (mut owner_ws, room_id, _) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, _) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "leave_room",
                "request_id": "leave-owner",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 leave_room");

    let closed = read_until_type(&mut member_ws, "room_closed").await;
    assert_eq!(closed["room_id"], room_id);
    assert!(matches!(
        state.rooms.get_room(&room_id),
        Err(voice::Error::RoomNotFound)
    ));
}

#[tokio::test]
async fn websocket_创建房间的房主会收到后续成员加入事件() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;

    let (mut owner_ws, room_id, _) = connect_create(&ws_url, "create-owner", "房主").await;
    let (_member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;

    let joined = read_until_type(&mut owner_ws, "member_joined").await;
    assert_eq!(joined["member_id"], member_id);
    assert_eq!(joined["room"]["members"][&member_id]["nickname"], "队友");
}

#[tokio::test]
async fn websocket_webrtc_offer_由后端媒体层处理而不是转发给成员() {
    let state = AppState::new(8).expect("创建应用状态");
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let ws_url = spawn_app(state).await;

    let (mut member_a_ws, _) = connect_join(&ws_url, &room_id, "join-a", "成员 A").await;
    let (mut member_b_ws, member_b_id) = connect_join(&ws_url, &room_id, "join-b", "成员 B").await;

    let member_joined = read_until_type(&mut member_a_ws, "member_joined").await;
    assert_eq!(member_joined["member_id"], member_b_id);

    let offer_sdp = create_audio_offer().await;

    member_a_ws
        .send(Message::Text(
            json!({
                "type": "webrtc_offer",
                "request_id": "offer-1",
                "sdp": offer_sdp
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 webrtc_offer");

    let answer = read_until_type(&mut member_a_ws, "webrtc_answer").await;
    assert_eq!(answer["request_id"], "offer-1");
    assert!(
        answer["sdp"]
            .as_str()
            .expect("answer 包含 SDP")
            .contains("m=audio")
    );

    let server_candidate = read_until_type(&mut member_a_ws, "ice_candidate").await;
    assert!(
        server_candidate["candidate"]
            .get("candidate")
            .and_then(Value::as_str)
            .expect("服务端 candidate 包含浏览器 candidate 字符串")
            .starts_with("candidate:")
    );

    let forwarded_message = timeout(Duration::from_millis(200), member_b_ws.next()).await;
    assert!(
        forwarded_message.is_err(),
        "成员 B 不应收到成员 A 的 webrtc_offer: {forwarded_message:?}"
    );
}

#[tokio::test]
async fn websocket_webrtc_offer_携带目标成员字段会被拒绝() {
    let state = AppState::new(8).expect("创建应用状态");
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let ws_url = spawn_app(state).await;

    let (mut member_ws, _) = connect_join(&ws_url, &room_id, "join-a", "成员 A").await;

    member_ws
        .send(Message::Text(
            json!({
                "type": "webrtc_offer",
                "request_id": "offer-target",
                "target_member_id": "member-b",
                "sdp": "v=0\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送带 target_member_id 的 webrtc_offer");

    let error = read_until_type(&mut member_ws, "error").await;
    assert_eq!(error["request_id"], "offer-target");
    assert_eq!(error["code"], "invalid_message");
}

#[tokio::test]
async fn websocket_房主用已有成员_id_加入后可以修改成员发言权限() {
    let state = AppState::new(8).expect("创建应用状态");
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let owner_member_id = created.member.id.clone();
    let owner_resume_token = created.resume_token.clone();
    let member = state.rooms.join_room(&room_id, "成员").expect("成员加入");
    let member_id = member.member.id.clone();
    let ws_url = spawn_app(state).await;

    let mut owner_ws = connect_existing_member(
        &ws_url,
        &room_id,
        "resume-owner",
        &owner_member_id,
        &owner_resume_token,
    )
    .await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "set_member_can_speak",
                "request_id": "speak-1",
                "member_id": member_id,
                "can_speak": false,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 set_member_can_speak");

    let updated = read_until_type(&mut owner_ws, "member_updated").await;
    assert_eq!(updated["member_id"], member_id);
    assert_eq!(updated["room"]["members"][&member_id]["can_speak"], false);
}

#[tokio::test]
async fn websocket_监听状态只回给当前听众() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id, _owner_resume_token) =
        connect_create_with_resume(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "set_member_listening",
                "request_id": "listen-off",
                "member_id": member_id,
                "listening": false
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送监听控制");

    let updated = read_until_type(&mut owner_ws, "member_listening_updated").await;
    assert_eq!(updated["request_id"], "listen-off");
    assert_eq!(updated["not_listening_member_ids"], json!([member_id]));
    assert!(
        timeout(Duration::from_millis(200), member_ws.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn websocket_监听控制拒绝屏蔽自己() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut ws, _room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;

    ws.send(Message::Text(
        json!({
            "type": "set_member_listening",
            "request_id": "listen-self",
            "member_id": owner_id,
            "listening": false
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送监听控制");

    let error = read_until_type(&mut ws, "error").await;
    assert_eq!(error["request_id"], "listen-self");
    assert_eq!(error["code"], "invalid_message");
}

#[tokio::test]
async fn websocket_重复绑定同一成员_id_返回错误且原连接仍可用() {
    let state = AppState::new(8).expect("创建应用状态");
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let owner_member_id = created.member.id.clone();
    let owner_resume_token = created.resume_token.clone();
    let member = state.rooms.join_room(&room_id, "成员").expect("成员加入");
    let member_id = member.member.id.clone();
    let ws_url = spawn_app(state).await;

    let mut owner_ws = connect_existing_member(
        &ws_url,
        &room_id,
        "resume-owner",
        &owner_member_id,
        &owner_resume_token,
    )
    .await;

    let (mut duplicate_ws, _) = connect_async(&ws_url).await.expect("连接重复 ws");
    duplicate_ws
        .send(Message::Text(
            json!({
                "type": "resume_room",
                "request_id": "resume-duplicate",
                "room_id": room_id,
                "member_id": owner_member_id,
                "resume_token": owner_resume_token,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送重复 resume_room");

    let duplicate_error = read_json(&mut duplicate_ws).await;
    assert_eq!(duplicate_error["type"], "error");
    assert_eq!(duplicate_error["request_id"], "resume-duplicate");
    assert_eq!(duplicate_error["code"], "invalid_message");

    owner_ws
        .send(Message::Text(
            json!({
                "type": "set_member_can_speak",
                "request_id": "speak-after-duplicate",
                "member_id": member_id,
                "can_speak": false,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("原连接仍可发送 set_member_can_speak");

    let updated = read_until_type(&mut owner_ws, "member_updated").await;
    assert_eq!(updated["member_id"], member_id);
    assert_eq!(updated["room"]["members"][&member_id]["can_speak"], false);
}
