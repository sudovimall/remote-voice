# Chat Mentions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured @ mentions to room chat, including mention validation, highlighted message rendering, and a 10-second in-app mention reminder when another member mentions the current user outside the chat panel.

**Architecture:** Keep chat message storage in `RoomStore` as the source of truth. Extend the websocket chat protocol with optional `mentions`, and keep frontend mention parsing in `chat-controls.mjs` pure helpers with `room.js` owning DOM behavior.

**Tech Stack:** Rust/Axum websocket signaling, serde, in-memory room domain state, vanilla ES modules, Node test runner, CSS.

---

### Task 1: Backend Mention Model And Domain Validation

**Files:**
- Modify: `src/domain/room.rs`

- [ ] **Step 1: Write failing domain tests**

Add tests in `src/domain/room.rs` that call `RoomStore::send_chat_message(room_id, sender_id, content, mentions)` and assert: valid mentions are saved with service-side nicknames, missing mentions default to empty, self mentions are dropped, duplicates are deduped, and unknown member IDs are rejected.

- [ ] **Step 2: Run domain tests to verify RED**

Run: `cargo test domain::room`

Expected: compile failure or test failure because `ChatMention` and the new `send_chat_message` signature do not exist.

- [ ] **Step 3: Implement domain support**

Add `ChatMention`, add `mentions: Vec<ChatMention>` to `ChatMessage`, update `RoomStore::send_chat_message` to accept mentions, validate room members, drop self mentions, dedupe by `member_id`, and replace client nicknames with current member nicknames.

- [ ] **Step 4: Run domain tests to verify GREEN**

Run: `cargo test domain::room`

Expected: PASS.

### Task 2: WebSocket Protocol Mentions

**Files:**
- Modify: `src/transport/http/signaling.rs`
- Modify: `tests/signaling_ws.rs`

- [ ] **Step 1: Write failing websocket/protocol tests**

Add serde tests for `send_chat_message` with `mentions` and websocket integration assertions that `chat_message_sent` / `chat_message` include `mentions`.

- [ ] **Step 2: Run signaling tests to verify RED**

Run: `cargo test signaling`

Expected: failure because the client signal does not parse or forward `mentions`.

- [ ] **Step 3: Implement protocol forwarding**

Add `mentions: Vec<ChatMention>` with serde default to `ClientSignal::SendChatMessage`, pass mentions to `RoomStore::send_chat_message`, and preserve backward compatibility when the field is omitted.

- [ ] **Step 4: Run signaling tests to verify GREEN**

Run: `cargo test signaling`

Expected: PASS.

### Task 3: Frontend Pure Mention Helpers

**Files:**
- Modify: `static/chat-controls.mjs`
- Modify: `tests/frontend/chat-controls.test.mjs`

- [ ] **Step 1: Write failing helper tests**

Add tests for `mentionCandidates`, `insertMentionText`, `mentionsForSend`, `messageMentionsCurrentMember`, and mention highlighting view data.

- [ ] **Step 2: Run helper tests to verify RED**

Run: `node --test tests/frontend/chat-controls.test.mjs`

Expected: failure because helper functions do not exist.

- [ ] **Step 3: Implement helpers**

Implement the helpers in `static/chat-controls.mjs`, reusing `membersForRoom` ordering semantics where practical and keeping all output as plain data.

- [ ] **Step 4: Run helper tests to verify GREEN**

Run: `node --test tests/frontend/chat-controls.test.mjs`

Expected: PASS.

### Task 4: Frontend RoomConnection Protocol

**Files:**
- Modify: `static/room-connection.mjs`
- Modify: `tests/frontend/room-connection.test.mjs`

- [ ] **Step 1: Write failing protocol test**

Update `RoomConnection.sendChatMessage` tests to expect `mentions` in the outgoing signal and confirmed messages to retain mentions.

- [ ] **Step 2: Run connection tests to verify RED**

Run: `node --test tests/frontend/room-connection.test.mjs`

Expected: failure because `sendChatMessage` ignores mentions.

- [ ] **Step 3: Implement protocol argument**

Change `sendChatMessage(content, requestId, mentions = [])` to include `mentions` only when non-empty.

- [ ] **Step 4: Run connection tests to verify GREEN**

Run: `node --test tests/frontend/room-connection.test.mjs`

Expected: PASS.

### Task 5: Chat UI Mention Picker, Highlighting, And Reminder

**Files:**
- Modify: `static/room.html`
- Modify: `static/room.js`
- Modify: `static/styles.css`
- Modify: `tests/frontend/room-layout.test.mjs`

- [ ] **Step 1: Write failing layout/static tests**

Add assertions for mention picker markup, mention reminder markup, highlight classes, and no browser prompt/alert usage.

- [ ] **Step 2: Run frontend static tests to verify RED**

Run: `node --test tests/frontend/room-layout.test.mjs`

Expected: failure because markup/classes do not exist.

- [ ] **Step 3: Implement UI behavior**

Add mention picker DOM, selected mention state, input handling, message rendering with mention spans, and reminder timer behavior. Reminder shows only for another member mentioning the current user while chat is closed, clears when chat opens, and auto-clears after 10 seconds.

- [ ] **Step 4: Run frontend static tests to verify GREEN**

Run: `node --test tests/frontend/room-layout.test.mjs`

Expected: PASS.

### Task 6: Full Verification

**Files:**
- Verify all touched files.

- [ ] **Step 1: Run frontend tests**

Run: `node --test tests/frontend/*.test.mjs`

Expected: PASS.

- [ ] **Step 2: Run Rust tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 3: Review diff**

Run: `git diff --stat` and `git diff --check`

Expected: no whitespace errors and only mention-related changes plus the new plan.
