# Phase 6 Handoff - Backend Service Split Implementation

Date: 2026-07-01

## Current State

- P2P-first media behavior from Phases 1-4 remains implemented.
- Phase 5 service split design has now been partially implemented.
- `src/service/` exists and is wired through `AppState::services`.
- HTTP auth/admin handlers and WebSocket signaling now delegate major business
  orchestration to service methods.
- Public protocol DTOs remain in `src/transport/http/signaling.rs`.
- Existing JSON fields, P2P signal names, SFU signal semantics, and error
  messages were preserved.

## Completed Progress

- Added service aggregate:

```text
src/service/
  mod.rs
  authenticated_room.rs
  room_lifecycle.rs
  member_control.rs
  media_route.rs
  chat.rs
```

- `AuthenticatedRoomService` owns persistent-room create, list, admin close,
  owner close, touch, and join-role decisions.
- `RoomLifecycleService` owns create, join, resume, register rollback, runtime
  close, and joined-room history lookup.
- `ChatService` owns chat send outcome creation.
- `MemberControlService` owns member mute, speak permission, listening
  preference, speaking normalization, and latency validation.
- `MediaRouteService` owns screen-share, video-call, SFU offer/ICE, P2P
  forwarding, P2P target validation, and P2P failure route updates.
- `transport/http/auth.rs` no longer reads persistent-room storage directly for
  admin room list/close.
- `transport/http/rooms.rs` no longer reads persistent-room storage directly
  for authenticated lobby room summaries.
- `transport/http/signaling.rs` now calls services for the migrated command
  slices and keeps transport-only concerns in place:
  - JSON parsing;
  - WebSocket socket lifecycle;
  - signal queue registration;
  - broadcast and targeted response sending;
  - final disconnect/leave broadcast mechanics.

## Files Changed In Phase 6

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

- Targeted rustfmt check passed.
- `cargo test` passed.
- `npm run test:browser` passed all 3 browser tests.
- `git diff --check` passed.

Global `cargo fmt --check` still reports unrelated formatting differences
outside the Phase 6 files. Do not use a global rustfmt commit unless a later
phase explicitly chooses to normalize those files.

## Unfinished Items

- `transport/http/signaling.rs` is still large because it owns protocol DTOs,
  WebSocket lifecycle, broadcast delivery, and leave/disconnect event emission.
- Leave/disconnect cleanup still calls `RoomStore`, `MediaController`, and
  `SignalHub` directly where transport must coordinate socket closure and
  broadcast side effects.
- No dedicated service-unit test files were added; existing domain, signaling,
  auth, and browser tests are the compatibility harness.
- A future pass could introduce a neutral realtime-effect return type if the
  project wants to reduce direct broadcast code inside transport further.

## Next Phase Goal

Phase 7 should perform final regression, comment review, and long-task closeout.

Recommended Phase 7 order:

1. Read this handoff and the long-term plan.
2. Inspect the Phase 6 diff for accidental protocol or behavior changes.
3. Review touched Rust public behavior entries and key service methods for
   Chinese comments.
4. Run the final planned verification:

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
git status --short
```

5. Record any global formatting caveat separately from Phase 6 service changes.
6. Produce final progress/handoff or closeout notes and commit only Phase 7
   files if any are changed.

## Suggested Phase 7 Agent Split

- Main agent: own final regression, status review, docs, and commit decisions.
- Backend reviewer: inspect service split for behavior drift in auth, room
  lifecycle, member controls, media routing, and WebSocket broadcast order.
- Frontend/browser verifier: run and inspect frontend/browser regressions,
  especially P2P fallback and cleanup flows.
- Documentation reviewer: verify all phase progress/handoff docs exist and
  summarize the final state.

## Assumptions

- `RoomStore` remains the domain invariant source.
- `MediaController` remains the SFU/WebRTC engine.
- `ClientSignal` and `ServerSignal` remain transport protocol DTOs.
- Existing direct `AppState` fields are intentionally retained for
  compatibility during this split.
