# Phase 1 Handoff - P2P Protocol and Test Design

Date: 2026-07-01

## Current State

- Phase 0 documentation was committed as `a0a4d63 docs: plan p2p media rollout`.
- Phase 1 added design and test-planning documents only.
- No runtime code has been changed for P2P yet.
- No runtime tests were run because Phase 1 is documentation-only.

## Completed Progress

- Finalized the P2P protocol boundary:
  - existing `webrtc_offer`, `webrtc_answer`, and `ice_candidate` remain
    SFU-only;
  - new `p2p_offer`, `p2p_answer`, `p2p_ice_candidate`, and
    `p2p_connection_failed` are used for member-to-member signaling;
  - `media_route_updated` reports per-member-pair route fallback.
- Assigned route-state ownership to `RoomStore`.
- Chose normalized member-pair route keys so A-B and B-A share one route.
- Defined missing-route default as `p2p`.
- Defined `p2p_connection_failed` as a single-pair fallback to `sfu`.
- Confirmed P2P offer, answer, and ICE require targeted WebSocket delivery.
- Captured Phase 2, Phase 3, and Phase 4 test coverage.

## Files Changed In Phase 1

- `docs/superpowers/specs/2026-07-01-p2p-protocol-design.md`
- `docs/superpowers/specs/2026-07-01-p2p-test-plan.md`
- `docs/dev-session/progress-2026-07-01-p2p-phase-1.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-1.md`

## Verification

Run before committing Phase 1:

```bash
git status --short
```

No runtime tests are required for this phase.

## Unfinished Items

- Backend P2P signal variants are not implemented.
- Backend media route state is not implemented.
- Targeted P2P WebSocket delivery is not implemented.
- Frontend P2P manager is not implemented.
- Browser P2P and fallback tests are not implemented.
- Backend service split has not started and must remain deferred until P2P is
  complete.

## Next Phase Goal

Phase 2 should implement backend P2P signaling and per-member-pair media route
state while preserving the current SFU path.

Recommended Phase 2 order:

1. Read this handoff and both Phase 1 spec documents.
2. Add backend route types and normalized member-pair key in
   `src/domain/room.rs`.
3. Add `RoomStore` validation, route read/update, and route cleanup methods.
4. Add P2P variants to `ClientSignal` and `ServerSignal` in
   `src/transport/http/signaling.rs`.
5. Add `SignalHub::send_to_member`.
6. Wire P2P offer, answer, ICE, and failure handling in `handle_socket`.
7. Add backend tests in `tests/room_permissions.rs` and
   `tests/signaling_ws.rs`.
8. Run:

```bash
cargo test --test room_permissions
cargo test --test signaling_ws
```

## Suggested Phase 2 Agent Split

- Main agent: own phase boundaries, protocol compatibility, final integration,
  test selection, docs, and commit.
- Backend domain worker: implement `RoomStore` media route types, methods, and
  `tests/room_permissions.rs`.
- Backend signaling worker: implement signal variants, targeted delivery, and
  WebSocket tests in `tests/signaling_ws.rs`.
- Review/explorer agent: check SFU compatibility and Chinese comment coverage
  before final tests.

## Assumptions

- P2P fallback is per member pair.
- Existing SFU behavior remains available and unchanged.
- P2P route state is backend-private and should not alter `Room` JSON snapshots
  in Phase 2.
- P2P messages require `request_id` for error correlation but do not require a
  success acknowledgement in Phase 2.
- Recoverable disconnect keeps route state until the member either resumes or
  expires.
