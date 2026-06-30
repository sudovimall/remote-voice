# Phase 2 Progress - Archive Legacy Static UI

Date: 2026-06-30

## Completed

- Moved frontend tests off legacy `static/*.mjs` helpers and onto `frontend/src/lib/*.js`.
- Removed the old static room layout test that depended on `static/room.html` and `static/room.js`; Vue room layout coverage remains in `vue-room-layout.test.mjs`.
- Narrowed `/assets/{asset}` so Rust only serves static assets still used by login, register, and admin pages.
- Archived the old lobby and room static implementation under `docs/dev-session/archive/static-legacy-2026-06-30/`.
- Left the active static auth/admin pages and Vite build output in place.

## Archived Files

- `static/index.html`
- `static/room.html`
- `static/lobby.js`
- `static/room.js`
- `static/audio-volume.mjs`
- `static/auth-ui.mjs`
- `static/chat-controls.mjs`
- `static/lobby-rooms.mjs`
- `static/media-session.mjs`
- `static/room-connection.mjs`
- `static/room-controls.mjs`
- `static/room-entry.mjs`
- `static/room-state.mjs`
- `static/signaling-client.mjs`

## Verification

Completed:

```bash
npm run test:frontend
npm run build:frontend
cargo test
npm run test:browser
```

## Notes

- `static/login.html`, `static/register.html`, `static/admin.html`, `static/auth-page.js`, `static/admin.js`, `static/styles.css`, and `static/dist` remain active.
- `tests/frontend/auth-page.test.mjs` still imports `static/auth-page.js` intentionally because login/register still use that script.
