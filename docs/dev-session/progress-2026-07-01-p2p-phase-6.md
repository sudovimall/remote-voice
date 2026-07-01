# Phase 6 Progress - Backend Service Split Implementation

Date: 2026-07-01

## Completed

- Read the Phase 5 handoff and backend service split design.
- Attempted to use the requested multi-agent split for Phase 6 exploration.
  All three spawned subagents returned 503 errors, so the main agent completed
  the implementation and verification directly.
- Added the `src/service/` module with:
  - `authenticated_room.rs`
  - `room_lifecycle.rs`
  - `member_control.rs`
  - `media_route.rs`
  - `chat.rs`
  - `mod.rs`
- Wired `AppState` to construct and expose a `Services` aggregate while keeping
  the existing `rooms`, `media`, `signals`, and `auth` fields for
  compatibility during the split.
- Moved authenticated persistent-room orchestration behind
  `AuthenticatedRoomService`.
- Moved room create, join, resume, register-rollback, close, and joined-room
  history reads behind `RoomLifecycleService`.
- Moved chat message send orchestration behind `ChatService`.
- Moved member mute, speaking permission, listening preference, speaking
  normalization, and latency validation behind `MemberControlService`.
- Moved screen-share, video-call, SFU offer/ICE, P2P signal validation/forward,
  and P2P failure routing behind `MediaRouteService`.
- Updated HTTP auth/admin handlers and room list handlers to use service
  methods instead of direct persistent-room storage access.
- Updated WebSocket signaling orchestration to call service methods while
  preserving existing JSON message shapes, error codes, and broadcast order.
- Preserved two compatibility details from the previous signaling path:
  - changing `can_speak` updates room state even if media policy sync fails;
  - stopping screen share broadcasts room state cleanup even if media owner
    release fails.
- Added Chinese comments for new public Rust behavior entries and key service
  compatibility decisions.

## Files Changed

- `src/lib.rs`
- `src/state.rs`
- `src/service/mod.rs`
- `src/service/authenticated_room.rs`
- `src/service/chat.rs`
- `src/service/member_control.rs`
- `src/service/media_route.rs`
- `src/service/room_lifecycle.rs`
- `src/transport/http/auth.rs`
- `src/transport/http/rooms.rs`
- `src/transport/http/signaling.rs`
- `docs/dev-session/progress-2026-07-01-p2p-phase-6.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-6.md`

## Verification

Ran:

```bash
rustfmt --edition 2024 --check --config skip_children=true src/lib.rs src/state.rs src/transport/http/auth.rs src/transport/http/rooms.rs src/transport/http/signaling.rs src/service/authenticated_room.rs src/service/chat.rs src/service/media_route.rs src/service/member_control.rs src/service/mod.rs src/service/room_lifecycle.rs
cargo test
npm run test:browser
git diff --check
```

Results:

- Targeted rustfmt check passed for the Phase 6 files.
- `cargo test` passed:
  - 81 library tests;
  - 28 `tests/room_permissions.rs` tests;
  - 38 `tests/signaling_ws.rs` tests;
  - doc tests.
- `npm run test:browser` passed all 3 Playwright tests.
- `git diff --check` passed.

Note: a global `cargo fmt --check` reports pre-existing format differences in
files outside Phase 6 (`src/config/settings.rs`, `src/media/mod.rs`, and
`src/transport/http/mod.rs`). Those unrelated files were left untouched.

## Notes

- No frontend source files were changed in Phase 6.
- Existing browser P2P coverage was run because WebSocket signaling
  orchestration and P2P/SFU route handling changed internally.
- `RoomStore`, `MediaController`, and `SignalHub` remain available on
  `AppState` during this transition; Phase 6 did not attempt to hide all lower
  layers from transport in one pass.
