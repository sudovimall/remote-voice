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
