# Phase 2 Handoff - Backend P2P Signaling and Media Routes

Date: 2026-07-01

## Current State

- Phase 2 backend implementation is complete.
- Backend now accepts and forwards member-to-member P2P signaling through new
  `p2p_*` message types.
- Backend records media route fallback per normalized member pair.
- Existing SFU signaling still uses `webrtc_offer`, `webrtc_answer`, and
  `ice_candidate`; those messages were not repurposed.
- Frontend does not yet create P2P `RTCPeerConnection` instances or consume the
  new route updates.

## Completed Progress

- Added `MediaRoute`, `MediaRouteReason`, and backend-private route state in
  `RoomStore`.
- Added route cleanup on explicit member leave and disconnected-member
  expiration.
- Kept route state during recoverable disconnect and resume.
- Added `RoomStore::validate_p2p_target`, `RoomStore::media_route`, and
  `RoomStore::mark_p2p_connection_failed`.
- Added `ClientSignal` variants for `p2p_offer`, `p2p_answer`,
  `p2p_ice_candidate`, and `p2p_connection_failed`.
- Added `ServerSignal` variants for targeted P2P delivery and
  `media_route_updated`.
- Added `SignalHub::send_to_member` and `SignalHub::ensure_member_registered`.
- Added domain and WebSocket tests for:
  - default P2P route;
  - normalized member pairs;
  - single-pair SFU fallback;
  - self, missing, cross-room, offline, and no-sender target failures;
  - targeted P2P offer, answer, and ICE delivery;
  - route-update broadcast;
  - existing SFU offer compatibility.

## Files Changed In Phase 2

- `src/domain/room.rs`
- `src/transport/http/signaling.rs`
- `tests/room_permissions.rs`
- `tests/signaling_ws.rs`
- `docs/dev-session/progress-2026-07-01-p2p-phase-2.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-2.md`

## Verification

Passed:

```bash
cargo test --test room_permissions
cargo test --test signaling_ws
cargo test --lib
```

## Unfinished Items

- Frontend P2P media session manager is not implemented.
- Frontend does not send `p2p_offer`, `p2p_answer`, `p2p_ice_candidate`, or
  `p2p_connection_failed`.
- Frontend does not consume `media_route_updated`.
- Browser end-to-end P2P fallback tests are not implemented.
- Backend service split has not started and must remain deferred.

## Next Phase Goal

Phase 3 should implement frontend P2P session management while preserving the
current SFU media session as fallback.

Recommended Phase 3 order:

1. Read this handoff plus:
   - `docs/superpowers/specs/2026-07-01-p2p-protocol-design.md`
   - `docs/superpowers/specs/2026-07-01-p2p-test-plan.md`
2. Read the current frontend media/session boundaries:
   - `frontend/src/lib/media-session.js`
   - `frontend/src/composables/useRoomMediaSession.js`
   - `frontend/src/composables/useRoomScreenShareSession.js`
   - `frontend/src/composables/useRoomSession.js`
   - `frontend/src/lib/room-connection.js`
3. Add an independent P2P media session manager instead of folding browser to
   browser PeerConnections into the existing SFU `MediaSession`.
4. Wire `handleRoomSignal()` to dispatch P2P offer, answer, ICE, and
   `media_route_updated`.
5. Publish microphone, camera, and screen-share tracks to active P2P peers while
   keeping SFU fallback available for failed pairs.
6. Add focused frontend unit tests for the P2P manager and integration boundary.
7. Run:

```bash
npm run test:frontend
npm run build:frontend
```

Run `npm run test:browser` only if Phase 3 touches browser end-to-end flow or
adds enough browser behavior to validate there; otherwise leave it for Phase 4.

## Suggested Phase 3 Agent Split

- Main agent: own frontend architecture boundary, integration, test selection,
  docs, and commit.
- Frontend P2P worker: implement `p2p-media-session` and its unit tests.
- Frontend integration worker: wire `useRoomSession`/signal dispatch and update
  composable boundary tests.
- Review/explorer agent: verify SFU compatibility, track cleanup, and Chinese
  comment coverage.

## Assumptions

- P2P remains the default route for member pairs unless the backend broadcasts
  `media_route_updated` with `route: "sfu"`.
- Existing SFU negotiation remains the fallback path and should not be removed.
- Screen-share video and camera video must remain separate local media sources.
- The backend `media_route_updated.member_ids` order is already normalized and
  should be used as-is by the frontend.
