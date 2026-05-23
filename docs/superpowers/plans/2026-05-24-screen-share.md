# Screen Share Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add single-presenter screen sharing to a voice room with no system audio, owner force-stop, panel switching, popout viewing, fullscreen viewing, and video forwarding through the existing SFU.

**Architecture:** Extend the room domain with an optional `screen_share` owner, expose start/stop screen-share signals over the existing WebSocket protocol, and let the media controller accept video tracks only from the current screen-share owner. The browser keeps microphone audio unchanged, adds/removes a display video track during screen sharing, and renders a new `成员 / 聊天 / 共享` panel plus in-page popout.

**Tech Stack:** Rust/Axum, webrtc-rs, vanilla ES modules, browser WebRTC APIs, Node test runner, Rust `cargo test`.

---

### Task 1: Domain Screen Share State

**Files:**
- Modify: `src/domain/room.rs`
- Test: `tests/room_permissions.rs`

- [ ] **Step 1: Write failing Rust tests**

Add tests covering:

```rust
#[test]
fn 同房间只能一个成员共享屏幕() { /* create room, join second, start first, second is rejected */ }

#[test]
fn 房主可以强制停止成员屏幕共享() { /* member starts, owner stops, snapshot has no screen_share */ }

#[test]
fn 普通成员不能停止别人屏幕共享() { /* third member stop fails with NotRoomOwner */ }

#[test]
fn 共享者离开或断线过期后释放共享占用() { /* start share, leave/disconnect cleanup clears screen_share */ }
```

- [ ] **Step 2: Run failing tests**

Run: `cargo test --test room_permissions 屏幕共享 -- --nocapture`

Expected: compile failure because `screen_share`, `start_screen_share`, and `stop_screen_share` do not exist.

- [ ] **Step 3: Implement room state**

Add `ScreenShareState { member_id, nickname }`, `Room.screen_share: Option<ScreenShareState>`, and methods:

```rust
pub fn start_screen_share(&self, room_id: &str, member_id: &str) -> Result<Room>;
pub fn stop_screen_share(&self, room_id: &str, requester_member_id: &str) -> Result<Room>;
pub fn clear_screen_share_for_member(&self, room_id: &str, member_id: &str) -> Result<Option<Room>>;
```

Use `Error::InvalidMessage` for occupied share and `Error::NotRoomOwner` for unauthorized force-stop.

- [ ] **Step 4: Run tests**

Run: `cargo test --test room_permissions`

Expected: all room permission tests pass.

### Task 2: WebSocket Signaling

**Files:**
- Modify: `src/transport/http/signaling.rs`
- Test: `tests/signaling_ws.rs`

- [ ] **Step 1: Write failing WebSocket tests**

Add tests for:

```rust
#[tokio::test]
async fn websocket_开始屏幕共享会广播共享状态() { /* start_screen_share -> screen_share_started */ }

#[tokio::test]
async fn websocket_第二个成员开始屏幕共享会失败() { /* error current sharing */ }

#[tokio::test]
async fn websocket_房主可以强制停止屏幕共享() { /* owner stop -> screen_share_stopped */ }

#[tokio::test]
async fn websocket_joined_room_返回屏幕共享状态() { /* joined_room.room.screen_share exists */ }
```

- [ ] **Step 2: Run failing tests**

Run: `cargo test --test signaling_ws 屏幕共享 -- --nocapture`

Expected: compile failure because signal variants do not exist.

- [ ] **Step 3: Implement signal variants and handlers**

Add client signals `StartScreenShare` and `StopScreenShare`. Add server signals `ScreenShareStarted` and `ScreenShareStopped`. Broadcast state changes to room members. On disconnect/leave, clear active screen share if the leaving member owns it.

- [ ] **Step 4: Run tests**

Run: `cargo test --test signaling_ws`

Expected: all WebSocket tests pass.

### Task 3: Media Controller Video Forwarding

**Files:**
- Modify: `src/media/mod.rs`
- Modify: `src/transport/http/signaling.rs`
- Test: `src/media/mod.rs`

- [ ] **Step 1: Write failing media tests**

Add tests for:

```rust
#[tokio::test]
async fn 非共享者视频_track_不会转发() { /* video from non-owner ignored */ }

#[tokio::test]
async fn 共享者视频_track_会转发给听众() { /* video RTP reaches listener */ }

#[tokio::test]
async fn 停止屏幕共享会清理视频下行_track() { /* video fanout replaced with empty slot */ }
```

- [ ] **Step 2: Run failing tests**

Run: `cargo test media::tests::共享 -- --nocapture`

Expected: compile failure because media controller cannot mark a screen-share owner and snapshots do not distinguish video.

- [ ] **Step 3: Implement media video track routing**

Add media state for current screen-share owners by room. Add methods:

```rust
pub async fn set_screen_share_owner(&self, room_id: &str, member_id: Option<&str>) -> Result<()>;
```

Accept inbound video only when `(room_id, member_id)` matches the screen-share owner. Keep audio behavior unchanged. Add one video downlink slot per session, separate from audio slots.

- [ ] **Step 4: Run media tests**

Run: `cargo test media::tests`

Expected: all media tests pass.

### Task 4: Frontend MediaSession Screen Sharing

**Files:**
- Modify: `static/media-session.mjs`
- Test: `tests/frontend/media-session.test.mjs`

- [ ] **Step 1: Write failing frontend media tests**

Add tests for:

```js
test("media session starts screen share without system audio", async () => {});
test("media session stops display tracks and renegotiates", async () => {});
test("display track ended reports screen share stopped", async () => {});
test("screen share bitrate follows captured resolution", async () => {});
test("remote video track is attached to the screen share video element", async () => {});
```

- [ ] **Step 2: Run failing tests**

Run: `node --test tests/frontend/media-session.test.mjs`

Expected: failures because `startScreenShare`, `stopScreenShare`, and remote video handling do not exist.

- [ ] **Step 3: Implement MediaSession methods**

Add `startScreenShare()`, `stopScreenShare()`, `setScreenShareState()`, `canShareScreen()`, and video track rendering. Use `getDisplayMedia({ video: true, audio: false })`. On sender parameter support, set `maxBitrate` from video settings.

- [ ] **Step 4: Run frontend media tests**

Run: `node --test tests/frontend/media-session.test.mjs`

Expected: all media-session tests pass.

### Task 5: Frontend Room UI

**Files:**
- Modify: `static/room.html`
- Modify: `static/room.js`
- Modify: `static/room-connection.mjs`
- Modify: `static/room-state.mjs`
- Modify: `static/styles.css`
- Test: `tests/frontend/room-layout.test.mjs`
- Test: `tests/frontend/room-connection.test.mjs`
- Test: `tests/frontend/room-state.test.mjs`

- [ ] **Step 1: Write failing UI tests**

Add tests proving:

```js
test("room side panel exposes members chat and screen tabs", () => {});
test("screen panel contains start popout fullscreen and stop controls", () => {});
test("room connection parses screen share signals", () => {});
test("joined room state preserves screen share state", () => {});
```

- [ ] **Step 2: Run failing tests**

Run: `node --test tests/frontend/room-layout.test.mjs tests/frontend/room-connection.test.mjs tests/frontend/room-state.test.mjs`

Expected: failures because screen panel markup and signal parsing do not exist.

- [ ] **Step 3: Implement UI and orchestration**

Add a three-tab panel switch, `#screen-panel`, `#screen-popout`, screen-share buttons, and video containers. In `room.js`, handle `screen_share_started`, `screen_share_stopped`, start/stop button clicks, owner force-stop visibility, popout open/close, and `requestFullscreen()`.

- [ ] **Step 4: Run frontend tests**

Run: `node --test tests/frontend/*.test.mjs`

Expected: all frontend tests pass.

### Task 6: Full Verification

**Files:**
- No new files.

- [ ] **Step 1: Run JavaScript checks**

Run:

```bash
node --check static/media-session.mjs
node --check static/room.js
node --test tests/frontend/*.test.mjs
```

Expected: no syntax errors and all frontend tests pass.

- [ ] **Step 2: Run Rust checks**

Run:

```bash
cargo test
git diff --check
```

Expected: all Rust tests pass and no whitespace errors.

- [ ] **Step 3: Browser smoke**

Run the local server and verify the room page renders with the three panel tabs and screen-share controls. If full `getDisplayMedia` automation is blocked in headless Chrome, verify static layout and signal tests, then document that real display capture needs manual browser confirmation.

