# SFU 信令语义修正 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 WebSocket 信令从成员间转发语义修正为 SFU 语义：浏览器只与 Rust 后端协商 WebRTC，不向其他成员直连转发 offer/answer/ICE。

**Architecture:** WebSocket 继续只传 JSON 信令，不传 RTP/SRTP 媒体包。媒体层先以 `MediaController` 接口占位，信令层调用该接口处理 offer 和 ICE；当前实现返回 `media_not_ready`，下一阶段再用 `webrtc` crate 替换为真实 PeerConnection。这样可以先消除错误的 P2P 信令模型，同时给媒体层留下清晰接口。

**Tech Stack:** Rust 2024、Tokio、Axum WebSocket、Serde JSON。

---

## 文件结构

- 修改 `src/error.rs`：增加 `MediaNotReady` 错误码，供媒体层尚未接入时返回。
- 创建 `src/media/mod.rs`：定义 `MediaController`，提供 MVP 占位实现。
- 修改 `src/lib.rs`：导出 `media` 模块。
- 修改 `src/state.rs`：保存共享 `MediaController`。
- 修改 `src/transport/http/signaling.rs`：移除成员间 WebRTC 信令转发，改为调用后端媒体控制器。
- 修改 `tests/signaling_ws.rs`：更新 WebRTC 信令测试，验证不再接受 `target_member_id` 直连转发。

## Task 1: 媒体控制器占位接口

**Files:**
- Modify: `src/error.rs`
- Create: `src/media/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/state.rs`

- [ ] **Step 1: 写失败测试**

在 `src/media/mod.rs` 写单元测试：

```rust
#[cfg(test)]
mod tests {
    use super::MediaController;
    use crate::Error;

    #[tokio::test]
    async fn 占位媒体控制器返回_media_not_ready() {
        let media = MediaController::new();

        let err = media
            .handle_offer("room-1", "member-1", "v=0".to_string())
            .await
            .expect_err("真实媒体层接入前不生成 answer");

        assert!(matches!(err, Error::MediaNotReady));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test media`

Expected: 编译失败，提示 `media` 模块、`MediaController` 或 `MediaNotReady` 不存在。

- [ ] **Step 3: 实现最小媒体接口**

实现：

```rust
use crate::{Error, Result};

#[derive(Debug, Default)]
pub struct MediaController;

impl MediaController {
    pub fn new() -> Self {
        Self
    }

    pub async fn handle_offer(
        &self,
        _room_id: &str,
        _member_id: &str,
        _sdp: String,
    ) -> Result<String> {
        Err(Error::MediaNotReady)
    }

    pub async fn add_ice_candidate(
        &self,
        _room_id: &str,
        _member_id: &str,
        _candidate: String,
    ) -> Result<()> {
        Err(Error::MediaNotReady)
    }

    pub async fn close_member(&self, _room_id: &str, _member_id: &str) -> Result<()> {
        Ok(())
    }
}
```

在 `src/error.rs` 添加：

```rust
#[error("媒体层尚未就绪")]
MediaNotReady,
```

并映射到 `service_unavailable` / `StatusCode::SERVICE_UNAVAILABLE`。

在 `src/lib.rs` 增加：

```rust
pub mod media;
```

在 `src/state.rs` 增加：

```rust
pub media: Arc<MediaController>,
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test media`

Expected: 媒体接口测试通过。

## Task 2: 修正 WebRTC 信令语义

**Files:**
- Modify: `src/transport/http/signaling.rs`
- Modify: `tests/signaling_ws.rs`

- [ ] **Step 1: 写失败测试**

更新 `tests/signaling_ws.rs`：

```rust
#[tokio::test]
async fn websocket_webrtc_offer_由后端媒体层处理而不是转发给成员() {
    let state = AppState::new(8);
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let ws_url = spawn_app(state).await;

    let (mut member_a_ws, _) = connect_join(&ws_url, &room_id, "join-a", "成员 A").await;
    let (mut member_b_ws, _) = connect_join(&ws_url, &room_id, "join-b", "成员 B").await;
    let _ = read_until_type(&mut member_a_ws, "member_joined").await;

    member_a_ws
        .send(Message::Text(
            json!({
                "type": "webrtc_offer",
                "request_id": "offer-1",
                "sdp": "v=0\r\n"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 webrtc_offer");

    let error = read_until_type(&mut member_a_ws, "error").await;
    assert_eq!(error["request_id"], "offer-1");
    assert_eq!(error["code"], "media_not_ready");

    let member_b_message = timeout(Duration::from_millis(200), member_b_ws.next()).await;
    assert!(
        member_b_message.is_err(),
        "成员 B 不应收到成员 A 的 webrtc_offer: {member_b_message:?}"
    );
}

#[tokio::test]
async fn websocket_webrtc_offer_携带目标成员字段会被拒绝() {
    let state = AppState::new(8);
    let created = state.rooms.create_room("房主").expect("创建房间");
    let room_id = created.room.id.clone();
    let ws_url = spawn_app(state).await;

    let (mut ws, member_id) = connect_join(&ws_url, &room_id, "join-a", "成员 A").await;

    ws.send(Message::Text(
        json!({
            "type": "webrtc_offer",
            "request_id": "offer-target",
            "target_member_id": member_id,
            "sdp": "v=0\r\n"
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送带 target_member_id 的 webrtc_offer");

    let error = read_until_type(&mut ws, "error").await;
    assert_eq!(error["request_id"], "offer-target");
    assert_eq!(error["code"], "invalid_message");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test signaling_ws`

Expected: 旧实现仍会解析/转发 target，测试失败。

- [ ] **Step 3: 修改协议类型和处理逻辑**

在 `ClientSignal` 上添加：

```rust
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
```

把客户端 WebRTC 消息改成：

```rust
WebrtcOffer {
    request_id: Option<String>,
    sdp: String,
},
WebrtcAnswer {
    request_id: Option<String>,
    sdp: String,
},
IceCandidate {
    request_id: Option<String>,
    candidate: String,
},
```

把服务端 WebRTC 消息改成：

```rust
WebrtcAnswer { sdp: String },
IceCandidate { candidate: String },
```

处理逻辑：

- `WebrtcOffer`：要求已加入房间，调用 `state.media.handle_offer(room_id, member_id, sdp).await`。成功时向当前 socket 返回 `ServerSignal::WebrtcAnswer { sdp: answer }`，失败时向当前 socket 返回错误。
- `IceCandidate`：要求已加入房间，调用 `state.media.add_ice_candidate(room_id, member_id, candidate).await`。失败时向当前 socket 返回错误。
- `WebrtcAnswer`：当前阶段服务端不会主动发 offer，收到客户端 answer 时返回 `invalid_message`。
- 关闭 socket 清理时调用 `state.media.close_member(&room_id, &member_id).await`，不要因为媒体清理失败影响房间清理。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --test signaling_ws`

Expected: WebSocket 集成测试通过。

## Task 3: 全量验证和提交前停下

**Files:**
- No new files.

- [ ] **Step 1: 运行格式化**

Run: `cargo fmt`

Expected: 格式化完成，无错误。

- [ ] **Step 2: 运行所有测试**

Run: `cargo test`

Expected: 所有测试通过。

- [ ] **Step 3: 提交前说明**

不要自动执行 `git commit`，先说明本阶段修改文件、测试结果和剩余限制，等待用户确认。
