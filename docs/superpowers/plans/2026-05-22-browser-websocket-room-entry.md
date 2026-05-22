# Browser WebSocket Room Entry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move browser room creation and joining onto WebSocket, persist the lobby nickname locally, and leave a reusable browser signaling client for later room features.

**Architecture:** Restore the existing browser page baseline from the recent room-state commit, then move membership writes behind the WebSocket protocol. The room page owns the socket that creates or joins the room, while the lobby stores only a persistent nickname and a one-shot session entry intent. Browser signaling transport logic lives in a module with no DOM dependency so request/response routing and event fanout stay reusable and testable.

**Tech Stack:** Rust 2024, Axum WebSocket, Serde JSON, native HTML/CSS/JavaScript ES modules, Node test runner, Cargo tests.

---

## File Structure

- Modify `src/transport/http/signaling.rs` for `create_room` handling and socket room binding.
- Modify `tests/signaling_ws.rs` for WebSocket creation and member-join notification coverage.
- Modify `src/transport/http/rooms.rs` to remove HTTP room write routes and keep room lookup.
- Modify `src/transport/http/mod.rs` to serve browser pages and the new browser modules.
- Create or modify `static/index.html`, `static/room.html`, `static/styles.css`, `static/lobby.js`, and `static/room.js` for the lobby and room UI entry flow.
- Create `static/room-entry.mjs` for nickname persistence and one-shot entry intents.
- Create `static/signaling-client.mjs` for DOM-free WebSocket signaling behavior.
- Modify `static/room-state.mjs` for create/join signal constructors and room snapshot helpers.
- Add or modify `tests/frontend/room-entry.test.mjs`, `tests/frontend/signaling-client.test.mjs`, and `tests/frontend/room-state.test.mjs`.

### Task 1: WebSocket Creates Rooms

**Files:**
- Modify: `src/transport/http/signaling.rs`
- Modify: `tests/signaling_ws.rs`

- [ ] **Step 1: Write the failing WebSocket creation tests**

Add tests that connect to `/ws`, send `{"type":"create_room","request_id":"create-1","nickname":"房主"}`, assert the first `joined_room` has the request ID, generated room ID, owner member snapshot, and then join another socket with the returned room ID and assert the creator receives `member_joined`.

- [ ] **Step 2: Run the focused test and verify red**

Run:

```bash
cargo test websocket_创建房间 --test signaling_ws -- --nocapture
```

Expected: FAIL because `create_room` is not a `ClientSignal` yet.

- [ ] **Step 3: Implement the minimal WebSocket protocol path**

Add `ClientSignal::CreateRoom { request_id, nickname }`, create the room from `RoomStore`, register the new owner with `SignalHub`, store `joined_room_id` and `joined_member_id`, and reply with `ServerSignal::JoinedRoom`.

- [ ] **Step 4: Run the focused WebSocket tests and verify green**

Run:

```bash
cargo test websocket_创建房间 --test signaling_ws -- --nocapture
```

Expected: PASS.

### Task 2: Browser Entry State And Reusable Signaling Module

**Files:**
- Create: `static/room-entry.mjs`
- Create: `static/signaling-client.mjs`
- Modify: `static/room-state.mjs`
- Add: `tests/frontend/room-entry.test.mjs`
- Add: `tests/frontend/signaling-client.test.mjs`
- Modify: `tests/frontend/room-state.test.mjs`

- [ ] **Step 1: Write failing Node module tests**

Cover nickname load/save, create and join entry-intent round trips, join intent rejection for a mismatched route, `createRoomSignal` and `joinRoomSignal`, signaling request resolution by `request_id`, and broadcast event delivery for `member_joined`.

- [ ] **Step 2: Run the frontend tests and verify red**

Run:

```bash
node --test tests/frontend/*.test.mjs
```

Expected: FAIL because the room entry and signaling client modules do not exist yet and the room-state create signal is missing.

- [ ] **Step 3: Implement DOM-free modules**

Use `room-entry.mjs` for `remote-voice.nickname` and `remote-voice.room-entry-intent`. Use `signaling-client.mjs` to manage socket open, request IDs, pending request promises, JSON parse failures, close/error hooks, and broadcast listeners without touching DOM nodes.

- [ ] **Step 4: Run the frontend tests and verify green**

Run:

```bash
node --test tests/frontend/*.test.mjs
```

Expected: PASS.

### Task 3: Wire Lobby And Room Pages To WebSocket Entry

**Files:**
- Modify: `src/transport/http/mod.rs`
- Modify: `src/transport/http/rooms.rs`
- Create or modify: `static/index.html`
- Create or modify: `static/room.html`
- Create or modify: `static/styles.css`
- Modify: `static/lobby.js`
- Modify: `static/room.js`

- [ ] **Step 1: Write failing route and endpoint tests**

Route tests must assert pages and JavaScript modules are served. Room HTTP tests must stop relying on `POST /api/rooms` and instead create fixture rooms through `RoomStore` before testing `GET /api/rooms/:room_id`.

- [ ] **Step 2: Run focused HTTP tests and verify red**

Run:

```bash
cargo test transport::http -- --nocapture
```

Expected: FAIL until browser assets are present in this checkout and the old HTTP write routes are removed from tests.

- [ ] **Step 3: Wire the pages**

Update lobby JS to persist nickname and write a create or join intent before navigation. Update room JS to consume the intent, ask `SignalingClient` to create or join, clear the intent after `joined_room`, replace `/rooms/new` with the generated room ID, and keep room snapshot rendering driven by broadcasts.

- [ ] **Step 4: Remove HTTP write routes**

Keep `GET /api/rooms/{room_id}` and delete the `POST /api/rooms` and `POST /api/rooms/{room_id}/join` handlers and route registration.

- [ ] **Step 5: Run the focused HTTP tests and verify green**

Run:

```bash
cargo test transport::http -- --nocapture
```

Expected: PASS.

### Task 4: Verify And联调

**Files:**
- Verify repository and running server state.

- [ ] **Step 1: Run backend and frontend automated checks**

Run:

```bash
node --test tests/frontend/*.test.mjs
cargo test
```

Expected: both commands exit 0.

- [ ] **Step 2: Start the Rust server**

Run:

```bash
cargo run
```

Expected: the server binds the configured local port.

- [ ] **Step 3: Run browser room-flow QA**

Use the existing browser QA harness or a small Playwright script to create a room from one page, join from a second page, and assert the owner member list gains the second nickname. Also inspect nickname persistence and the created room URL after `/rooms/new`.

- [ ] **Step 4: Record outcomes**

Report the exact verification commands, the local URL, and any remaining gaps such as refresh rejoin not being supported by this design.
