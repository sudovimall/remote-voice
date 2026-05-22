# Room Resume And Docker Nginx Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add refresh/reconnect recovery for members and owners with a 30 second grace period, then provide Docker + Nginx HTTPS/WebSocket deployment files.

**Architecture:** Add server-owned resume tokens and disconnected-member lifecycle operations to the room domain. WebSocket disconnects mark members offline and schedule guarded cleanup; explicit leave keeps the existing immediate removal/owner close behavior. The browser saves a tab-local room session and prefers create/join intent before resume, while deployment files keep HTTPS termination in Nginx and the Rust app internal to Compose.

**Tech Stack:** Rust 2024, Tokio timers, Axum WebSocket, Serde JSON, browser ES modules, Node test runner, Playwright, Docker Compose, Nginx.

---

## File Structure

- Modify `src/domain/room.rs`, `src/state.rs`, `src/config/settings.rs`, and Rust domain tests for resume token and disconnect lifecycle.
- Modify `src/transport/http/signaling.rs` and `tests/signaling_ws.rs` for `resume_room`, `joined_room.resume_token`, explicit leave, and delayed disconnect cleanup.
- Modify `static/room-entry.mjs`, `static/room-state.mjs`, `static/room.js`, `static/signaling-client.mjs`, and frontend tests for room sessions and resume/reconnect.
- Create `Dockerfile`, `.dockerignore`, `docker-compose.yml`, `deploy/nginx/nginx.conf`, and README deployment guidance.

### Task 1: Room Domain Resume Lifecycle

- [ ] Write failing Rust tests for member resume token issue/validation, mark disconnected/connected, member disconnect cleanup, and owner disconnect cleanup.
- [ ] Run focused domain tests to verify red.
- [ ] Add private recovery token storage that does not serialize in `Member`, plus room store methods to resume, mark disconnected, and expire disconnected members.
- [ ] Verify focused Rust tests green.

### Task 2: WebSocket Resume And Grace Cleanup

- [ ] Write failing WebSocket integration tests for `joined_room.resume_token`, `resume_room` success, bad token rejection, disconnect keeping owner room alive, and explicit owner leave closing immediately.
- [ ] Run focused WebSocket tests to verify red.
- [ ] Add `resume_room` protocol, explicit leave tracking, disconnected cleanup scheduling, and resume response/broadcast behavior.
- [ ] Verify WebSocket tests green.

### Task 3: Browser Tab Session And Reconnect

- [ ] Write failing frontend tests for tab-local room session save/load/clear and resume signal construction.
- [ ] Run Node frontend tests to verify red.
- [ ] Save resume credentials after joined room, load resume on refresh, send explicit leave before clearing session, and reconnect/resume when socket closes while page remains active.
- [ ] Verify frontend tests green.

### Task 4: Browser Resume QA

- [ ] Extend Playwright flow for owner refresh, member refresh, media reconnect, and explicit owner leave closure.
- [ ] Run local server and Playwright flow until desktop/mobile console checks pass.

### Task 5: Docker Nginx Entry

- [ ] Add failing lightweight checks where practical: `docker compose config` and text checks for Nginx WebSocket upgrade if Docker is available.
- [ ] Create multi-stage Dockerfile, compose file, Nginx HTTPS reverse proxy config, cert ignore path, and README instructions.
- [ ] Verify Compose config when Docker command is available; otherwise report the unavailable verification.

### Task 6: Final Verification

- [ ] Run `cargo fmt`, `node --test tests/frontend/*.test.mjs`, `cargo test`, and `git diff --check`.
- [ ] Report the exact checks, running local URL if server is kept alive, and remaining certificate/network validation.
