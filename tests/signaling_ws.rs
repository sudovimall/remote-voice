use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use voice::{app::build_router, state::AppState};
use webrtc::{
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
    },
    interceptor::registry::Registry,
    peer_connection::configuration::RTCConfiguration,
    rtp_transceiver::rtp_codec::RTPCodecType,
};

type TestWebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

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
async fn websocket_joined_room_返回恢复凭据() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;

    let (_ws, _room_id, _owner_id, resume_token) =
        connect_create_with_resume(&ws_url, "create-resume", "房主").await;

    assert!(resume_token.starts_with("r_"));
}

#[tokio::test]
async fn websocket_断线后可以通过恢复凭据重新绑定原成员() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, owner_id, resume_token) =
        connect_create_with_resume(&ws_url, "create-owner", "房主").await;

    owner_ws.close(None).await.expect("关闭房主 ws");

    let (mut resumed_ws, _) = connect_async(&ws_url).await.expect("连接恢复 ws");
    resumed_ws
        .send(Message::Text(
            json!({
                "type": "resume_room",
                "request_id": "resume-owner",
                "room_id": room_id,
                "member_id": owner_id,
                "resume_token": resume_token,
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 resume_room");

    let joined = read_until_type(&mut resumed_ws, "joined_room").await;
    assert_eq!(joined["request_id"], "resume-owner");
    assert_eq!(joined["member_id"], owner_id);
    assert_eq!(joined["room"]["members"][&owner_id]["connected"], true);
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
async fn websocket_房主断线时房间先保留并同步离线状态() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state.clone()).await;
    let (mut owner_ws, room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, _) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws.close(None).await.expect("关闭房主 ws");

    let updated = read_until_type(&mut member_ws, "member_updated").await;
    assert_eq!(updated["member_id"], owner_id);
    assert_eq!(updated["room"]["members"][&owner_id]["connected"], false);
    assert!(state.rooms.get_room(&room_id).is_ok());
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
