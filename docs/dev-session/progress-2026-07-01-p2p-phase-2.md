# Phase 2 Progress - Backend P2P Signaling and Media Routes

Date: 2026-07-01

## Completed

- Read the Phase 1 handoff, protocol design, and test plan.
- Used scoped read-only subagents for:
  - `RoomStore` lifecycle and route-state boundaries;
  - WebSocket signaling, `SignalHub`, and protocol tests.
- Added backend-private per-member-pair media route state to `RoomStore`.
- Added normalized member-pair keys so A-B and B-A share one route.
- Kept missing route state as the default `p2p` route.
- Added P2P target validation for same-room, non-self, online members.
- Added P2P failure handling that switches only the failed member pair to
  `sfu`.
- Kept recoverable disconnect from deleting route state, while explicit leave
  and disconnected-member expiration clean routes involving the removed member.
- Added P2P WebSocket signal variants:
  - `p2p_offer`
  - `p2p_answer`
  - `p2p_ice_candidate`
  - `p2p_connection_failed`
  - `media_route_updated`
- Added targeted `SignalHub::send_to_member` delivery for P2P offer, answer,
  and ICE.
- Preserved the existing SFU `webrtc_offer`, `webrtc_answer`, and
  `ice_candidate` behavior and kept the existing compatibility tests passing.
- Added Chinese comments for the new public backend behavior and modified
  lifecycle methods in the touched areas.

## Files Changed

- `src/domain/room.rs`
- `src/transport/http/signaling.rs`
- `tests/room_permissions.rs`
- `tests/signaling_ws.rs`

## Verification

Passed:

```bash
cargo test --test room_permissions
cargo test --test signaling_ws
cargo test --lib
```

`cargo test --lib` was run because this phase also added unit coverage inside
`src/transport/http/signaling.rs`.

## Notes

- No frontend runtime code was changed in this phase.
- No backend service split was started; it remains deferred until the P2P
  behavior is complete and verified.
- Existing untracked files outside this phase were left untouched.
