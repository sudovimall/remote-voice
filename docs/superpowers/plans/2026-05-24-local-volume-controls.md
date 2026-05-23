# Local Volume Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local persisted controls for per-member playback volume and current-user microphone input gain.

**Architecture:** Keep all volume preferences client-local in `localStorage`; no backend protocol changes. Put storage and formatting logic in `static/audio-volume.mjs`, apply media effects in `static/media-session.mjs`, and keep `static/room.js` responsible for DOM wiring and persistence.

**Tech Stack:** Vanilla ES modules, browser Web Audio API, hidden HTML audio elements, Node test runner, static HTML/CSS.

---

### Task 1: Audio Volume Helper Module

**Files:**
- Create: `static/audio-volume.mjs`
- Create: `tests/frontend/audio-volume.test.mjs`

- [ ] **Step 1: Write failing helper tests**

Create tests for `clampVolume`, `volumePercent`, `memberVolumeKey`, `loadMemberVolume`, `saveMemberVolume`, `loadMicrophoneGain`, and `saveMicrophoneGain`.

- [ ] **Step 2: Run helper tests to verify RED**

Run: `node --test tests/frontend/audio-volume.test.mjs`

Expected: FAIL because `static/audio-volume.mjs` does not exist.

- [ ] **Step 3: Implement helper module**

Create `static/audio-volume.mjs` with volume clamping, percentage formatting, key generation, and safe storage read/write.

- [ ] **Step 4: Run helper tests to verify GREEN**

Run: `node --test tests/frontend/audio-volume.test.mjs`

Expected: PASS.

### Task 2: MediaSession Playback Volume

**Files:**
- Modify: `static/media-session.mjs`
- Modify: `tests/frontend/media-session.test.mjs`

- [ ] **Step 1: Write failing MediaSession remote volume tests**

Add tests proving remote track audio nodes receive a saved member volume and `setMemberVolume()` updates existing audio nodes.

- [ ] **Step 2: Run MediaSession tests to verify RED**

Run: `node --test tests/frontend/media-session.test.mjs`

Expected: FAIL because `setMemberVolume()` and member-aware audio entries are missing.

- [ ] **Step 3: Implement remote playback volume**

Track audio nodes with their `memberId`, add `setMemberVolume(memberId, volume)`, and apply clamped volume when remote streams play or settings change.

- [ ] **Step 4: Run MediaSession tests to verify GREEN**

Run: `node --test tests/frontend/media-session.test.mjs`

Expected: PASS.

### Task 3: MediaSession Microphone Gain

**Files:**
- Modify: `static/media-session.mjs`
- Modify: `tests/frontend/media-session.test.mjs`

- [ ] **Step 1: Write failing microphone gain tests**

Add tests proving Web Audio gain destination tracks are sent, `setMicrophoneGain()` updates the GainNode, mute targets the outbound track, and fallback still sends the original track.

- [ ] **Step 2: Run MediaSession tests to verify RED**

Run: `node --test tests/frontend/media-session.test.mjs`

Expected: FAIL because Web Audio gain support is missing.

- [ ] **Step 3: Implement microphone gain**

Inject an optional `AudioContextImpl`, build `source -> gain -> destination` when supported, add `setMicrophoneGain(gain)`, expose `microphoneGainSupported`, and release Web Audio resources on close.

- [ ] **Step 4: Run MediaSession tests to verify GREEN**

Run: `node --test tests/frontend/media-session.test.mjs`

Expected: PASS.

### Task 4: Room UI And Persistence

**Files:**
- Modify: `static/room.html`
- Modify: `static/room.js`
- Modify: `static/styles.css`
- Modify: `tests/frontend/room-layout.test.mjs`
- Modify: `tests/frontend/room-controls.test.mjs`

- [ ] **Step 1: Write failing static/UI tests**

Add static tests for the microphone gain slider, member volume slider classes, fixed percentage labels, and imports from `audio-volume.mjs`.

- [ ] **Step 2: Run UI tests to verify RED**

Run: `node --test tests/frontend/room-layout.test.mjs tests/frontend/room-controls.test.mjs`

Expected: FAIL because volume UI markup/classes are missing.

- [ ] **Step 3: Implement room UI wiring**

Add microphone gain slider to the voice panel, per-other-member volume sliders to member rows, persisted storage calls, and calls into `mediaSession.setMemberVolume()` / `setMicrophoneGain()`.

- [ ] **Step 4: Run UI tests to verify GREEN**

Run: `node --test tests/frontend/room-layout.test.mjs tests/frontend/room-controls.test.mjs`

Expected: PASS.

### Task 5: Full Verification

**Files:**
- Verify all touched files.

- [ ] **Step 1: Run frontend tests**

Run: `node --test tests/frontend/*.test.mjs`

Expected: PASS.

- [ ] **Step 2: Run JS syntax checks**

Run: `node --check static/audio-volume.mjs && node --check static/media-session.mjs && node --check static/room.js`

Expected: PASS.

- [ ] **Step 3: Run Rust tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 4: Check diff hygiene**

Run: `git diff --check`

Expected: no whitespace errors.
