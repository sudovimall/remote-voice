# Browser E2E Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a formal Playwright browser test entry point for the rendered Remote Voice frontend.

**Architecture:** Keep browser tests separate from the existing DOM-free Node module tests. Playwright starts the Rust server, uses the real static pages over HTTP, and drives Chrome through the anonymous lobby-to-room flow. The first suite avoids authentication and real user media prompts by using Chromium fake media flags.

**Tech Stack:** Node 22, npm scripts, `@playwright/test`, system Google Chrome when available, Rust `cargo run`.

---

## File Structure

- Create `package.json`: npm scripts and Playwright dev dependency.
- Create `playwright.config.mjs`: Playwright server startup, Chrome launch flags, browser-test directory.
- Create `tests/browser/lobby-create-room.spec.mjs`: anonymous browser smoke flow.

## Tasks

### Task 1: Browser Test Harness

**Files:**
- Create: `package.json`
- Create: `playwright.config.mjs`
- Create: `tests/browser/lobby-create-room.spec.mjs`

- [ ] **Step 1: Write the failing browser test**

Create a Playwright test that opens `/`, fills a nickname, clicks `创建`, waits for `/rooms/{room_id}`, and asserts `已连接`, `1 位成员`, and the nickname in the member list.

- [ ] **Step 2: Run test to verify RED**

Run: `npm run test:browser`
Expected: FAIL before `@playwright/test` is installed.

- [ ] **Step 3: Install dependencies and generate lockfile**

Run: `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm install`
Expected: `node_modules` and `package-lock.json` are created without downloading a Playwright-managed browser.

- [ ] **Step 4: Run browser test to verify GREEN**

Run: `npm run test:browser`
Expected: PASS against the local Rust server.

- [ ] **Step 5: Run full verification**

Run: `cargo test`, `node --test tests/frontend/*.test.mjs`, `npm run test:browser`, and `git diff --check`.
Expected: all tests pass and no whitespace errors.
