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

fn auth_state_with_grace(disconnect_grace_period: Duration) -> (AppState, Arc<AuthService>) {
    let (mut state, service) = auth_state();
    state.disconnect_grace_period = disconnect_grace_period;
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

// 断言连接短时间内没有额外消息，用来验证定向 P2P 信令不会被广播。
async fn assert_no_message(ws: &mut TestWebSocket, reason: &str) {
    let message = timeout(Duration::from_millis(200), ws.next()).await;
    assert!(message.is_err(), "{reason}: {message:?}");
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
    let ws_url = spawn_app(state.clone()).await;

    let error = connect_async(ws_url).await.expect_err("未登录不能升级 ws");

    assert!(error.to_string().contains("401"));
}

#[tokio::test]
async fn websocket_认证用户创建房间会写入持久房间() {
    let (state, service) = auth_state();
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    let ws_url = spawn_app(state.clone()).await;
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
async fn websocket_认证房主普通断开不会立即关闭持久房间() {
    let (state, service) = auth_state_with_grace(Duration::from_millis(250));
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    let ws_url = spawn_app(state.clone()).await;
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
            .is_some()
    );
    assert!(
        !state
            .rooms
            .get_room(&room_id)
            .expect("房间仍处于恢复宽限期")
            .members[&_owner_id]
            .connected
    );
}

#[tokio::test]
async fn websocket_认证房主断线超时后关闭持久房间() {
    let (state, service) = auth_state_with_grace(Duration::from_millis(30));
    let login = service
        .login_at("admin", "secret", now_epoch_seconds())
        .expect("管理员登录");
    let ws_url = spawn_app(state).await;
    let cookie = format!("remote_voice_session={}", login.token);
    let (mut owner_ws, room_id, _owner_id) =
        connect_create_with_cookie(&ws_url, &cookie, "create-auth", "管理员").await;

    owner_ws.close(None).await.expect("关闭房主 ws");
    tokio::time::sleep(Duration::from_millis(120)).await;

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
async fn websocket_视频通话开始停止会广播发布人数() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut first_ws, first_id) = connect_join(&ws_url, &room_id, "join-first", "一号").await;
    let (mut second_ws, second_id) = connect_join(&ws_url, &room_id, "join-second", "二号").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut first_ws, "member_joined").await;

    first_ws
        .send(Message::Text(
            json!({
                "type": "start_video_call",
                "request_id": "video-first",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("一号开启摄像头");
    let started = read_until_type(&mut owner_ws, "video_call_started").await;
    assert_eq!(started["member_id"], first_id);
    assert_eq!(started["nickname"], "一号");
    let count = read_until_type(&mut owner_ws, "video_call_publisher_count_updated").await;
    assert_eq!(count["publisher_count"], 1);

    second_ws
        .send(Message::Text(
            json!({
                "type": "start_video_call",
                "request_id": "video-second",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("二号开启摄像头");
    let started = read_until_type(&mut owner_ws, "video_call_started").await;
    assert_eq!(started["member_id"], second_id);
    let count = read_until_type(&mut owner_ws, "video_call_publisher_count_updated").await;
    assert_eq!(count["publisher_count"], 2);

    first_ws
        .send(Message::Text(
            json!({
                "type": "stop_video_call",
                "request_id": "video-stop",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("一号关闭摄像头");
    let stopped = read_until_type(&mut owner_ws, "video_call_stopped").await;
    assert_eq!(stopped["member_id"], first_id);
    let count = read_until_type(&mut owner_ws, "video_call_publisher_count_updated").await;
    assert_eq!(count["publisher_count"], 1);
}

#[tokio::test]
async fn websocket_joined_room_返回视频通话发布者状态() {
    let state = AppState::new(8).expect("创建应用状态");
    let owner = state.rooms.create_room("房主").expect("创建房间");
    let publisher = state
        .rooms
        .join_room(&owner.room.id, "摄像头")
        .expect("成员加入");
    state
        .rooms
        .start_video_call(&owner.room.id, &publisher.member.id)
        .expect("成员开启摄像头");
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
        joined["room"]["video_call_publishers"][&publisher.member.id]["nickname"],
        "摄像头"
    );
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
async fn websocket_普通成员普通断开后可以在宽限期内恢复() {
    let state = AppState::with_disconnect_grace_period(8, Duration::from_millis(250))
        .expect("创建应用状态");
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
    let member_joined = read_until_type(&mut member_ws, "joined_room").await;
    let member_id = member_joined["member_id"]
        .as_str()
        .expect("成员 ID")
        .to_string();
    let resume_token = member_joined["resume_token"]
        .as_str()
        .expect("恢复凭据")
        .to_string();
    let owner_joined = read_until_type(&mut owner_ws, "member_joined").await;
    let leaked_resume_token = owner_joined["room"]["members"][&member_id]["resume_token"]
        .as_str()
        .map(str::to_string);
    assert!(leaked_resume_token.is_none(), "房间快照不能暴露恢复凭据");

    member_ws.close(None).await.expect("关闭成员 ws");
    let updated = read_until_type(&mut owner_ws, "member_updated").await;
    assert_eq!(updated["member_id"], member_id);
    assert_eq!(updated["room"]["members"][&member_id]["connected"], false);

    let mut resumed_ws = connect_existing_member(
        &ws_url,
        &room_id,
        "resume-member",
        &member_id,
        &resume_token,
    )
    .await;
    let resumed = read_until_type(&mut owner_ws, "member_updated").await;
    assert_eq!(resumed["member_id"], member_id);
    assert_eq!(resumed["room"]["members"][&member_id]["connected"], true);
    resumed_ws.close(None).await.expect("关闭恢复 ws");
}

#[tokio::test]
async fn websocket_普通成员断线超时后恢复凭据不能恢复且重新加入生成新成员() {
    let state =
        AppState::with_disconnect_grace_period(8, Duration::from_millis(30)).expect("创建应用状态");
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
    let left = read_until_type(&mut owner_ws, "member_left").await;
    assert_eq!(left["member_id"], member_id);

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
async fn websocket_房主普通断开后可以在宽限期内恢复() {
    let state = AppState::with_disconnect_grace_period(8, Duration::from_millis(250))
        .expect("创建应用状态");
    let ws_url = spawn_app(state.clone()).await;
    let (mut owner_ws, room_id, owner_id, resume_token) =
        connect_create_with_resume(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws.close(None).await.expect("关闭房主 ws");

    let updated = read_until_type(&mut member_ws, "member_updated").await;
    assert_eq!(updated["member_id"], owner_id);
    assert_eq!(updated["room"]["members"][&owner_id]["connected"], false);

    let mut resumed_ws =
        connect_existing_member(&ws_url, &room_id, "resume-owner", &owner_id, &resume_token).await;
    resumed_ws
        .send(Message::Text(
            json!({
                "type": "set_member_can_speak",
                "request_id": "owner-after-resume",
                "member_id": member_id,
                "can_speak": false,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("恢复后的房主修改权限");

    let owner_update = read_until_type(&mut resumed_ws, "member_updated").await;
    assert_eq!(owner_update["request_id"], Value::Null);
    assert_eq!(owner_update["room"]["owner_member_id"], owner_id);
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
async fn websocket_p2p_offer_只转发给目标成员并替换发送者() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut target_ws, target_id) = connect_join(&ws_url, &room_id, "join-target", "目标").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let (mut third_ws, _third_id) = connect_join(&ws_url, &room_id, "join-third", "旁观者").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut target_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_offer",
                "request_id": "p2p-offer-1",
                "target_member_id": target_id,
                "sdp": "v=0\r\np2p-offer"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 p2p_offer");

    let offer = read_until_type(&mut target_ws, "p2p_offer").await;
    assert_eq!(offer["from_member_id"], owner_id);
    assert_eq!(offer["sdp"], "v=0\r\np2p-offer");
    assert_no_message(&mut third_ws, "第三个成员不应收到 P2P offer").await;
}

#[tokio::test]
async fn websocket_p2p_answer_只转发给目标成员并替换发送者() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "成员").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    member_ws
        .send(Message::Text(
            json!({
                "type": "p2p_answer",
                "request_id": "p2p-answer-1",
                "target_member_id": owner_id,
                "sdp": "v=0\r\np2p-answer"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 p2p_answer");

    let answer = read_until_type(&mut owner_ws, "p2p_answer").await;
    assert_eq!(answer["from_member_id"], member_id);
    assert_eq!(answer["sdp"], "v=0\r\np2p-answer");
}

#[tokio::test]
async fn websocket_p2p_ice_candidate_保留浏览器_candidate_并定向转发() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "成员").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_ice_candidate",
                "request_id": "p2p-ice-1",
                "target_member_id": member_id,
                "candidate": {
                    "candidate": "candidate:p2p",
                    "sdpMid": "0",
                    "sdpMLineIndex": 0,
                    "usernameFragment": "ufrag"
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 p2p_ice_candidate");

    let ice = read_until_type(&mut member_ws, "p2p_ice_candidate").await;
    assert_eq!(ice["from_member_id"], owner_id);
    assert_eq!(ice["candidate"]["candidate"], "candidate:p2p");
    assert_eq!(ice["candidate"]["sdpMid"], "0");
    assert_eq!(ice["candidate"]["sdpMLineIndex"], 0);
    assert_eq!(ice["candidate"]["usernameFragment"], "ufrag");
}

#[tokio::test]
async fn websocket_p2p_connection_failed_广播媒体路由更新() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state.clone()).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut first_ws, first_id) = connect_join(&ws_url, &room_id, "join-first", "一号").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let (mut second_ws, second_id) = connect_join(&ws_url, &room_id, "join-second", "二号").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;
    let _ = read_until_type(&mut first_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_connection_failed",
                "request_id": "p2p-failed-1",
                "target_member_id": first_id,
                "reason": "ice_failed"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 p2p_connection_failed");

    let mut expected_pair = vec![owner_id.clone(), first_id.clone()];
    expected_pair.sort();
    for update in [
        read_until_type(&mut owner_ws, "media_route_updated").await,
        read_until_type(&mut first_ws, "media_route_updated").await,
        read_until_type(&mut second_ws, "media_route_updated").await,
    ] {
        assert_eq!(update["member_ids"], json!(expected_pair));
        assert_eq!(update["route"], "sfu");
        assert_eq!(update["reason"], "p2p_failed");
    }
    assert_eq!(
        state
            .rooms
            .media_route(&room_id, &owner_id, &first_id)
            .expect("读取失败成员对路由"),
        voice::domain::room::MediaRoute::Sfu
    );
    assert_eq!(
        state
            .rooms
            .media_route(&room_id, &owner_id, &second_id)
            .expect("读取未失败成员对路由"),
        voice::domain::room::MediaRoute::P2p
    );
}

#[tokio::test]
async fn websocket_p2p_signal_未加入房间返回错误() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut ws, _) = connect_async(&ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "p2p_offer",
            "request_id": "p2p-not-joined",
            "target_member_id": "m_target",
            "sdp": "v=0\r\n"
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送未加入的 p2p_offer");

    let error = read_until_type(&mut ws, "error").await;
    assert_eq!(error["request_id"], "p2p-not-joined");
    assert_eq!(error["code"], "invalid_message");
    assert_eq!(error["message"], "加入房间后才能发送该消息");
}

#[tokio::test]
async fn websocket_p2p_signal_拒绝自己或未知目标() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, _room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_offer",
                "request_id": "p2p-self",
                "target_member_id": owner_id,
                "sdp": "v=0\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送指向自己的 p2p_offer");
    let self_error = read_until_type(&mut owner_ws, "error").await;
    assert_eq!(self_error["request_id"], "p2p-self");
    assert_eq!(self_error["code"], "invalid_message");

    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_offer",
                "request_id": "p2p-missing",
                "target_member_id": "m_missing",
                "sdp": "v=0\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送指向未知成员的 p2p_offer");
    let missing_error = read_until_type(&mut owner_ws, "error").await;
    assert_eq!(missing_error["request_id"], "p2p-missing");
    assert_eq!(missing_error["code"], "invalid_message");
}

#[tokio::test]
async fn websocket_p2p_signal_拒绝离线或无信令连接目标() {
    let state = AppState::with_disconnect_grace_period(8, Duration::from_millis(250))
        .expect("创建应用状态");
    let ws_url = spawn_app(state.clone()).await;
    let (mut owner_ws, room_id, _owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) = connect_join(&ws_url, &room_id, "join-member", "成员").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    member_ws.close(None).await.expect("关闭成员 ws");
    let _ = read_until_type(&mut owner_ws, "member_updated").await;
    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_offer",
                "request_id": "p2p-offline",
                "target_member_id": member_id,
                "sdp": "v=0\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("向离线成员发送 p2p_offer");
    let offline_error = read_until_type(&mut owner_ws, "error").await;
    assert_eq!(offline_error["request_id"], "p2p-offline");
    assert_eq!(offline_error["code"], "invalid_message");

    let shadow = state
        .rooms
        .join_room(&room_id, "无连接成员")
        .expect("直接加入房间但不注册 ws");
    owner_ws
        .send(Message::Text(
            json!({
                "type": "p2p_offer",
                "request_id": "p2p-no-sender",
                "target_member_id": shadow.member.id,
                "sdp": "v=0\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("向无信令连接成员发送 p2p_offer");
    let no_sender_error = read_until_type(&mut owner_ws, "error").await;
    assert_eq!(no_sender_error["request_id"], "p2p-no-sender");
    assert_eq!(no_sender_error["code"], "invalid_message");
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
