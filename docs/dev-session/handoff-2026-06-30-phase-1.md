# Phase 1 Handoff - Vue Dev Backend Proxy

## Current State

- Rust backend local port is `18080`.
- Vite development mode should be opened at `http://127.0.0.1:5173/`.
- Vite proxies `/api` and `/ws` to `http://127.0.0.1:18080` by default.
- Production build still emits `/ui/assets/app.js` and `/ui/assets/index.css`, matching the Rust `/ui/assets/{asset}` route.
- Nginx now proxies page and WebSocket traffic to `host.docker.internal:18080`.

## Files Changed In Phase 1

- `frontend/vite.config.js`
- `README.md`
- `Dockerfile`
- `deploy/nginx/nginx.conf`
- `src/config/settings.rs`
- `tests/frontend/vite-config.test.mjs`
- `tests/frontend/room-state.test.mjs`
- `docs/dev-session/progress-2026-06-30-phase-1.md`
- `docs/dev-session/handoff-2026-06-30-phase-1.md`

## Next Phase Goal

Archive legacy lobby and room files from `static` after moving remaining frontend tests off old `static/*.mjs` helpers.

Do not archive these yet:

- `static/login.html`
- `static/register.html`
- `static/admin.html`
- `static/auth-page.js`
- `static/admin.js`
- `static/styles.css`
- `static/dist`

Recommended phase 2 order:

1. Move frontend tests that import old `static/*.mjs` helpers to `frontend/src/lib/*.js`.
2. Replace or remove `tests/frontend/room-layout.test.mjs` coverage that still reads `static/room.html`.
3. Narrow `/assets/{asset}` in `src/transport/http/mod.rs` to only resources still used by auth/admin pages.
4. Archive old lobby and room implementation files under `docs/dev-session/archive/static-legacy-2026-06-30/`.
5. Run `npm run test:frontend`, `npm run build:frontend`, `cargo test`, and `npm run test:browser`.

## Assumptions

- The next session should continue from this document instead of re-planning the Vite proxy fix.
- Login, register, and admin pages remain static until a separate Vue migration phase.
