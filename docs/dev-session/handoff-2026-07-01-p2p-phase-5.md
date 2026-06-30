# Phase 5 Handoff - Backend Service Split Design

Date: 2026-07-01

## Current State

- P2P-first media behavior was completed and tested through Phase 4.
- Phase 5 added only a backend service split design document.
- No runtime code has been moved yet.
- The current backend still works through:
  - `RoomStore` as the room/domain facade;
  - `MediaController` as the SFU/WebRTC engine;
  - `transport/http/signaling.rs` as the WebSocket protocol and orchestration
    entry point;
  - `transport/http/auth.rs` and `rooms.rs` for HTTP auth/admin/room APIs.

## Completed Progress

- Added:
  `docs/superpowers/specs/2026-07-01-backend-service-split-design.md`.
- The design recommends adding:

```text
src/service/
  mod.rs
  authenticated_room.rs
  room_lifecycle.rs
  member_control.rs
  media_route.rs
  chat.rs
  realtime.rs
```

- The design keeps protocol DTOs in transport and moves command orchestration
  into services.
- The design keeps `RoomStore` as a single-lock domain authority for Phase 6,
  rather than splitting room state into multiple stores.
- The design documents draft service methods, realtime effect shapes, error
  mapping rules, and migration/test order.

## Files Changed In Phase 5

- `docs/superpowers/specs/2026-07-01-backend-service-split-design.md`
- `docs/dev-session/progress-2026-07-01-p2p-phase-5.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-5.md`

## Verification

Ran:

```bash
git status --short
```

No runtime tests were required for this design-only phase.

## Unfinished Items

- `src/service/` has not been created.
- `AppState` has not been changed to expose service handles.
- `AuthService::store()` is still used by transport code.
- `transport/http/signaling.rs` still owns business orchestration.
- `tests/signaling_ws.rs` and `tests/room_permissions.rs` have not been moved
  or split into service tests.

## Next Phase Goal

Phase 6 should implement the backend service split while preserving protocol
and behavior compatibility.

Recommended Phase 6 order:

1. Read this handoff and:
   `docs/superpowers/specs/2026-07-01-backend-service-split-design.md`.
2. Add `src/service/mod.rs` and service shells.
3. Wire service handles from `AppState` without removing existing direct fields
   yet.
4. Implement `AuthenticatedRoomService` first and replace raw
   `AuthService::store()` usage in transport.
5. Add service tests before moving each behavior slice.
6. Migrate orchestration in this order:
   - authenticated room/persistence;
   - chat;
   - member controls;
   - P2P media route forwarding/fallback;
   - screen-share/video-call orchestration;
   - room lifecycle and disconnect expiration;
   - SFU offer/ICE/renegotiation plumbing.
7. Keep `tests/signaling_ws.rs` as the end-to-end compatibility harness during
   the split.
8. Run:

```bash
cargo test
```

Also run `npm run test:browser` if any implementation changes WebSocket message
ordering, P2P/SFU signaling behavior, or browser-observable room lifecycle
events.

## Suggested Phase 6 Agent Split

- Main agent: own service skeleton, integration order, final test decisions,
  docs, and commits.
- Authenticated room worker: implement persistent-room service and update
  `auth.rs`, `rooms.rs`, and persistent-room tests.
- Chat/member worker: migrate chat and member-control orchestration into
  service methods with tests.
- Media route worker: migrate P2P forwarding/fallback and screen/video command
  orchestration.
- Lifecycle worker: migrate leave/disconnect/expiration last after lower-risk
  services are stable.

## Assumptions

- `RoomStore` remains the domain invariant source in Phase 6.
- `MediaController` remains the SFU/WebRTC engine.
- `ClientSignal` and `ServerSignal` remain the public protocol DTOs.
- Existing JSON fields, error codes, and Chinese error messages remain
  compatible unless a test is intentionally updated in the same phase.
