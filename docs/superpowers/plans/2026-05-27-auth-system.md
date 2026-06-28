# Auth System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the configurable account authentication system described in `docs/superpowers/specs/2026-05-27-auth-system-design.md`.

**Architecture:** Add an `auth` runtime to `AppState` that is disabled by default and initialized from config when enabled. Keep auth, SQLite persistence, HTTP handlers, and WebSocket checks in focused modules so the current anonymous room flow remains unchanged when auth is disabled.

**Tech Stack:** Rust 2024, Axum 0.8, Tokio, rusqlite, Argon2id, SHA-256 token hashing, HttpOnly session cookies, static HTML/CSS/ES modules.

---

## File Structure

- Create `src/auth/model.rs`: user/session/invite DTOs and role types.
- Create `src/auth/password.rs`: Argon2id password hash and verification.
- Create `src/auth/session.rs`: random token/code generation, SHA-256 hashing, cookie helpers.
- Create `src/auth/service.rs`: login, logout, current-user, invite, register, admin authorization, persistent-room operations.
- Create `src/auth/mod.rs`: public module exports and `AuthRuntime`.
- Create `src/storage/migrations.rs`: SQLite schema creation.
- Create `src/storage/sqlite.rs`: mutex-protected SQLite connection and repository queries.
- Create `src/storage/mod.rs`: storage module exports.
- Create `src/transport/http/auth.rs`: login/register/admin API handlers.
- Create `static/login.html`, `static/register.html`, `static/admin.html`: auth pages.
- Create `static/auth-page.js`, `static/admin.js`, `static/auth-ui.mjs`: frontend auth behavior.
- Modify `Cargo.toml`: add direct dependencies for SQLite, password hashing, token hashing, and cookies.
- Modify `src/config/settings.rs`: add `auth` and `storage` config with defaults and validation tests.
- Modify `src/state.rs`: initialize `AuthRuntime`.
- Modify `src/error.rs`: add stable auth error codes.
- Modify `src/transport/http/mod.rs`: register auth pages/assets/APIs and apply route-level auth behavior.
- Modify `src/transport/http/rooms.rs`: include persistent rooms when auth is enabled and reject closed rooms.
- Modify `src/transport/http/signaling.rs`: require a valid session when enabled and persist room ownership on create/join/close.
- Modify `static/index.html`, `static/lobby.js`, `static/lobby-rooms.mjs`, `static/styles.css`: show auth user/logout/admin affordances when enabled.

## Tasks

### Task 1: Config And Error Surface

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config/settings.rs`
- Modify: `src/error.rs`

- [ ] **Step 1: Write failing config tests**

Add tests proving `auth.enabled=false` is the default, enabled auth requires admin settings, and storage defaults to SQLite path `remote-voice.db`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test config::settings::tests::认证配置默认关闭 --lib`
Expected: FAIL because auth config fields do not exist.

- [ ] **Step 3: Implement config and auth errors**

Add `AuthSettings`, `AdminSettings`, `SessionSettings`, `StorageSettings`, `StorageKind`, `SqliteSettings`, and `SessionSecureSetting`. Add error variants for `Unauthenticated`, `Forbidden`, `InvalidCredentials`, `InviteNotFound`, `InviteExpired`, `InviteUsed`, `UsernameTaken`, `SessionExpired`, `AuthDisabled`, and `RoomClosed`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test config::settings::tests::认证配置 --lib`
Expected: PASS.

### Task 2: SQLite Storage

**Files:**
- Create: `src/storage/mod.rs`
- Create: `src/storage/migrations.rs`
- Create: `src/storage/sqlite.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing storage tests**

Add tests that open an in-memory SQLite database, run migrations, upsert an admin user, create sessions, create invites, consume invites transactionally, and create/list/close persistent rooms.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test storage::sqlite::tests --lib`
Expected: FAIL because storage module does not exist.

- [ ] **Step 3: Implement storage repository**

Use one `rusqlite::Connection` behind `std::sync::Mutex`. Store all timestamps as epoch seconds. Keep password/session/invite hashes opaque strings and never expose hash fields in HTTP DTOs.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test storage::sqlite::tests --lib`
Expected: PASS.

### Task 3: Auth Service

**Files:**
- Create: `src/auth/mod.rs`
- Create: `src/auth/model.rs`
- Create: `src/auth/password.rs`
- Create: `src/auth/session.rs`
- Create: `src/auth/service.rs`
- Modify: `src/lib.rs`
- Modify: `src/state.rs`

- [ ] **Step 1: Write failing service tests**

Add tests for password verification, admin sync, login success, login failure with uniform error, session lookup, logout revocation, invite creation, invite registration, invite reuse rejection, and user/admin authorization.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test auth:: --lib`
Expected: FAIL because auth module does not exist.

- [ ] **Step 3: Implement auth runtime and service**

Add `AuthRuntime::Disabled` and `AuthRuntime::Enabled(AuthContext)`. Initialize SQLite and sync the configured admin only when `auth.enabled=true`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test auth:: --lib`
Expected: PASS.

### Task 4: HTTP Auth Routes And Guards

**Files:**
- Create: `src/transport/http/auth.rs`
- Modify: `src/transport/http/mod.rs`
- Create: `static/login.html`
- Create: `static/register.html`
- Create: `static/admin.html`
- Create: `static/auth-page.js`
- Create: `static/admin.js`
- Create: `static/auth-ui.mjs`
- Modify: `static/styles.css`

- [ ] **Step 1: Write failing HTTP tests**

Add tests proving unauthenticated page requests redirect to `/login?next=...`, unauthenticated APIs return `401`, login sets the session cookie, logout clears it, register auto-logs in, and non-admin admin API calls return `403`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test transport::http::auth --lib`
Expected: FAIL because auth routes do not exist.

- [ ] **Step 3: Implement routes and page guards**

Expose `/login`, `/register`, `/admin`, `/api/auth/login`, `/api/auth/logout`, `/api/auth/register`, `/api/auth/me`, `/api/admin/invites`, `/api/admin/users`, `/api/admin/rooms`, and `/api/admin/rooms/{room_id}/close`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test transport::http::auth --lib`
Expected: PASS.

### Task 5: Room Persistence And WebSocket Enforcement

**Files:**
- Modify: `src/transport/http/rooms.rs`
- Modify: `src/transport/http/signaling.rs`
- Modify: `src/domain/room.rs`

- [ ] **Step 1: Write failing integration tests**

Add tests proving `/ws` rejects unauthenticated upgrade when auth is enabled, accepts authenticated create/join, creates `persistent_rooms`, lists persisted empty rooms, restores a runtime room on join, and rejects closed rooms.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test signaling_ws websocket_未登录访问_ws_被拒绝`
Expected: FAIL because WebSocket auth is not implemented.

- [ ] **Step 3: Implement WebSocket auth and persistent room hooks**

Parse the session cookie from WebSocket headers before upgrade. On create, create a runtime room then persist ownership, rolling back runtime room if persistence fails. On join, restore a closed-over persistent room into `RoomStore` when needed. On close, mark the persistent room closed and broadcast `room_closed`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test signaling_ws websocket_未登录访问_ws_被拒绝`
Expected: PASS.

### Task 6: Frontend Integration

**Files:**
- Modify: `static/index.html`
- Modify: `static/lobby.js`
- Modify: `static/lobby-rooms.mjs`
- Modify: `static/styles.css`
- Add tests under `tests/frontend/` if behavior can be unit-tested without a browser.

- [ ] **Step 1: Write failing frontend tests**

Add ES module tests for `auth-ui.mjs` helpers and lobby room normalization of persisted room summaries.

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/frontend/auth-ui.test.mjs tests/frontend/lobby-rooms.test.mjs`
Expected: FAIL because helpers do not exist.

- [ ] **Step 3: Implement frontend auth controls**

Show current user and logout/admin controls only when `/api/auth/me` reports auth enabled. Keep the anonymous lobby unchanged when auth is disabled.

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test tests/frontend/auth-ui.test.mjs tests/frontend/lobby-rooms.test.mjs`
Expected: PASS.

### Task 7: Full Verification

**Files:**
- All modified files.

- [ ] **Step 1: Format**

Run: `cargo fmt`
Expected: no unformatted Rust files remain.

- [ ] **Step 2: Run backend tests**

Run: `cargo test`
Expected: all Rust tests pass.

- [ ] **Step 3: Run frontend tests**

Run: `node --test tests/frontend/*.test.mjs`
Expected: all frontend module tests pass.

- [ ] **Step 4: Review diff**

Run: `git diff --stat` and `git diff --check`
Expected: no whitespace errors and changes match the auth scope.
