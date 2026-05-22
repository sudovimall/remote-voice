# Room Chat And WebSocket Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add room-scoped text chat with configurable in-memory history, a polished chat/member toggle UI, and a room-level WebSocket wrapper.

**Architecture:** `RoomStore` owns chat history because it already owns room lifetime and resume state. WebSocket signaling adds chat messages and includes history on join/resume. Frontend keeps `SignalingClient` as the low-level transport and adds `RoomConnection` for room protocol events, while `room.js` handles UI state and rendering.

**Tech Stack:** Rust, Axum WebSocket, serde JSON, browser ES modules, Node test runner.

---

## File Map

- Modify `src/config/settings.rs`: add `room.chat_history_limit` default/config logging.
- Modify `src/domain/room.rs`: add `ChatMessage`, chat history storage, validation, truncation.
- Modify `tests/room_permissions.rs`: cover domain chat semantics.
- Modify `src/transport/http/signaling.rs`: add chat protocol messages and history on `joined_room`.
- Modify `tests/signaling_ws.rs`: cover chat WebSocket behavior.
- Create `static/room-connection.mjs`: room-level WebSocket protocol wrapper.
- Create `tests/frontend/room-connection.test.mjs`: wrapper behavior tests.
- Create `static/chat-controls.mjs`: DOM-free chat helper functions.
- Create `tests/frontend/chat-controls.test.mjs`: chat helper tests.
- Modify `static/room.html`: add chat view containers.
- Modify `static/room.js`: use `RoomConnection`, render chat/member toggle, unread count, messages and input.
- Modify `static/styles.css`: style chat view and triangular toggle button.
- Modify `src/transport/http/mod.rs`: serve new frontend assets.

## Task 1: Backend Config And Domain Chat History

**Files:**
- Modify: `src/config/settings.rs`
- Modify: `src/domain/room.rs`
- Modify: `tests/room_permissions.rs`

- [ ] **Step 1: Write failing domain tests**

Add tests to `tests/room_permissions.rs`:

```rust
#[test]
fn 房间聊天会保存最近消息并裁剪历史() {
    let store = RoomStore::new(8).with_chat_history_limit(2);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    let first = store
        .send_chat_message(&owner.room.id, &owner.member.id, "第一条")
        .expect("发送第一条");
    let second = store
        .send_chat_message(&owner.room.id, &member.member.id, " 第二条 ")
        .expect("发送第二条");
    let third = store
        .send_chat_message(&owner.room.id, &owner.member.id, "第三条")
        .expect("发送第三条");

    assert!(first.id.starts_with("c_"));
    assert_eq!(second.content, "第二条");
    assert_eq!(second.nickname, "队友");
    assert!(third.sent_at_epoch_millis >= second.sent_at_epoch_millis);

    let history = store.chat_history(&owner.room.id).expect("读取历史");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "第二条");
    assert_eq!(history[1].content, "第三条");
}

#[test]
fn 房间聊天拒绝空消息和超长消息() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");

    let empty = store
        .send_chat_message(&owner.room.id, &owner.member.id, "   ")
        .expect_err("空消息应拒绝");
    assert!(matches!(empty, Error::InvalidMessage(_)));

    let too_long = "a".repeat(501);
    let error = store
        .send_chat_message(&owner.room.id, &owner.member.id, &too_long)
        .expect_err("超长消息应拒绝");
    assert!(matches!(error, Error::InvalidMessage(_)));
}
```

- [ ] **Step 2: Run domain tests for RED**

Run:

```bash
cargo test --test room_permissions 房间聊天
```

Expected: FAIL because `with_chat_history_limit`, `send_chat_message`, and `chat_history` do not exist.

- [ ] **Step 3: Implement config and domain chat history**

Update `src/config/settings.rs`:

```rust
pub struct RoomSettings {
    #[serde(default = "default_max_members")]
    pub max_members: usize,
    #[serde(default = "default_disconnect_grace_seconds")]
    pub disconnect_grace_seconds: u64,
    #[serde(default = "default_chat_history_limit")]
    pub chat_history_limit: usize,
}

fn default_chat_history_limit() -> usize {
    100
}
```

Include `chat_history_limit` in `Default`, `Settings::Display`, default YAML, and settings tests.

Update `src/domain/room.rs`:

```rust
const CHAT_MESSAGE_ID_LENGTH: usize = 22;
const CHAT_MESSAGE_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub room_id: String,
    pub member_id: String,
    pub nickname: String,
    pub content: String,
    pub sent_at_epoch_millis: u64,
}
```

Add `chat_messages: Vec<ChatMessage>` to `Room` with `#[serde(skip, default)]`. Add `chat_history_limit: usize` to `RoomStore`, `with_chat_history_limit`, `send_chat_message`, and `chat_history`. Generate IDs with a `c_` prefix and random alphanumeric suffix. Use `SystemTime` milliseconds for `sent_at_epoch_millis`.

- [ ] **Step 4: Run config/domain tests for GREEN**

Run:

```bash
cargo test config::settings::tests
cargo test --test room_permissions 房间聊天
```

Expected: PASS.

- [ ] **Step 5: Commit backend domain**

```bash
git add src/config/settings.rs src/domain/room.rs tests/room_permissions.rs
git commit -m "feat: store room chat history"
```

## Task 2: WebSocket Chat Protocol

**Files:**
- Modify: `src/transport/http/signaling.rs`
- Modify: `tests/signaling_ws.rs`

- [ ] **Step 1: Write failing WebSocket tests**

Add tests to `tests/signaling_ws.rs`:

```rust
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
                "content": " 晚上打哪张图？ "
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
    assert_eq!(sent["message"]["content"], "晚上打哪张图？");

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

    let (mut member_ws, _member_id) = connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let joined = read_until_type(&mut member_ws, "joined_room").await;
    assert_eq!(joined["chat_messages"][0]["content"], "历史消息");
}
```

- [ ] **Step 2: Run WebSocket tests for RED**

Run:

```bash
cargo test --test signaling_ws websocket_聊天
cargo test --test signaling_ws websocket_joined_room_返回聊天历史
```

Expected: FAIL because chat protocol is missing.

- [ ] **Step 3: Implement chat signal handling**

Add `SendChatMessage { request_id: Option<String>, content: String }` to `ClientSignal`.

Add `chat_messages: Vec<ChatMessage>` to `ServerSignal::JoinedRoom`, plus:

```rust
ChatMessageSent {
    request_id: Option<String>,
    message: ChatMessage,
},
ChatMessage {
    message: ChatMessage,
},
```

On create/join/resume, pass `state.rooms.chat_history(&room_id)?` to `JoinedRoom`.

In `handle_socket`, handle `SendChatMessage` only after a member has joined. Call `state.rooms.send_chat_message`, send `ChatMessageSent` to current sender, and broadcast `ChatMessage` to all other room members.

- [ ] **Step 4: Run WebSocket tests for GREEN**

Run:

```bash
cargo test --test signaling_ws websocket_聊天
cargo test --test signaling_ws websocket_joined_room_返回聊天历史
cargo test --test signaling_ws
```

Expected: PASS.

- [ ] **Step 5: Commit WebSocket chat**

```bash
git add src/transport/http/signaling.rs tests/signaling_ws.rs
git commit -m "feat: add room chat signaling"
```

## Task 3: Frontend RoomConnection Wrapper

**Files:**
- Create: `static/room-connection.mjs`
- Create: `tests/frontend/room-connection.test.mjs`
- Modify: `src/transport/http/mod.rs`

- [ ] **Step 1: Write failing RoomConnection tests**

Create `tests/frontend/room-connection.test.mjs` with fake `SignalingClient` coverage for `enter`, `sendChatMessage`, `onChatMessage`, and `onMediaSignal`.

- [ ] **Step 2: Run tests for RED**

Run:

```bash
node --test tests/frontend/room-connection.test.mjs
```

Expected: FAIL because `static/room-connection.mjs` does not exist.

- [ ] **Step 3: Implement RoomConnection**

Create `static/room-connection.mjs` exporting `RoomConnection`. It wraps `SignalingClient`, provides room-level methods, and routes `chat_message`, `chat_message_sent`, `ice_candidate`, `renegotiation_needed`, `member_listening_updated`, `room_closed`, `member_joined`, `member_left`, `member_updated`, and `error` to typed listener sets.

Serve `room-connection.mjs` from `src/transport/http/mod.rs`.

- [ ] **Step 4: Run wrapper tests for GREEN**

Run:

```bash
node --test tests/frontend/room-connection.test.mjs tests/frontend/signaling-client.test.mjs
cargo test transport::http::tests::页面静态模块可以访问
```

Expected: PASS.

- [ ] **Step 5: Commit wrapper**

```bash
git add static/room-connection.mjs tests/frontend/room-connection.test.mjs src/transport/http/mod.rs
git commit -m "feat: wrap room websocket protocol"
```

## Task 4: Chat Helpers And Room UI

**Files:**
- Create: `static/chat-controls.mjs`
- Create: `tests/frontend/chat-controls.test.mjs`
- Modify: `static/room.html`
- Modify: `static/room.js`
- Modify: `static/styles.css`
- Modify: `src/transport/http/mod.rs`

- [ ] **Step 1: Write failing chat helper tests**

Create `tests/frontend/chat-controls.test.mjs` covering `chatMessageSignal`, `trimChatContent`, `canSendChatMessage`, `chatMessageTimeLabel`, and `chatAvatarText`.

- [ ] **Step 2: Run helper tests for RED**

Run:

```bash
node --test tests/frontend/chat-controls.test.mjs
```

Expected: FAIL because `static/chat-controls.mjs` does not exist.

- [ ] **Step 3: Implement chat helpers and serve asset**

Create `static/chat-controls.mjs` with DOM-free helpers and add it to `src/transport/http/mod.rs`.

- [ ] **Step 4: Wire room UI**

Update `static/room.html` with chat view containers inside the members pane. Update `static/room.js` to use `RoomConnection`, maintain `activePanel`, `chatMessages`, `unreadChatCount`, render triangular toggle, render messages, send on Enter, and clear unread count when opening chat. Update CSS for triangular button, chat list, message rows, avatars, timestamps, and input.

- [ ] **Step 5: Run frontend tests for GREEN**

Run:

```bash
node --test tests/frontend/*.test.mjs
cargo test transport::http::tests::页面静态模块可以访问
```

Expected: PASS.

- [ ] **Step 6: Commit frontend chat UI**

```bash
git add static/chat-controls.mjs tests/frontend/chat-controls.test.mjs static/room.html static/room.js static/styles.css src/transport/http/mod.rs
git commit -m "feat: add room chat interface"
```

## Task 5: Final Verification

- [ ] **Step 1: Run all Rust tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 2: Run all frontend tests**

Run:

```bash
node --test tests/frontend/*.test.mjs
```

Expected: PASS.

- [ ] **Step 3: Run local service smoke**

Run:

```bash
cargo run
```

Expected logs include Chinese config and startup messages. Visit `/rooms/NEW` via the normal lobby flow and verify chat/member toggle, unread count, and message rendering manually if browser access is available.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short
git diff --stat HEAD
```

Expected: only known untracked `.idea/`, previous plan doc, and archive files remain if they were pre-existing; implementation changes are committed.
