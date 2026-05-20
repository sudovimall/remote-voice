# WebRTC Answer 骨架 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让后端媒体层创建真实 WebRTC PeerConnection，接收客户端 SDP offer 并返回 SDP answer，为后续 RTP 音频转发铺好会话生命周期。

**Architecture:** `MediaController` 负责 WebRTC PeerConnection 生命周期，信令层仍只负责 JSON 消息和调用媒体接口。本阶段只实现 offer/answer 和 remote ICE candidate 接收，不做 RTP 转发、不生成服务端 ICE candidate 下发、不做 track relay。

**Tech Stack:** Rust 2024、Tokio、webrtc-rs `webrtc = 0.17.1`、Serde JSON。

---

## 依据

- `webrtc` latest stable 文档显示当前版本是 `0.17.1`，并提供 `RTCPeerConnection`、`set_remote_description`、`create_answer`、`set_local_description`、`add_ice_candidate` 等 API。
- 项目方已说明 `0.17.x` 进入 feature freeze，后续会迁移到 Sans-I/O 架构；本阶段只做最小 answer 骨架，并把媒体控制器封装在单一模块，降低未来替换成本。

## 文件结构

- 修改 `Cargo.toml` / `Cargo.lock`：增加 `webrtc = "0.17.1"`。
- 修改 `src/media/mod.rs`：从占位实现替换为真实 PeerConnection 会话管理。
- 修改 `tests/signaling_ws.rs`：WebSocket offer 测试改为发送真实 SDP offer，并断言收到 `webrtc_answer`。

## Task 1: 媒体层生成 WebRTC answer

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: 写失败测试**

把 `src/media/mod.rs` 里的占位测试改成：

```rust
#[tokio::test]
async fn 媒体控制器可以根据_offer_生成_answer() {
    let media = MediaController::new().expect("创建媒体控制器");
    let offer_sdp = create_audio_offer().await;

    let answer = media
        .handle_offer("room-1", "member-1", offer_sdp)
        .await
        .expect("根据 offer 生成 answer");

    assert!(answer.contains("m=audio"));
}
```

同时保留一个无效 SDP 测试：

```rust
#[tokio::test]
async fn 无效_offer_返回_invalid_message() {
    let media = MediaController::new().expect("创建媒体控制器");

    let err = media
        .handle_offer("room-1", "member-1", "not sdp".to_string())
        .await
        .expect_err("无效 SDP 应失败");

    assert!(matches!(err, Error::InvalidMessage(_)));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test media`

Expected: 失败，因为 `webrtc` 依赖和真实实现尚未接入。

- [ ] **Step 3: 实现真实媒体控制器**

实现要点：

- `MediaController::new() -> Result<Self>` 初始化 `MediaEngine`、默认 codecs、默认 interceptors 和 `API`。
- `handle_offer`：
  - 用 `RTCSessionDescription::offer(sdp)` 解析客户端 SDP。
  - 创建 `RTCPeerConnection`。
  - `set_remote_description(offer).await`。
  - `create_answer(None).await`。
  - `set_local_description(answer.clone()).await`。
  - 按 `(room_id, member_id)` 保存 `Arc<RTCPeerConnection>`。
  - 返回 `answer.sdp`。
- `add_ice_candidate`：
  - 查找已有 PeerConnection。
  - 用 `RTCIceCandidateInit { candidate, ..Default::default() }` 调用 `add_ice_candidate`。
  - 没有会话时返回 `InvalidMessage("媒体会话不存在，请先发送 offer")`。
- `close_member`：
  - 移除保存的 PeerConnection。
  - 调用 `close().await`，错误转为 `Internal` 或忽略关闭错误。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test media`

Expected: 媒体层测试通过。

## Task 2: WebSocket offer 返回真实 answer

**Files:**
- Modify: `tests/signaling_ws.rs`

- [ ] **Step 1: 更新失败测试**

把 `websocket_webrtc_offer_由后端媒体层处理而不是转发给成员` 调整为：

- 使用测试 helper 生成真实 audio offer SDP。
- 成员 A 发送 `webrtc_offer`。
- 成员 A 收到 `webrtc_answer`，且 `sdp` 包含 `m=audio`。
- 成员 B 不收到成员 A 的 offer。

- [ ] **Step 2: 运行测试确认失败或现有实现不满足**

Run: `cargo test --test signaling_ws`

Expected: 在媒体实现接入前失败；接入后应通过。

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test --test signaling_ws`

Expected: WebSocket offer/answer 流程通过。

## Task 3: 全量验证和提交

**Files:**
- No new files.

- [ ] **Step 1: 运行格式化**

Run: `cargo fmt`

Expected: 无错误。

- [ ] **Step 2: 运行所有测试**

Run: `cargo test`

Expected: 所有测试通过。

- [ ] **Step 3: 提交**

提交信息：

```bash
git commit -m "feat: add webrtc answer skeleton"
```
