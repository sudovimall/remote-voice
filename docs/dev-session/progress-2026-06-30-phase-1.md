# Phase 1 Progress - Vue Dev Backend Proxy

Date: 2026-06-30

## Completed

- Confirmed the active backend port is `18080` from `application.yaml`.
- Reproduced the Vite development failure: `/api/rooms` and `/ws` were handled by Vite because `base` was fixed to `/ui/` and no backend proxy existed.
- Updated Vite configuration so development uses `/` and proxies `/api` plus `/ws` to the Rust backend.
- Kept production build assets under `/ui/assets/*` for the existing Rust static route.
- Updated the built-in fallback config, Dockerfile, Nginx, and README references from `8080` to `18080`.
- Added frontend tests for Vite base selection, proxy defaults, backend override, and Vite dev WebSocket URL behavior.

## Verification

Completed:

```bash
npm run test:frontend
npm run build:frontend
cargo test
npm run test:browser
```

Development proxy behavior was also verified with:

```bash
cargo run
npm run dev:frontend
```

Results:

- `http://127.0.0.1:5173/api/rooms` returned backend JSON with HTTP 200.
- `http://127.0.0.1:5173/ws` returned the backend WebSocket upgrade error instead of a Vite 404.
- `http://127.0.0.1:5173/rooms/ABC123` returned the Vite SPA entry page.

## Notes

- Existing untracked files outside this phase were left untouched.
- The legacy `static` cleanup is intentionally deferred to phase 2.
