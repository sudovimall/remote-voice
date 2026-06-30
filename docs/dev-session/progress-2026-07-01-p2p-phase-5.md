# Phase 5 Progress - Backend Service Split Design

Date: 2026-07-01

## Completed

- Read the Phase 4 handoff and the long-term P2P/media-service split plan.
- Spawned three scoped read-only explorer agents:
  - room-domain explorer for `src/domain/room.rs` and
    `tests/room_permissions.rs`;
  - signaling transport explorer for `src/transport/http/signaling.rs` and
    `tests/signaling_ws.rs`;
  - auth/persistence explorer for `src/transport/http/auth.rs`, `src/auth`,
    and `src/storage`.
- Reviewed the current backend boundaries locally:
  - `src/domain/room.rs`
  - `src/transport/http/signaling.rs`
  - `src/transport/http/auth.rs`
  - `src/transport/http/rooms.rs`
  - `src/auth/service.rs`
  - `src/media/mod.rs`
  - `src/state.rs`
- Added the Phase 5 design document:
  `docs/superpowers/specs/2026-07-01-backend-service-split-design.md`.
- The design defines:
  - proposed `src/service/` layout;
  - authenticated persistent-room service boundary;
  - room lifecycle service boundary;
  - member control service boundary;
  - media route service boundary;
  - chat service boundary;
  - neutral realtime effect shape;
  - error mapping rules;
  - Phase 6 migration order;
  - service and regression test migration order.

## Design Decisions

- Keep `RoomStore` as the domain authority and public compatibility facade in
  the first implementation pass.
- Do not split room state into multiple independently locked stores yet,
  because leave/disconnect/expiration cleanup must remain atomic.
- Keep `ClientSignal` and `ServerSignal` in `transport/http/signaling.rs`.
- Keep WebSocket socket lifecycle and JSON protocol parsing in transport.
- Move business orchestration out of the large `handle_socket` match into
  services that return domain outcomes and realtime effects.
- Hide persistent-room storage access behind an authenticated room service so
  `AuthService::store()` no longer leaks into transport handlers.
- Keep SFU `webrtc_*` semantics separate from member-to-member `p2p_*`
  semantics.

## Files Changed

- `docs/superpowers/specs/2026-07-01-backend-service-split-design.md`
- `docs/dev-session/progress-2026-07-01-p2p-phase-5.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-5.md`

## Verification

Ran:

```bash
git status --short
```

Result at the time of verification showed the new Phase 5 design document plus
pre-existing unrelated untracked local files. No runtime tests were required by
Phase 5 because this phase is design-only.

## Notes

- No runtime Rust or frontend files were changed in this phase.
- No backend service implementation has started.
- Existing untracked files outside the phase were left untouched.
