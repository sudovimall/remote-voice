# Phase 2 Handoff - Legacy Static UI Archived

## Current State

- Vue/Vite owns the lobby and room UI.
- Legacy lobby and room static files are archived in `docs/dev-session/archive/static-legacy-2026-06-30/`.
- Rust `/assets/{asset}` now serves only:
  - `styles.css`
  - `auth-page.js`
  - `admin.js`
- `static/` now contains only active auth/admin files plus ignored Vite build output.

## Files Changed In Phase 2

- `src/transport/http/mod.rs`
- `tests/frontend/audio-volume.test.mjs`
- `tests/frontend/auth-ui.test.mjs`
- `tests/frontend/chat-controls.test.mjs`
- `tests/frontend/lobby-rooms.test.mjs`
- `tests/frontend/room-connection.test.mjs`
- `tests/frontend/room-controls.test.mjs`
- `tests/frontend/room-entry.test.mjs`
- `tests/frontend/signaling-client.test.mjs`
- `tests/frontend/room-layout.test.mjs`
- `docs/dev-session/archive/static-legacy-2026-06-30/*`
- `docs/dev-session/progress-2026-06-30-phase-2.md`
- `docs/dev-session/handoff-2026-06-30-phase-2.md`

## Next Phase Goal

The remaining cleanup path is to migrate login, register, and admin pages into Vue or a separate maintained frontend area. Do not delete the current static auth/admin files until those routes have replacements.

Recommended next order:

1. Decide whether auth/admin should become Vue routes inside `frontend/src` or stay as small standalone static pages.
2. If migrating to Vue, add route-aware components for `/login`, `/register`, and `/admin`.
3. Move `safeNextPath` coverage off `static/auth-page.js` only after the Vue replacement exists.
4. Remove `/assets/styles.css`, `/assets/auth-page.js`, and `/assets/admin.js` only after auth/admin pages no longer load them.
5. Run `npm run test:frontend`, `npm run build:frontend`, `cargo test`, and `npm run test:browser`.

## Assumptions

- Archived legacy files are kept for reference only and are no longer served by the backend.
- `static/dist` stays ignored build output.
