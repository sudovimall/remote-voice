# WebSocket 信令 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 增加 MVP WebSocket 信令层，让客户端能加入/离开房间、广播成员状态，并交换 WebRTC 协商所需的 SDP/ICE JSON 消息。

**Architecture:** HTTP 层继续保持轻量，只负责升级 WebSocket 和调用信令服务。信令协议类型放在 `transport::http::signaling`，房间成员状态仍由 `domain::room::RoomStore` 维护，WebSocket 会话只保存连接和消息广播状态。本阶段不引入 `webrtc` crate，不传输 RTP/SRTP 媒体包；真正的音频媒体包必须在后续 WebRTC PeerConnection 建立后走 WebRTC 媒体通道，WebSocket 只承载 offer、answer、ICE candidate 等信令 JSON。

**Tech Stack:** Rust 2024、Tokio、Axum WebSocket、Serde JSON、futures-util。

---

## 文件结构

- 修改 `Cargo.toml`：打开 `axum` 的 `ws` 特性，增加 `futures-util`。
- 修改 `src/state.rs`：保存 `RoomStore` 和信令广播中心。
- 修改 `src/domain/room.rs`：增加成员离开、成员自静音、房主离开关闭房间等领域操作。
- 创建 `src/transport/http/signaling.rs`：定义信令消息 JSON 协议和 WebSocket 会话处理。
- 修改 `src/transport/http/mod.rs`：挂载 `GET /ws`，导出 `signaling` 模块。
- 创建 `tests/signaling_ws.rs`：覆盖 WebSocket 加入房间和成员状态广播。

## 边界说明

- WebSocket 不传麦克风音频，不传 RTP/SRTP，不作为媒体隧道。
- WebSocket 只传控制和协商消息：加入/离开房间、成员状态、`webrtc_offer`、`webrtc_answer`、`ice_candidate`。
- `webrtc_offer`、`webrtc_answer`、`ice_candidate` 里的 SDP 或 candidate 是建立 PeerConnection 的信令数据，不是 WebRTC 媒体包。
- 媒体转发阶段需要单独接入 `webrtc` crate，由 Rust 后端终止 PeerConnection 并转发 RTP；这不属于本计划。

## Task 1: 领域层补齐成员状态操作

**Files:**
- Modify: `src/domain/room.rs`
- Modify: `tests/room_permissions.rs`

- [ ] **Step 1: 写失败测试**

在 `tests/room_permissions.rs` 追加：

```rust
#[test]
fn 成员可以更新自己的本地静音状态() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");

    let room = store
        .set_self_muted(&owner.room.id, &owner.member.id, true)
        .expect("成员可以更新自己的静音状态");

    assert!(room.members[&owner.member.id].self_muted);
}

#[test]
fn 普通成员离开后房间保留() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    let room = store
        .leave_room(&owner.room.id, &member.member.id)
        .expect("普通成员可以离开");

    assert!(room.members.contains_key(&owner.member.id));
    assert!(!room.members.contains_key(&member.member.id));
}

#[test]
fn 房主离开后关闭房间() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");

    store
        .leave_room(&owner.room.id, &owner.member.id)
        .expect("房主可以离开并关闭房间");

    let err = store
        .get_room(&owner.room.id)
        .expect_err("房主离开后房间不存在");

    assert!(matches!(err, Error::RoomNotFound));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test room_permissions`

Expected: 编译失败，提示 `set_self_muted` 和 `leave_room` 未定义。

- [ ] **Step 3: 实现最小领域方法**

在 `impl RoomStore` 中增加：

```rust
pub fn set_self_muted(&self, room_id: &str, member_id: &str, self_muted: bool) -> Result<Room> {
    let mut rooms = self.write_rooms()?;
    let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
    let member = room.members.get_mut(member_id).ok_or(Error::MemberNotFound)?;

    member.self_muted = self_muted;
    room.last_active_epoch_seconds = now_epoch_seconds();

    Ok(room.clone())
}

pub fn leave_room(&self, room_id: &str, member_id: &str) -> Result<Room> {
    let mut rooms = self.write_rooms()?;
    let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

    if room.owner_member_id == member_id {
        let closed = room.clone();
        rooms.remove(room_id);
        return Ok(closed);
    }

    if room.members.remove(member_id).is_none() {
        return Err(Error::MemberNotFound);
    }

    room.last_active_epoch_seconds = now_epoch_seconds();
    Ok(room.clone())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --test room_permissions`

Expected: 所有房间领域测试通过。

## Task 2: 信令协议类型

**Files:**
- Modify: `Cargo.toml`
- Create: `src/transport/http/signaling.rs`
- Modify: `src/transport/http/mod.rs`

- [ ] **Step 1: 写失败测试**

在 `src/transport/http/signaling.rs` 中创建协议和测试骨架：

```rust
#[cfg(test)]
mod tests {
    use super::{ClientSignal, ServerSignal};

    #[test]
    fn 客户端信令消息按_type_字段解析() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"join_room","request_id":"req-1","room_id":"ABC123","nickname":"小明"}"#,
        )
        .expect("解析 join_room 信令");

        assert!(matches!(
            signal,
            ClientSignal::JoinRoom {
                request_id,
                room_id,
                nickname
            } if request_id == "req-1" && room_id == "ABC123" && nickname == "小明"
        ));
    }

    #[test]
    fn 服务端错误信令包含请求_id() {
        let json = serde_json::to_value(ServerSignal::Error {
            request_id: Some("req-1".to_string()),
            code: "room_not_found",
            message: "房间不存在".to_string(),
        })
        .expect("序列化 error 信令");

        assert_eq!(json["type"], "error");
        assert_eq!(json["request_id"], "req-1");
        assert_eq!(json["code"], "room_not_found");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test transport::http::signaling`

Expected: 编译失败，提示 `signaling` 模块或信令类型不存在。

- [ ] **Step 3: 增加依赖和模块导出**

修改 `Cargo.toml`：

```toml
axum = { version = "0.8", features = ["ws"] }
futures-util = "0.3"
```

在 `src/transport/http/mod.rs` 顶部增加：

```rust
mod signaling;
```

- [ ] **Step 4: 实现协议类型**

在 `src/transport/http/signaling.rs` 写入：

```rust
use crate::domain::room::Room;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientSignal {
    JoinRoom {
        request_id: String,
        room_id: String,
        nickname: String,
    },
    LeaveRoom {
        request_id: Option<String>,
    },
    SetSelfMuted {
        request_id: Option<String>,
        self_muted: bool,
    },
    SetMemberCanSpeak {
        request_id: Option<String>,
        member_id: String,
        can_speak: bool,
    },
    WebrtcOffer {
        request_id: Option<String>,
        target_member_id: String,
        sdp: String,
    },
    WebrtcAnswer {
        request_id: Option<String>,
        target_member_id: String,
        sdp: String,
    },
    IceCandidate {
        request_id: Option<String>,
        target_member_id: String,
        candidate: String,
    },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerSignal {
    JoinedRoom {
        request_id: String,
        room: Room,
        member_id: String,
    },
    MemberJoined {
        room: Room,
        member_id: String,
    },
    MemberLeft {
        room: Room,
        member_id: String,
    },
    RoomClosed {
        room_id: String,
    },
    MemberUpdated {
        room: Room,
        member_id: String,
    },
    WebrtcOffer {
        from_member_id: String,
        sdp: String,
    },
    WebrtcAnswer {
        from_member_id: String,
        sdp: String,
    },
    IceCandidate {
        from_member_id: String,
        candidate: String,
    },
    Error {
        request_id: Option<String>,
        code: &'static str,
        message: String,
    },
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test transport::http::signaling`

Expected: 信令协议序列化测试通过。

## Task 3: WebSocket 加入房间和广播

**Files:**
- Modify: `src/state.rs`
- Modify: `src/transport/http/signaling.rs`
- Modify: `src/transport/http/mod.rs`
- Create: `tests/signaling_ws.rs`

- [ ] **Step 1: 写失败测试**

创建 `tests/signaling_ws.rs`：

```rust
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use voice::{app::build_router, state::AppState};

async fn spawn_app() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定测试端口");
    let addr = listener.local_addr().expect("读取测试地址");
    let app = build_router(AppState::new(8));

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("测试服务运行");
    });

    format!("ws://{addr}/ws")
}

#[tokio::test]
async fn websocket_加入房间后收到_joined_room() {
    let ws_url = spawn_app().await;
    let state = AppState::new(8);
    let created = state.rooms.create_room("房主").expect("创建房间");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定测试端口");
    let addr = listener.local_addr().expect("读取测试地址");
    let app = build_router(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("测试服务运行");
    });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("连接 ws");

    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        json!({
            "type": "join_room",
            "request_id": "req-1",
            "room_id": created.room.id,
            "nickname": "队友"
        })
        .to_string(),
    ))
    .await
    .expect("发送 join_room");

    let message = ws.next().await.expect("收到消息").expect("消息有效");
    let body: serde_json::Value =
        serde_json::from_str(message.to_text().expect("文本消息")).expect("JSON 消息");

    assert_eq!(body["type"], "joined_room");
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["room"]["id"], created.room.id);
}
```

同时在 `Cargo.toml` 增加测试依赖：

```toml
tokio-tungstenite = "0.28"
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test signaling_ws`

Expected: 编译失败或 `GET /ws` 返回 404，因为 WebSocket 路由尚未实现。

- [ ] **Step 3: 增加信令广播状态**

在 `src/state.rs` 中加入：

```rust
use crate::{config::settings::Settings, domain::room::RoomStore, transport::http::signaling::SignalHub};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppState {
    pub rooms: Arc<RoomStore>,
    pub signals: Arc<SignalHub>,
}

impl AppState {
    pub fn new(max_members: usize) -> Self {
        Self {
            rooms: Arc::new(RoomStore::new(max_members)),
            signals: Arc::new(SignalHub::new()),
        }
    }

    pub fn from_settings(settings: &Settings) -> Self {
        Self::new(settings.room.max_members)
    }
}
```

- [ ] **Step 4: 实现 `/ws` 路由和最小会话**

在 `src/transport/http/mod.rs` 中导出模块并挂载路由：

```rust
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
```

在 `src/transport/http/signaling.rs` 中追加会话处理：

```rust
use crate::{Error, Result, state::AppState};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tokio::sync::broadcast;

#[derive(Debug)]
pub struct SignalHub {
    rooms: RwLock<HashMap<String, broadcast::Sender<ServerSignal>>>,
}

impl SignalHub {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
        }
    }

    fn room_sender(&self, room_id: &str) -> Result<broadcast::Sender<ServerSignal>> {
        let mut rooms = self
            .rooms
            .write()
            .map_err(|_| Error::Internal("信令房间写锁已损坏".to_string()))?;

        Ok(rooms
            .entry(room_id.to_string())
            .or_insert_with(|| broadcast::channel(128).0)
            .clone())
    }
}

pub async fn websocket(
    State(state): State<AppState>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: AppState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let mut joined_room_id: Option<String> = None;
    let mut joined_member_id: Option<String> = None;
    let mut room_events: Option<broadcast::Receiver<ServerSignal>> = None;

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(Ok(Message::Text(text))) = incoming else {
                    break;
                };

                let signal = match serde_json::from_str::<ClientSignal>(&text) {
                    Ok(signal) => signal,
                    Err(error) => {
                        let _ = send_json(&mut sender, &ServerSignal::Error {
                            request_id: None,
                            code: "invalid_message",
                            message: format!("消息格式无效: {error}"),
                        }).await;
                        continue;
                    }
                };

                match signal {
                    ClientSignal::JoinRoom { request_id, room_id, nickname } => {
                        match state.rooms.join_room(&room_id, nickname) {
                            Ok(join) => {
                                let room_sender = match state.signals.room_sender(&room_id) {
                                    Ok(sender) => sender,
                                    Err(error) => {
                                        let _ = send_error(&mut sender, Some(request_id), error).await;
                                        continue;
                                    }
                                };
                                room_events = Some(room_sender.subscribe());
                                joined_room_id = Some(room_id.clone());
                                joined_member_id = Some(join.member.id.clone());

                                let _ = send_json(&mut sender, &ServerSignal::JoinedRoom {
                                    request_id,
                                    room: join.room.clone(),
                                    member_id: join.member.id.clone(),
                                }).await;

                                let _ = room_sender.send(ServerSignal::MemberJoined {
                                    room: join.room,
                                    member_id: join.member.id,
                                });
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                            }
                        }
                    }
                    ClientSignal::LeaveRoom { .. } => break,
                    ClientSignal::SetSelfMuted { self_muted, .. } => {
                        if let (Some(room_id), Some(member_id)) = (&joined_room_id, &joined_member_id) {
                            if let Ok(room) = state.rooms.set_self_muted(room_id, member_id, self_muted) {
                                if let Ok(room_sender) = state.signals.room_sender(room_id) {
                                    let _ = room_sender.send(ServerSignal::MemberUpdated {
                                        room,
                                        member_id: member_id.clone(),
                                    });
                                }
                            }
                        }
                    }
                    ClientSignal::SetMemberCanSpeak { member_id, can_speak, .. } => {
                        if let (Some(room_id), Some(actor_id)) = (&joined_room_id, &joined_member_id) {
                            if let Ok(room) = state.rooms.set_member_can_speak(room_id, actor_id, &member_id, can_speak) {
                                if let Ok(room_sender) = state.signals.room_sender(room_id) {
                                    let _ = room_sender.send(ServerSignal::MemberUpdated { room, member_id });
                                }
                            }
                        }
                    }
                    ClientSignal::WebrtcOffer { target_member_id: _, sdp, .. } => {
                        if let (Some(room_id), Some(member_id)) = (&joined_room_id, &joined_member_id) {
                            if let Ok(room_sender) = state.signals.room_sender(room_id) {
                                let _ = room_sender.send(ServerSignal::WebrtcOffer {
                                    from_member_id: member_id.clone(),
                                    sdp,
                                });
                            }
                        }
                    }
                    ClientSignal::WebrtcAnswer { target_member_id: _, sdp, .. } => {
                        if let (Some(room_id), Some(member_id)) = (&joined_room_id, &joined_member_id) {
                            if let Ok(room_sender) = state.signals.room_sender(room_id) {
                                let _ = room_sender.send(ServerSignal::WebrtcAnswer {
                                    from_member_id: member_id.clone(),
                                    sdp,
                                });
                            }
                        }
                    }
                    ClientSignal::IceCandidate { target_member_id: _, candidate, .. } => {
                        if let (Some(room_id), Some(member_id)) = (&joined_room_id, &joined_member_id) {
                            if let Ok(room_sender) = state.signals.room_sender(room_id) {
                                let _ = room_sender.send(ServerSignal::IceCandidate {
                                    from_member_id: member_id.clone(),
                                    candidate,
                                });
                            }
                        }
                    }
                }
            }
            event = async {
                match &mut room_events {
                    Some(events) => events.recv().await.ok(),
                    None => std::future::pending().await,
                }
            } => {
                if let Some(event) = event {
                    let _ = send_json(&mut sender, &event).await;
                }
            }
        }
    }

    if let (Some(room_id), Some(member_id)) = (joined_room_id, joined_member_id) {
        if let Ok(room) = state.rooms.leave_room(&room_id, &member_id) {
            if let Ok(room_sender) = state.signals.room_sender(&room_id) {
                if room.owner_member_id == member_id {
                    let _ = room_sender.send(ServerSignal::RoomClosed { room_id });
                } else {
                    let _ = room_sender.send(ServerSignal::MemberLeft { room, member_id });
                }
            }
        }
    }
}

async fn send_json(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    signal: &ServerSignal,
) -> Result<()> {
    let text = serde_json::to_string(signal)
        .map_err(|error| Error::Internal(format!("序列化信令失败: {error}")))?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| Error::Internal(format!("发送信令失败: {error}")))?;
    Ok(())
}

async fn send_error(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    request_id: Option<String>,
    error: Error,
) -> Result<()> {
    send_json(
        sender,
        &ServerSignal::Error {
            request_id,
            code: error.code(),
            message: error.to_string(),
        },
    )
    .await
}
```

- [ ] **Step 5: 暴露错误代码**

把 `src/error.rs` 里的 `fn code(&self) -> &'static str` 改为：

```rust
pub fn code(&self) -> &'static str {
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --test signaling_ws`

Expected: WebSocket 加入房间测试通过。

## Task 4: 全量验证和提交前停下

**Files:**
- No new files.

- [ ] **Step 1: 运行格式化**

Run: `cargo fmt`

Expected: 格式化完成，无错误。

- [ ] **Step 2: 运行所有测试**

Run: `cargo test`

Expected: 所有测试通过。

- [ ] **Step 3: 提交前说明**

不要自动执行 `git commit`。先向用户说明本阶段修改文件、测试结果和剩余限制，等待用户明确确认后再提交。
