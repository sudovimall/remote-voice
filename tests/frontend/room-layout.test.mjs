import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const html = readFileSync(new URL("../../static/room.html", import.meta.url), "utf8");
const css = readFileSync(new URL("../../static/styles.css", import.meta.url), "utf8");
const roomJs = readFileSync(new URL("../../static/room.js", import.meta.url), "utf8");

test("room side panel uses hidden as the exclusive members/chat switch", () => {
  assert.match(html, /id="member-list"[\s\S]*class="member-list"/);
  assert.match(html, /id="chat-panel"[\s\S]*class="chat-panel"[\s\S]*hidden/);
  assert.match(css, /\[hidden\]\s*\{[\s\S]*display:\s*none\s*!important;[\s\S]*\}/);
});

test("room side panel exposes members chat and screen tabs", () => {
  assert.match(html, /id="members-tab"[\s\S]*data-panel="members"/);
  assert.match(html, /id="chat-tab"[\s\S]*data-panel="chat"/);
  assert.match(html, /id="screen-tab"[\s\S]*data-panel="screen"/);
  assert.match(html, /id="chat-unread"[\s\S]*class="chat-unread"[\s\S]*hidden/);
  assert.match(css, /\.panel-tabs\b/);
  assert.match(css, /\.panel-tab-active\b/);
});

test("room screen panel contains sharing controls and popout", () => {
  assert.match(html, /id="screen-panel"[\s\S]*class="screen-panel"[\s\S]*hidden/);
  assert.match(html, /id="start-screen-share"/);
  assert.match(html, /id="stop-screen-share"/);
  assert.match(html, /id="open-screen-popout"/);
  assert.match(html, /id="fullscreen-screen-share"/);
  assert.match(html, /id="screen-popout"[\s\S]*class="screen-popout"[\s\S]*hidden/);
  assert.match(css, /\.screen-video-frame\b/);
  assert.match(css, /\.screen-popout\b/);
});

test("room chat mentions use inline picker, highlight, and passive reminder", () => {
  assert.match(html, /id="mention-picker"[\s\S]*class="mention-picker"[\s\S]*hidden/);
  assert.match(html, /id="mention-reminder"[\s\S]*class="mention-reminder"[\s\S]*hidden/);
  assert.doesNotMatch(
    html,
    /id="chat-panel"[\s\S]*id="mention-reminder"[\s\S]*<\/section>/,
  );
  assert.match(css, /\.chat-mention\b/);
  assert.match(css, /\.mention-picker\b/);
  assert.match(css, /\.mention-reminder\b/);
  assert.match(roomJs, /messageMentionsCurrentMember/);
  assert.match(roomJs, /MENTION_REMINDER_MS\s*=\s*10000/);
  assert.doesNotMatch(roomJs, /\b(prompt|alert|confirm)\s*\(/);
});

test("room voice page exposes local volume controls", () => {
  assert.match(html, /id="microphone-gain"[\s\S]*type="range"[\s\S]*max="2"/);
  assert.match(html, /id="microphone-gain-value"/);
  assert.match(roomJs, /volumeInput\.max\s*=\s*"1"/);
  assert.match(css, /\.volume-control\b/);
  assert.match(css, /\.member-volume-control\b/);
  assert.match(css, /\.microphone-gain-control\b/);
  assert.match(roomJs, /from "\/assets\/audio-volume\.mjs"/);
  assert.match(roomJs, /setMemberVolume/);
  assert.match(roomJs, /setMicrophoneGain/);
});

test("room local voice controls stay compact", () => {
  assert.match(css, /\.voice-pane\s*\{[\s\S]*gap:\s*16px;[\s\S]*\}/);
  assert.match(css, /\.mic-button\s*\{[\s\S]*min-height:\s*64px;[\s\S]*\}/);
  assert.match(css, /\.mic-button\s*\{[\s\S]*font-size:\s*0\.96rem;[\s\S]*\}/);
  assert.match(css, /\.microphone-gain-control\s*\{[\s\S]*min-height:\s*44px;[\s\S]*\}/);
  assert.match(css, /\.microphone-gain-control\s*\{[\s\S]*padding:\s*8px 12px;[\s\S]*\}/);
});

test("desktop room layout keeps page fixed and scrolls inside panels", () => {
  assert.match(css, /body\[data-page="voice-room"\]\s*\{[\s\S]*overflow:\s*hidden;[\s\S]*\}/);
  assert.match(css, /\.room-shell\s*\{[\s\S]*height:\s*100dvh;[\s\S]*grid-template-rows:\s*auto auto minmax\(0,\s*1fr\);[\s\S]*overflow:\s*hidden;[\s\S]*\}/);
  assert.match(css, /\.room-grid\s*\{[\s\S]*min-height:\s*0;[\s\S]*\}/);
  assert.match(css, /\.member-list\s*\{[\s\S]*overflow:\s*auto;[\s\S]*\}/);
  assert.match(css, /\.chat-messages\s*\{[\s\S]*min-height:\s*0;[\s\S]*overflow:\s*auto;[\s\S]*\}/);
  assert.match(css, /@media \(max-width:\s*900px\)\s*\{[\s\S]*body\[data-page="voice-room"\]\s*\{[\s\S]*overflow:\s*auto;[\s\S]*\}/);
});

test("room media startup failure does not always mark microphone permission denied", () => {
  assert.doesNotMatch(
    roomJs,
    /catch\s*\([^)]*\)\s*\{[\s\S]*renderVoiceState\(\{\s*device:\s*"denied",\s*media:\s*"failed"\s*\}\)/,
  );
  assert.match(
    roomJs,
    /catch\s*\([^)]*\)\s*\{[\s\S]*renderVoiceState\(\{\s*media:\s*"failed"\s*\}\)/,
  );
});

test("room page does not close media or websocket from page lifecycle handlers", () => {
  assert.doesNotMatch(roomJs, /addEventListener\(\s*["']unload["']/);
  assert.doesNotMatch(roomJs, /addEventListener\(\s*["']beforeunload["']/);
  assert.doesNotMatch(roomJs, /addEventListener\(\s*["']pagehide["'][\s\S]*?(mediaSession|client)\?\.close\(\)/);
});

test("speaking microphone icon is visually placed after the nickname", () => {
  assert.match(
    roomJs,
    /const speakingIndicator = textNode\("span",\s*"member-speaking-indicator",\s*""\);[\s\S]*nameLine\.append\(textNode\("strong",\s*"",\s*member\.nickname\),\s*speakingIndicator\);[\s\S]*textNode\("span",\s*"member-state",\s*memberStateLabel\(member\)\)/,
  );
  assert.match(css, /\.member-name-line\s*\{[\s\S]*width:\s*fit-content;[\s\S]*\}/);
  assert.match(css, /\.member-state\s*\{[\s\S]*display:\s*block;[\s\S]*\}/);
  assert.match(css, /\.member-speaking-indicator\s*\{[\s\S]*visibility:\s*hidden;[\s\S]*\}/);
  assert.match(css, /\.member-speaking-indicator-active\s*\{[\s\S]*visibility:\s*visible;[\s\S]*\}/);
});

test("member identity layout selectors do not override nickname row flex layout", () => {
  assert.doesNotMatch(css, /\.member-identity div\s*\{/);
  assert.doesNotMatch(css, /\.member-identity div span\s*\{/);
  assert.match(css, /\.member-identity\s*>\s*div\s*\{[\s\S]*display:\s*grid;[\s\S]*\}/);
  assert.match(css, /\.member-name-line\s*\{[\s\S]*display:\s*flex;[\s\S]*\}/);
});
