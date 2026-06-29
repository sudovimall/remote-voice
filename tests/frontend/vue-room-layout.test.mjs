import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const roomView = readFileSync(new URL("../../frontend/src/components/RoomView.vue", import.meta.url), "utf8");
const css = readFileSync(new URL("../../frontend/src/styles.css", import.meta.url), "utf8");
const roomSession = readFileSync(
  new URL("../../frontend/src/composables/useRoomSession.js", import.meta.url),
  "utf8",
);
const chatSession = readFileSync(
  new URL("../../frontend/src/composables/useRoomChatSession.js", import.meta.url),
  "utf8",
);
const membersPanel = readFileSync(
  new URL("../../frontend/src/components/room/MembersPanel.vue", import.meta.url),
  "utf8",
);
const chatPanel = readFileSync(
  new URL("../../frontend/src/components/room/ChatPanel.vue", import.meta.url),
  "utf8",
);
const screenSharePanel = readFileSync(
  new URL("../../frontend/src/components/room/ScreenSharePanel.vue", import.meta.url),
  "utf8",
);
const videoGridPanel = readFileSync(
  new URL("../../frontend/src/components/room/VideoGridPanel.vue", import.meta.url),
  "utf8",
);
const voicePanel = readFileSync(
  new URL("../../frontend/src/components/room/VoicePanel.vue", import.meta.url),
  "utf8",
);
const chatNotifications = readFileSync(
  new URL("../../frontend/src/components/room/ChatNotifications.vue", import.meta.url),
  "utf8",
);

test("Vue room view renders members chat and screen panels in the center stage", () => {
  const stageIndex = roomView.indexOf('class="stage-pane"');
  const videoGridIndex = roomView.indexOf("<VideoGridPanel");
  const memberListIndex = roomView.indexOf("<MembersPanel");
  const chatPanelIndex = roomView.indexOf("<ChatPanel");
  const screenPanelIndex = roomView.indexOf("<ScreenSharePanel");
  const voicePaneIndex = roomView.indexOf("<VoicePanel");

  assert.ok(stageIndex > 0, "stage pane exists");
  assert.ok(videoGridIndex > stageIndex, "video grid is inside center stage");
  assert.ok(memberListIndex > videoGridIndex, "members follow the video grid inside center stage");
  assert.ok(chatPanelIndex > memberListIndex, "chat follows members inside center stage");
  assert.ok(screenPanelIndex > chatPanelIndex, "screen share follows chat inside center stage");
  assert.ok(voicePaneIndex > screenPanelIndex, "voice pane stays outside the center stage");
  assert.match(videoGridPanel, /id="video-grid-panel"/);
  assert.match(membersPanel, /id="member-list"/);
  assert.match(chatPanel, /id="chat-panel"/);
  assert.match(screenSharePanel, /id="screen-panel"/);
  assert.doesNotMatch(roomView, /class="stage-idle"/);
  assert.match(css, /\.stage-content\b/);
  assert.match(css, /\.video-grid-panel\b/);
  assert.match(css, /\.video-tile\b/);
});

test("Vue room view wires camera controls to the media boundary", () => {
  assert.match(roomView, /:local-camera-stream="room\.localCameraStream"/);
  assert.match(roomView, /:remote-camera-streams="room\.remoteCameraStreams"/);
  assert.match(roomView, /:video-call-publishers="room\.currentRoom\?\.video_call_publishers \?\? \{\}"/);
  assert.match(roomView, /@toggle-camera="room\.toggleCamera"/);
  assert.match(voicePanel, /id="toggle-camera"/);
  assert.match(voicePanel, /id="camera-state"/);
  assert.match(roomSession, /\bhandleVideoCallStarted\b/);
  assert.match(roomSession, /\bhandleVideoCallStopped\b/);
  assert.match(roomSession, /\bapplyVideoCallPublisherCount\b/);
});

test("incoming chat messages show a temporary toast notification", () => {
  assert.match(roomView, /<ChatNotifications\b/);
  assert.match(chatNotifications, /class="chat-toast-container"/);
  // Toast/mention behavior now lives in the chat boundary composable rather
  // than the composition layer, so the assertions follow the implementation.
  assert.match(chatSession, /function showChatToast\(/);
  assert.match(chatSession, /function clearChatToast\(/);
  assert.match(
    chatSession,
    /function handleChatMessage\(message\)\s*\{[\s\S]*showChatToast\(message\);[\s\S]*renderUnreadBadge\(\);/,
  );
  assert.match(css, /\.chat-toast-container\b/);
  assert.match(css, /\.chat-toast\b/);
});

test("Vue room view owns its lifecycle instead of loading the legacy room script", () => {
  assert.doesNotMatch(roomView, /\/assets\/room\.js/);
  assert.match(roomView, /useRoomSession\(/);
  assert.match(roomSession, /onMounted\(/);
  assert.match(roomSession, /onBeforeUnmount\(/);
  assert.match(roomSession, /useRoomConnectionSession/);
  assert.match(roomSession, /useRoomMediaSession/);
  assert.doesNotMatch(roomSession, /new RoomConnection\(/);
  assert.doesNotMatch(roomSession, /new MediaSession\(/);
});

test("mention notification is global and not tied to opening the chat panel", () => {
  assert.match(roomView, /<ChatNotifications\b/);
  assert.match(chatSession, /function showMentionReminder\(message\)\s*\{/);
  assert.doesNotMatch(
    chatSession,
    /showMentionReminder\(message\)[\s\S]*activeSidePanel\.value\s*===\s*"chat"/,
  );
  assert.match(chatSession, /messageMentionsCurrentMember\(message,\s*ownMemberId\.value\)/);
});
