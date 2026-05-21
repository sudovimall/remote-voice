# Browser Page Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add split lobby and room page skeletons that are served by the Rust app and leave stable UI boundaries for later room, WebSocket, microphone, and WebRTC work.

**Architecture:** Keep the frontend dependency-free for this batch: `static/` owns HTML, CSS, and page-local JavaScript, while the Axum HTTP router serves those files from compile-time includes so page delivery does not add a static-file dependency yet. Route tests assert the public page and asset paths before the page skeleton is filled in.

**Tech Stack:** Rust 2024, Axum 0.8, Tower test utilities, native HTML/CSS/JavaScript.

---

## File Structure

- Create `static/index.html` for the lobby page structure.
- Create `static/room.html` for the room page structure.
- Create `static/styles.css` for shared frontend layout, state styling, controls, and responsive behavior.
- Create `static/lobby.js` for lobby form validation and skeleton-stage status messages.
- Create `static/room.js` for room ID extraction and room-page skeleton initialization.
- Modify `src/transport/http/mod.rs` to serve `/`, `/rooms/{room_id}`, and `/assets/*`.
- Add HTTP route tests in `src/transport/http/mod.rs` so page and asset routes are verified through the app router.

### Task 1: Serve Page And Asset Routes

**Files:**
- Modify: `src/transport/http/mod.rs`
- Create: `static/index.html`
- Create: `static/room.html`
- Create: `static/styles.css`
- Create: `static/lobby.js`
- Create: `static/room.js`

- [ ] **Step 1: Write failing HTTP route tests**

Add tests to `src/transport/http/mod.rs` that build `router(AppState::new(8).expect("创建应用状态"))`, request the public frontend routes, and assert page markers and content types:

```rust
#[tokio::test]
async fn 大厅页和房间页可以访问() {
    let app = super::router(AppState::new(8).expect("创建应用状态"));

    let lobby = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("构造大厅页请求"),
        )
        .await
        .expect("读取大厅页响应");
    assert_eq!(lobby.status(), StatusCode::OK);
    let lobby_body = to_bytes(lobby.into_body(), 1024 * 1024)
        .await
        .expect("读取大厅页响应体");
    assert!(String::from_utf8_lossy(&lobby_body).contains("voice-lobby"));

    let room = app
        .oneshot(
            Request::builder()
                .uri("/rooms/ABC123")
                .body(Body::empty())
                .expect("构造房间页请求"),
        )
        .await
        .expect("读取房间页响应");
    assert_eq!(room.status(), StatusCode::OK);
    let room_body = to_bytes(room.into_body(), 1024 * 1024)
        .await
        .expect("读取房间页响应体");
    assert!(String::from_utf8_lossy(&room_body).contains("voice-room"));
}

#[tokio::test]
async fn 页面静态资源可以访问() {
    let app = super::router(AppState::new(8).expect("创建应用状态"));

    let styles = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/styles.css")
                .body(Body::empty())
                .expect("构造样式请求"),
        )
        .await
        .expect("读取样式响应");
    assert_eq!(styles.status(), StatusCode::OK);
    assert_eq!(
        styles.headers().get("content-type").and_then(|value| value.to_str().ok()),
        Some("text/css; charset=utf-8")
    );

    let room_script = app
        .oneshot(
            Request::builder()
                .uri("/assets/room.js")
                .body(Body::empty())
                .expect("构造房间脚本请求"),
        )
        .await
        .expect("读取房间脚本响应");
    assert_eq!(room_script.status(), StatusCode::OK);
    assert_eq!(
        room_script
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/javascript; charset=utf-8")
    );
}
```

- [ ] **Step 2: Run the route tests and confirm they fail**

Run:

```bash
cargo test transport::http::tests -- --nocapture
```

Expected: FAIL because `/`, `/rooms/ABC123`, `/assets/styles.css`, and `/assets/room.js` are not served yet.

- [ ] **Step 3: Add minimal page and asset routes**

Create placeholder `static/` files with stable page markers:

```html
<!-- static/index.html -->
<!doctype html>
<html lang="zh-CN">
  <body data-page="voice-lobby"></body>
</html>
```

```html
<!-- static/room.html -->
<!doctype html>
<html lang="zh-CN">
  <body data-page="voice-room"></body>
</html>
```

```css
/* static/styles.css */
body {
  margin: 0;
}
```

```javascript
// static/lobby.js
```

```javascript
// static/room.js
```

Serve those files in `src/transport/http/mod.rs` with Axum responses and explicit asset content types:

```rust
use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

fn lobby_page() -> Html<&'static str> {
    Html(include_str!("../../../static/index.html"))
}

fn room_page(Path(_room_id): Path<String>) -> Html<&'static str> {
    Html(include_str!("../../../static/room.html"))
}

fn asset(Path(asset): Path<String>) -> Response {
    match asset.as_str() {
        "styles.css" => (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            include_str!("../../../static/styles.css"),
        )
            .into_response(),
        "lobby.js" => (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            include_str!("../../../static/lobby.js"),
        )
            .into_response(),
        "room.js" => (
            [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
            include_str!("../../../static/room.js"),
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
```

Wire `lobby_page`, `room_page`, and `asset` before `.with_state(state)` in the existing router.

- [ ] **Step 4: Run the route tests and confirm they pass**

Run:

```bash
cargo test transport::http::tests -- --nocapture
```

Expected: PASS.

### Task 2: Build The Lobby Skeleton

**Files:**
- Modify: `static/index.html`
- Modify: `static/styles.css`
- Modify: `static/lobby.js`

- [ ] **Step 1: Expand the lobby HTML**

Replace the placeholder lobby page with a two-section skeleton that keeps the action surface first:

```html
<main class="lobby-shell">
  <section class="brand-rail" aria-labelledby="lobby-title">
    <p class="eyebrow">Remote Voice</p>
    <h1 id="lobby-title">进入语音房间</h1>
    <p class="lede">准备昵称和房间号，房间内的语音控制在下一页完成。</p>
    <div class="stage-strip" aria-live="polite">
      <span class="stage-dot" aria-hidden="true"></span>
      <span id="lobby-status">页面骨架已就绪，房间动作稍后接入。</span>
    </div>
  </section>
  <section class="lobby-actions" aria-label="房间入口">
    <label class="field">
      <span>昵称</span>
      <input id="nickname" name="nickname" autocomplete="nickname" maxlength="32" placeholder="输入昵称">
    </label>
    <div id="lobby-error" class="inline-message" role="status" hidden></div>
    <form id="create-room" class="action-band">
      <h2>创建房间</h2>
      <button type="submit">创建</button>
    </form>
    <form id="join-room" class="action-band join-band">
      <label class="field">
        <span>房间号</span>
        <input id="room-id" name="room_id" autocomplete="off" maxlength="12" placeholder="ABC123">
      </label>
      <button type="submit">加入</button>
    </form>
  </section>
</main>
```

Include `/assets/styles.css` and `/assets/lobby.js`.

- [ ] **Step 2: Add lobby validation behavior**

Use `static/lobby.js` to keep skeleton-stage actions honest:

```javascript
const nickname = document.querySelector("#nickname");
const roomId = document.querySelector("#room-id");
const error = document.querySelector("#lobby-error");
const status = document.querySelector("#lobby-status");

function showError(message) {
  error.textContent = message;
  error.hidden = false;
}

function showPending(message) {
  error.hidden = true;
  status.textContent = message;
}

function requireNickname(event) {
  event.preventDefault();
  if (!nickname.value.trim()) {
    showError("先输入昵称。");
    nickname.focus();
    return;
  }
  showPending("房间接口将在下一阶段接入。");
}

document.querySelector("#create-room").addEventListener("submit", requireNickname);
document.querySelector("#join-room").addEventListener("submit", (event) => {
  event.preventDefault();
  if (!nickname.value.trim()) {
    showError("先输入昵称。");
    nickname.focus();
    return;
  }

  roomId.value = roomId.value.trim().toUpperCase();
  if (!roomId.value) {
    showError("输入房间号后再加入。");
    roomId.focus();
    return;
  }
  showPending("加入流程将在下一阶段接入。");
});
```

- [ ] **Step 3: Add shared layout and lobby styles**

Define CSS variables, calm neutral page surfaces, stable input and button dimensions, action bands, and responsive layout that stacks the two lobby sections on narrow screens.

- [ ] **Step 4: Inspect the lobby in a browser**

Run the Rust server after the routes are available:

```bash
cargo run
```

Open `/` and verify:

- Nickname and room ID controls stay readable at desktop and narrow widths.
- Empty submit shows the inline error area.
- Skeleton-stage submits do not navigate or imply success.

### Task 3: Build The Room Skeleton

**Files:**
- Modify: `static/room.html`
- Modify: `static/styles.css`
- Modify: `static/room.js`

- [ ] **Step 1: Expand the room HTML**

Build the room work surface with topbar, members pane, local controls pane, and fixed state containers:

```html
<main class="room-shell">
  <header class="room-topbar">
    <div>
      <p class="eyebrow">语音房间</p>
      <h1>房间 <span id="room-id">--</span></h1>
    </div>
    <div class="room-meta">
      <span id="room-connection" class="status-pill">未连接</span>
      <a class="quiet-button" href="/">离开</a>
    </div>
  </header>
  <div id="room-error" class="room-alert" role="status" hidden></div>
  <section class="room-grid">
    <section class="members-pane" aria-labelledby="members-title">
      <div class="pane-head">
        <h2 id="members-title">成员</h2>
        <span class="meta-copy">等待房间状态</span>
      </div>
      <div class="member-list" aria-label="成员列表骨架">
        <article class="member-row">...</article>
      </div>
    </section>
    <aside class="voice-pane" aria-labelledby="voice-title">
      <div class="pane-head">
        <h2 id="voice-title">本地语音</h2>
      </div>
      <button class="mic-button" type="button" disabled>麦克风未连接</button>
      <div class="signal-stack">
        <p>设备状态 <strong>未请求权限</strong></p>
        <p>媒体状态 <strong>等待 WebRTC 接入</strong></p>
      </div>
      <div class="permission-note">麦克风权限将在加入真实房间流程后请求。</div>
    </aside>
  </section>
</main>
```

Include `/assets/styles.css` and `/assets/room.js`.

- [ ] **Step 2: Initialize the room ID from the URL**

Use `static/room.js` to parse the room path:

```javascript
const roomIdNode = document.querySelector("#room-id");
const roomError = document.querySelector("#room-error");
const segments = window.location.pathname.split("/").filter(Boolean);
const roomId = segments[0] === "rooms" ? segments[1] : "";

if (roomId) {
  roomIdNode.textContent = decodeURIComponent(roomId).toUpperCase();
} else {
  roomError.hidden = false;
  roomError.textContent = "房间地址缺少房间号。";
}
```

- [ ] **Step 3: Add room styles**

Extend shared CSS with a scan-friendly member table surface, room topbar, status pills, fixed voice control dimensions, disabled owner-control positions, and a mobile layout that moves local voice controls below the member list.

- [ ] **Step 4: Inspect the room page in a browser**

With the Rust server running, open `/rooms/ABC123` and verify:

- The room ID renders from the URL.
- Member and local voice areas remain legible at desktop and narrow widths.
- Disabled controls communicate skeleton state without starting microphone access.

### Task 4: Verify The Page Skeleton Batch

**Files:**
- Verify: `src/transport/http/mod.rs`
- Verify: `static/index.html`
- Verify: `static/room.html`
- Verify: `static/styles.css`
- Verify: `static/lobby.js`
- Verify: `static/room.js`

- [ ] **Step 1: Format Rust code**

Run:

```bash
cargo fmt
```

- [ ] **Step 2: Check the frontend routes**

Run:

```bash
cargo test transport::http::tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run the full test suite**

Run:

```bash
cargo test
```

Expected: all existing Rust unit and integration tests pass.

- [ ] **Step 4: Review the diff**

Run:

```bash
git diff --check
git diff --stat
```

Expected: no whitespace errors; the diff contains only the frontend skeleton plan, static frontend files, and HTTP router changes for this batch.
