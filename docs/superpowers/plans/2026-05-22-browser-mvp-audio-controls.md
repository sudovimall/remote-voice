# Browser MVP Audio Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the MVP browser audio flow by wiring microphone/WebRTC media, local mute, and owner speak controls into the existing room page and signaling channel.

**Architecture:** Keep `room.js` as the DOM and room snapshot coordinator. Add DOM-free `room-controls.mjs` for permission/control message construction and `media-session.mjs` for browser WebRTC concerns: microphone access, offer/answer, ICE, remote audio nodes, and serialized renegotiation. Existing Rust signaling and SFU code remains the protocol and media authority unless integration tests reveal a concrete gap.

**Tech Stack:** Native browser ES modules, WebRTC APIs, Node test runner, Rust Axum WebSocket signaling, webrtc-rs SFU, Playwright with fake microphone.

---

## File Structure

- Create `static/room-controls.mjs` and `tests/frontend/room-controls.test.mjs`.
- Create `static/media-session.mjs` and `tests/frontend/media-session.test.mjs`.
- Modify `static/signaling-client.mjs` only if media event forwarding needs a transport correction.
- Modify `static/room.js`, `static/room.html`, `static/styles.css`, and `src/transport/http/mod.rs` to wire the visible room controls and serve new modules.
- Keep existing Rust media and signaling tests; add Rust tests only if the browser flow exposes a protocol gap.

### Task 1: DOM-Free Room Controls

**Files:**
- Create: `static/room-controls.mjs`
- Add: `tests/frontend/room-controls.test.mjs`

- [ ] **Step 1: Write failing tests**

Add Node tests for `selfMutedSignal(true)`, `memberCanSpeakSignal("m_1", false)`, `canManageMember(room, ownMemberId, member)`, and `memberPermissionLabel(member)`.

- [ ] **Step 2: Verify red**

Run:

```bash
node --test tests/frontend/room-controls.test.mjs
```

Expected: FAIL because the control module does not exist.

- [ ] **Step 3: Implement control helpers**

Keep the module limited to control JSON and role/member decisions. Return `set_self_muted` and `set_member_can_speak` request bodies without DOM concerns.

- [ ] **Step 4: Verify green**

Run the same Node command and confirm it passes.

### Task 2: Browser Media Session

**Files:**
- Create: `static/media-session.mjs`
- Add: `tests/frontend/media-session.test.mjs`

- [ ] **Step 1: Write failing tests**

Use fake media devices, fake peer connection, fake audio node factory, and a fake signaling client to cover:

- microphone start creates an offer and applies a WebRTC answer;
- local ICE emits an `ice_candidate` request;
- remote service ICE is added to the peer connection;
- repeated renegotiation calls serialize offer work instead of overlapping;
- `setMuted(true)` disables local audio tracks;
- `close()` stops local tracks and removes remote audio nodes.

- [ ] **Step 2: Verify red**

Run:

```bash
node --test tests/frontend/media-session.test.mjs
```

Expected: FAIL because the media session module does not exist.

- [ ] **Step 3: Implement `MediaSession`**

Constructor dependencies must be injectable for Node tests. Production defaults use `navigator.mediaDevices`, `RTCPeerConnection`, `RTCSessionDescription`, `RTCIceCandidate`, and document-created audio nodes.

- [ ] **Step 4: Verify green**

Run the focused Node command and confirm it passes.

### Task 3: Wire Room UI

**Files:**
- Modify: `static/room.js`
- Modify: `static/room.html`
- Modify: `static/styles.css`
- Modify: `src/transport/http/mod.rs`

- [ ] **Step 1: Add failing HTTP asset coverage**

Extend the static asset route test so `/assets/media-session.mjs` and `/assets/room-controls.mjs` must be served.

- [ ] **Step 2: Verify red**

Run:

```bash
cargo test transport::http::tests::页面静态模块可以访问 -- --nocapture
```

Expected: FAIL until Axum serves the two new modules.

- [ ] **Step 3: Wire status nodes and controls**

Expose `#device-state`, `#media-state`, `#downlink-state`, `#mute-self`, `#leave-room`, and a host for remote audio. Update `room.js` to:

- start `MediaSession` after `joined_room`;
- forward `ice_candidate` and `renegotiation_needed` broadcasts to media session;
- update status nodes from media callbacks;
- send `set_self_muted` on mute changes;
- render owner permission buttons and send `set_member_can_speak`;
- close media on room close and page exit.

- [ ] **Step 4: Serve modules and verify green**

Update the asset router and rerun the focused HTTP test.

### Task 4: Verify And Integrate

**Files:**
- Use temporary Playwright script under `/tmp/remote-voice-browser-qa`.

- [ ] **Step 1: Run full automated checks**

Run:

```bash
node --test tests/frontend/*.test.mjs
cargo test
git diff --check
```

- [ ] **Step 2: Start server**

Run `cargo run` and keep the local server alive on the configured port.

- [ ] **Step 3: Extend Playwright flow**

Create owner and member pages with fake media devices. Assert:

- both rooms show microphone authorized and media connected;
- owner URL has generated room ID;
- mute toggles the current owner snapshot to muted;
- owner can mute member speak permission and both pages render the muted permission state;
- console and failed asset responses are clear on desktop and mobile.

- [ ] **Step 4: Record residual risks**

Report that real audible verification still requires a human microphone/speaker pass.
