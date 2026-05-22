import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const html = readFileSync(new URL("../../static/room.html", import.meta.url), "utf8");
const css = readFileSync(new URL("../../static/styles.css", import.meta.url), "utf8");

test("room side panel uses hidden as the exclusive members/chat switch", () => {
  assert.match(html, /id="member-list"[\s\S]*class="member-list"/);
  assert.match(html, /id="chat-panel"[\s\S]*class="chat-panel"[\s\S]*hidden/);
  assert.match(css, /\[hidden\]\s*\{[\s\S]*display:\s*none\s*!important;[\s\S]*\}/);
});

test("room chat toggle is a compact corner tab with an unread badge", () => {
  assert.match(html, /id="panel-toggle"[\s\S]*class="panel-switch"/);
  assert.match(html, /id="chat-unread"[\s\S]*class="chat-unread"[\s\S]*hidden/);
  assert.match(css, /\.panel-switch-tab/);
  assert.match(css, /clip-path:\s*polygon\(100% 0,\s*100% 100%,\s*0 0\)/);
});

test("desktop room layout keeps page fixed and scrolls inside panels", () => {
  assert.match(css, /body\[data-page="voice-room"\]\s*\{[\s\S]*overflow:\s*hidden;[\s\S]*\}/);
  assert.match(css, /\.room-shell\s*\{[\s\S]*height:\s*100dvh;[\s\S]*grid-template-rows:\s*auto auto minmax\(0,\s*1fr\);[\s\S]*overflow:\s*hidden;[\s\S]*\}/);
  assert.match(css, /\.room-grid\s*\{[\s\S]*min-height:\s*0;[\s\S]*\}/);
  assert.match(css, /\.member-list\s*\{[\s\S]*overflow:\s*auto;[\s\S]*\}/);
  assert.match(css, /\.chat-messages\s*\{[\s\S]*min-height:\s*0;[\s\S]*overflow:\s*auto;[\s\S]*\}/);
  assert.match(css, /@media \(max-width:\s*900px\)\s*\{[\s\S]*body\[data-page="voice-room"\]\s*\{[\s\S]*overflow:\s*auto;[\s\S]*\}/);
});
