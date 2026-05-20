use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use voice::{app::build_router, state::AppState};

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

async fn connect_existing_member(
    ws_url: &str,
    room_id: &str,
    request_id: &str,
    member_id: &str,
    nickname: &str,
) -> TestWebSocket {
    let (mut ws, _) = connect_async(ws_url).await.expect("连接 ws");

    ws.send(Message::Text(
        json!({
            "type": "join_room",
            "request_id": request_id,
            "room_id": room_id,
            "member_id": member_id,
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

#[tokio::test]
async fn websocket_加入房间后收到_joined_room() {
    let state = AppState::new(8);
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
async fn websocket_webrtc_offer_只转发给目标成员() {
    let state = AppState::new(8);
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let ws_url = spawn_app(state).await;

    let (mut member_a_ws, member_a_id) = connect_join(&ws_url, &room_id, "join-a", "成员 A").await;
    let (mut member_b_ws, member_b_id) = connect_join(&ws_url, &room_id, "join-b", "成员 B").await;

    let member_joined = read_until_type(&mut member_a_ws, "member_joined").await;
    assert_eq!(member_joined["member_id"], member_b_id);

    member_a_ws
        .send(Message::Text(
            json!({
                "type": "webrtc_offer",
                "request_id": "offer-1",
                "target_member_id": member_b_id,
                "sdp": "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 webrtc_offer");

    let offer = read_until_type(&mut member_b_ws, "webrtc_offer").await;
    assert_eq!(offer["from_member_id"], member_a_id);
    assert_eq!(offer["sdp"], "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\n");

    let sender_message = timeout(Duration::from_millis(200), member_a_ws.next()).await;
    assert!(
        sender_message.is_err(),
        "发送方不应收到自己的定向 offer: {sender_message:?}"
    );
}

#[tokio::test]
async fn websocket_房主用已有成员_id_加入后可以修改成员发言权限() {
    let state = AppState::new(8);
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let owner_member_id = created.member.id.clone();
    let member = state.rooms.join_room(&room_id, "成员").expect("成员加入");
    let member_id = member.member.id.clone();
    let ws_url = spawn_app(state).await;

    let mut owner_ws =
        connect_existing_member(&ws_url, &room_id, "join-owner", &owner_member_id, "房主").await;

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
    let state = AppState::new(8);
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let owner_member_id = created.member.id.clone();
    let member = state.rooms.join_room(&room_id, "成员").expect("成员加入");
    let member_id = member.member.id.clone();
    let ws_url = spawn_app(state).await;

    let mut owner_ws =
        connect_existing_member(&ws_url, &room_id, "join-owner", &owner_member_id, "房主").await;

    let (mut duplicate_ws, _) = connect_async(&ws_url).await.expect("连接重复 ws");
    duplicate_ws
        .send(Message::Text(
            json!({
                "type": "join_room",
                "request_id": "join-duplicate",
                "room_id": room_id,
                "member_id": owner_member_id,
                "nickname": "冒充房主",
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送重复 join_room");

    let duplicate_error = read_json(&mut duplicate_ws).await;
    assert_eq!(duplicate_error["type"], "error");
    assert_eq!(duplicate_error["request_id"], "join-duplicate");
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
