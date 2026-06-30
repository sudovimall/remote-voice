# Phase 3 Handoff - Frontend P2P Media Session

Date: 2026-07-01

## Current State

- Backend P2P signaling and route state were completed in Phase 2.
- Frontend now has an independent P2P media manager that uses the Phase 2
  `p2p_*` protocol.
- `useRoomSession` starts P2P after room join/resume and handles backend P2P
  signals before normal room snapshot processing.
- `MediaSession` still owns browser media permissions and SFU fallback, but now
  exposes local track changes to P2P through a callback instead of private-field
  access.
- Dedicated browser end-to-end validation for P2P media and per-pair fallback
  has not been added yet.

## Completed Progress

- Added `P2PMediaSession` with:
  - one PeerConnection per remote member;
  - deterministic initial offer ownership by member ID ordering;
  - P2P offer, answer, and ICE send/receive;
  - per-member fallback reporting through `p2p_connection_failed`;
  - `media_route_updated(route: "sfu")` handling that closes only the affected
    P2P connection;
  - local audio, camera, and screen track publishing;
  - a metadata DataChannel so remote video tracks can be mapped to camera or
    screen sources;
  - P2P audio playback with per-member volume.
- Added `useRoomP2PSession` as the frontend boundary for P2P lifecycle and
  signal dispatch.
- Updated `useRoomMemberPreferences` so member volume applies to SFU and P2P.
- Updated `useRoomMediaSession` and `MediaSession` so local tracks are exposed
  through `onLocalMediaTrack`.
- Added and updated frontend tests for P2P manager behavior and composition
  boundaries.

## Files Changed In Phase 3

- `frontend/src/lib/p2p-media-session.js`
- `frontend/src/composables/useRoomP2PSession.js`
- `frontend/src/composables/useRoomSession.js`
- `frontend/src/composables/useRoomMediaSession.js`
- `frontend/src/composables/useRoomMemberPreferences.js`
- `frontend/src/lib/media-session.js`
- `tests/frontend/p2p-media-session.test.mjs`
- `tests/frontend/media-session.test.mjs`
- `tests/frontend/room-session-boundaries.test.mjs`
- `docs/dev-session/progress-2026-07-01-p2p-phase-3.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-3.md`

## Verification

Passed:

```bash
npm run test:frontend
npm run build:frontend
npm run test:browser
```

## Unfinished Items

- Add browser end-to-end tests that prove two browser contexts exchange P2P
  offer/answer/ICE after joining the same room.
- Mock browser microphone, camera, and screen APIs in Playwright so P2P media
  tests do not require real devices.
- Verify camera video and screen-share video render through P2P.
- Add a test-only way to force one member pair's P2P failure.
- Verify forced A-B fallback switches only A-B to SFU while A-C and B-C remain
  on P2P.
- Audit whether SFU warm fallback creates duplicate playback in real browsers
  and add route-aware suppression or publishing optimization if the Phase 4
  browser tests expose it.
- Backend service split has not started and must remain deferred.

## Next Phase Goal

Phase 4 should add browser-level P2P media and fallback coverage, then fix any
real-browser issues uncovered by those tests while keeping existing SFU
compatibility.

Recommended Phase 4 order:

1. Read this handoff plus:
   - `docs/superpowers/specs/2026-07-01-p2p-protocol-design.md`
   - `docs/superpowers/specs/2026-07-01-p2p-test-plan.md`
2. Inspect current browser test setup under `tests/browser/`.
3. Add media API mocks for microphone, camera, and screen share.
4. Add a browser spec for two-member P2P signaling and media display.
5. Add a controlled failure path to force one member pair to report
   `p2p_connection_failed`.
6. Extend to a three-member case proving one-pair fallback does not close other
   P2P pairs.
7. Run:

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
```

## Suggested Phase 4 Agent Split

- Main agent: own Playwright environment, failure-injection design, final
  integration, docs, and commit.
- Browser test worker: implement media mocks and two-member P2P assertions.
- Frontend fix worker: address duplicate playback, route suppression, or
  cleanup issues discovered by the browser tests.
- Review/explorer agent: verify SFU compatibility and Chinese comment coverage.

## Assumptions

- P2P remains the default route for member pairs.
- `media_route_updated.member_ids` is already normalized by the backend.
- Existing SFU negotiation remains the fallback path and should not be removed.
- Screen-share video and camera video must stay independently identifiable in
  frontend state.
