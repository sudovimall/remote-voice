# Phase 4 Handoff - Browser P2P Integration and Regression

Date: 2026-07-01

## Current State

- P2P-first media work from Phases 1-4 is implemented and covered by backend,
  frontend, build, and browser regression tests.
- Backend P2P signaling and per-member-pair SFU fallback remain unchanged from
  Phase 2.
- Frontend P2P session management remains unchanged in production behavior from
  Phase 3, with only test-only telemetry and fake PeerConnection injection added
  in this phase.
- Browser tests now cover P2P offer/answer/ICE, camera video, screen-share
  video, forced single-pair fallback, unrelated pair preservation, refresh
  cleanup, normal member leave cleanup, and owner-close cleanup.
- Backend service split has not started and should begin only in Phase 5.

## Completed Progress

- Added Playwright P2P harness:
  - fake P2P `RTCPeerConnection` implementation injected through
    `useRoomP2PSession`;
  - fake DataChannel metadata delivery;
  - fake screen capture stream;
  - test event collection from `P2PMediaSession`.
- Added browser assertions that:
  - two joined pages exchange P2P offer, answer, and ICE through the backend;
  - camera video reaches the receiving video grid via P2P;
  - screen-share video reaches the receiving screen panel via P2P;
  - a forced A-B P2P failure broadcasts `media_route_updated(route: "sfu")` to
    both affected pages;
  - A-C and C-A P2P peers remain active after A-B fallback;
  - refresh closes stale P2P peers and creates new ones;
  - ordinary member leave closes only that member's P2P peer;
  - owner leave closes the room and releases P2P peers on the remaining page.
- Added admin-invite registration inside the cleanup test so ordinary members
  use a different authenticated user from the persistent-room owner.

## Files Changed In Phase 4

- `frontend/src/lib/p2p-media-session.js`
- `frontend/src/composables/useRoomP2PSession.js`
- `tests/browser/p2p-media.spec.mjs`
- `docs/dev-session/progress-2026-07-01-p2p-phase-4.md`
- `docs/dev-session/handoff-2026-07-01-p2p-phase-4.md`

## Verification

Passed:

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
```

Observed final browser coverage:

```text
tests/browser/lobby-create-room.spec.mjs
tests/browser/p2p-media.spec.mjs
3 passed
```

## Unfinished Items

- Backend service split design and implementation have not started.
- Real deployment NAT/TURN behavior is not covered by the deterministic fake
  P2P browser harness. The current scope verifies that the application protocol,
  media UI, fallback routing, and cleanup behave correctly in browser pages.
- The test-only P2P hook is intentionally global and guarded by absence of
  `window.__remoteVoiceP2PTest`; keep it out of product feature logic.

## Next Phase Goal

Phase 5 should design the backend service split after the P2P feature has
passed full regression. It should not move large amounts of code yet.

Recommended Phase 5 order:

1. Read this handoff and the long-term plan:
   `docs/superpowers/plans/2026-06-30-p2p-media-and-service-split.md`.
2. Inspect current backend boundaries in:
   - `src/domain/room.rs`
   - `src/transport/http/signaling.rs`
   - `src/transport/http/auth.rs`
   - `src/media/mod.rs`
3. Produce a service split design document covering:
   - room lifecycle service;
   - member control service;
   - media route service;
   - chat service;
   - authenticated persistent-room service.
4. Define public method drafts, error mapping, and migration/test order.
5. Generate Phase 5 progress/handoff docs.
6. Run:

```bash
git status --short
```

## Suggested Phase 5 Agent Split

- Main agent: own service boundaries, final design, docs, and commit scope.
- Room-domain explorer: map room lifecycle and member-control methods.
- Transport explorer: map WebSocket/HTTP handler responsibilities that should
  remain transport-only.
- Auth/persistence explorer: map persistent-room ownership and close semantics.
- Review agent: check compatibility, test migration order, and Chinese comment
  requirements.

## Assumptions

- P2P remains the default member-pair route.
- `webrtc_*` messages remain SFU-only and must not be reused for member P2P.
- Existing JSON protocol fields stay backward compatible.
- Service split starts from design in Phase 5, then implementation in Phase 6.
