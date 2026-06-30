# P2P Test Plan - Phase 1

Date: 2026-07-01

## Purpose

This document lists the concrete tests expected in later P2P phases. Phase 1
only records the plan; it does not add runtime tests or code.

## Phase 2 Backend Tests

Phase 2 should update both `tests/room_permissions.rs` and
`tests/signaling_ws.rs`.

### `tests/room_permissions.rs`

Add domain-level route tests:

- Default route for an unseen member pair is `p2p`.
- Pair keys are normalized, so A-B and B-A read and write the same route.
- Self-pair construction is rejected.
- `p2p_connection_failed` equivalent domain call changes only that pair to
  `sfu`.
- A-B fallback does not change A-C or B-C.
- Explicit member leave removes routes involving that member.
- Disconnected-member expiration removes routes involving that member.
- Owner room close removes the whole room and therefore all route state.
- Recoverable disconnect marks the member offline but does not immediately
  delete routes during the grace period.

### `tests/signaling_ws.rs`

Add WebSocket protocol tests:

- Same-room online member receives `p2p_offer` with `from_member_id`.
- `p2p_offer` is not broadcast to a third member.
- Same-room online member receives `p2p_answer` with `from_member_id`.
- Same-room online member receives `p2p_ice_candidate` with `from_member_id`
  and browser-shaped ICE fields.
- P2P signaling before joining returns the existing not-joined error.
- Sending a P2P signal to self returns `invalid_message`.
- Sending to a missing member returns `invalid_message` or `member_not_found`
  according to the final error mapping selected in Phase 2.
- Sending to a member in another room fails because the target is not in the
  sender's room.
- Sending to a disconnected member fails.
- If target delivery races with WebSocket unregister, the sender receives an
  error instead of silently losing the P2P signal.
- `p2p_connection_failed` broadcasts `media_route_updated` with sorted
  `member_ids`, `route: "sfu"`, and `reason: "p2p_failed"`.
- A-B fallback update does not change the route for A-C or B-C.
- Existing `webrtc_offer` still returns `webrtc_answer` from the SFU and is not
  forwarded to room members.
- Existing `webrtc_offer` with `target_member_id` remains rejected by
  `deny_unknown_fields`.

Existing helpers in `tests/signaling_ws.rs` are enough for Phase 2:

- `spawn_app`
- `connect_create`
- `connect_join`
- `connect_existing_member`
- `read_json`
- `read_until_type`

P2P SDP payloads can be fixed strings. They do not need real WebRTC offers
because Phase 2 only validates signaling and route state.

## Phase 3 Frontend Tests

Phase 3 should add `tests/frontend/p2p-media-session.test.mjs` for the new P2P
manager and extend existing boundary tests only where integration changes are
made.

### P2P Manager Unit Tests

Use a local fake `RTCPeerConnection` rather than reusing the SFU
`media-session.test.mjs` harness directly.

Required fake APIs:

- `createOffer`
- `createAnswer`
- `setLocalDescription`
- `setRemoteDescription`
- `addIceCandidate`
- `addTrack`
- `removeTrack`
- `getSenders`
- `close`
- `connectionState`
- `iceConnectionState`
- `localDescription`
- `remoteDescription`
- events: `icecandidate`, `track`, `connectionstatechange`,
  `iceconnectionstatechange`, and optionally `negotiationneeded`
- sender methods: `replaceTrack`, `setParameters`, `getParameters`

Core tests:

- Creating a connection for another member sends `p2p_offer` with
  `target_member_id`.
- The manager never creates a P2P connection to the local member.
- Receiving `p2p_offer` creates or reuses the target PeerConnection, sets remote
  description, creates an answer, and sends `p2p_answer`.
- Receiving `p2p_answer` applies the answer only to that member's connection.
- Receiving `p2p_ice_candidate` adds the candidate only to that member's
  connection.
- Local ICE sends `p2p_ice_candidate` with the correct `target_member_id`.
- Failed, disconnected, timeout, or unsupported state sends
  `p2p_connection_failed` for only that member pair.
- Closing one member connection does not close other P2P connections.
- Closing the manager closes every active PeerConnection.
- Audio, camera, and screen tracks are added to existing P2P connections with
  distinguishable metadata.
- Starting camera or screen share after P2P is already connected publishes the
  new track to all non-fallback P2P peers.
- Stopping camera or screen share removes or replaces only that media source.
- Applying `media_route_updated(route: "sfu")` disables P2P receive for that
  member pair without touching other pairs.

### Composable and Boundary Tests

Extend `tests/frontend/room-session-boundaries.test.mjs` or add a focused
boundary test when Phase 3 wires the manager:

- `useRoomSession` owns P2P through a boundary module or ref instead of
  constructing PeerConnections inline.
- `handleRoomSignal()` dispatches P2P signals before normal room snapshot
  processing.
- Existing SFU `ice_candidate` and `renegotiation_needed` handling still call
  `MediaSession`.
- `room_closed`, reconnect reset, leave, and unmount close P2P and SFU
  resources.
- Member leave closes only that member's P2P connection.
- Route fallback for A-B does not stop A-C or B-C P2P connections.

Extend `tests/frontend/room-connection.test.mjs` only if `RoomConnection`
starts caching route state. Otherwise its current raw-signal pass-through
behavior already supports P2P.

## Phase 4 Browser Tests

Phase 4 should add an end-to-end spec such as
`tests/browser/p2p-media.spec.mjs`.

Recommended coverage:

- Two browser contexts enter the same room and both see the other member.
- Browser media APIs are mocked so tests do not require real microphone,
  camera, or screen permissions.
- Default join triggers P2P offer/answer/ICE signaling.
- Camera video can be displayed through P2P.
- Screen share video can be displayed through P2P.
- A test-only switch can force one member pair's P2P failure.
- Forced A-B failure switches only A-B to SFU fallback.
- In a three-member room, A-B fallback does not stop A-C or B-C P2P.
- Refresh and resume close stale PeerConnections and create fresh ones as
  needed.
- Member leave closes the leaving member's P2P connections.
- Owner leave closes the room and all P2P resources.

## Acceptance Commands

Phase 1:

```bash
git status --short
```

Phase 2:

```bash
cargo test --test room_permissions
cargo test --test signaling_ws
```

Phase 3:

```bash
npm run test:frontend
npm run build:frontend
```

Phase 4:

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
```

Later service split phases should follow the acceptance commands defined in the
long-term rollout plan.
