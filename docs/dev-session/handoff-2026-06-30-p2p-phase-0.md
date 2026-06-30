# Phase 0 Handoff - P2P Media Rollout Documentation

## Current State

- The long-term implementation plan has been written to `docs/superpowers/plans/2026-06-30-p2p-media-and-service-split.md`.
- No runtime code has been changed.
- No tests have been run because this was a documentation-only phase.

## Completed Progress

- Defined the overall task order:
  1. Implement P2P-first media for screen sharing and video calls.
  2. Keep SFU as the fallback path.
  3. Fall back per member pair, not per whole room.
  4. Split backend code into service-style modules only after the P2P task is complete.
- Defined the expected phase workflow:
  - Read the latest handoff at phase start.
  - Use a main agent plus scoped subagents for each implementation phase.
  - Generate progress and handoff docs before each phase ends.
  - Run required tests.
  - Commit only phase-related files.
- Documented the proposed P2P signal shape and service split direction.

## Next Phase Goal

Phase 1 should turn the plan into a decision-complete technical design and test skeleton without changing runtime behavior.

Recommended Phase 1 order:

1. Read `docs/superpowers/plans/2026-06-30-p2p-media-and-service-split.md`.
2. Read current implementation around:
   - `src/transport/http/signaling.rs`
   - `src/domain/room.rs`
   - `src/media/mod.rs`
   - `frontend/src/lib/media-session.js`
   - `frontend/src/composables/useRoomMediaSession.js`
   - `frontend/src/lib/room-connection.js`
3. Use subagents for independent exploration:
   - Backend signaling and route-state boundaries.
   - Frontend P2P session manager integration points.
   - Test coverage and missing cases.
4. Produce Phase 1 protocol/test design docs.
5. End Phase 1 with progress and handoff docs plus a documentation commit.

## Phase 1 Acceptance Criteria

- P2P signal protocol is finalized enough for implementation.
- Media route state ownership is clearly assigned.
- Backend and frontend tests to add in Phase 2 and Phase 3 are listed.
- No runtime behavior changes are made in Phase 1.

## Assumptions

- P2P fallback remains per member pair.
- Existing SFU messages keep their current browser-to-server semantics.
- Main room UI changes should target Vue/Vite files under `frontend/src`.
- Backend service splitting starts only after P2P behavior is complete and verified.
