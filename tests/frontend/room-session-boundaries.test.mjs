import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const roomSession = readFileSync(
  new URL("../../frontend/src/composables/useRoomSession.js", import.meta.url),
  "utf8",
);

function readComposable(name) {
  return readFileSync(new URL(`../../frontend/src/composables/${name}`, import.meta.url), "utf8");
}

test("useRoomSession is a composition layer instead of directly constructing transports", () => {
  assert.match(roomSession, /useRoomConnectionSession/);
  assert.match(roomSession, /useRoomMediaSession/);
  assert.match(roomSession, /useRoomChatSession/);
  assert.match(roomSession, /useRoomScreenShareSession/);
  assert.match(roomSession, /useRoomMemberPreferences/);
  assert.match(roomSession, /useRoomP2PSession/);
  assert.doesNotMatch(roomSession, /new RoomConnection\(/);
  assert.doesNotMatch(roomSession, /new MediaSession\(/);
  assert.doesNotMatch(roomSession, /new RTCPeerConnection\(/);
});

test("room responsibilities have dedicated composable modules", () => {
  assert.match(readComposable("useRoomConnectionSession.js"), /export function useRoomConnectionSession/);
  assert.match(readComposable("useRoomMediaSession.js"), /export function useRoomMediaSession/);
  assert.match(readComposable("useRoomChatSession.js"), /export function useRoomChatSession/);
  assert.match(readComposable("useRoomScreenShareSession.js"), /export function useRoomScreenShareSession/);
  assert.match(readComposable("useRoomMemberPreferences.js"), /export function useRoomMemberPreferences/);
  assert.match(readComposable("useRoomP2PSession.js"), /export function useRoomP2PSession/);
});

test("room composition layer dispatches P2P before room snapshots", () => {
  assert.match(roomSession, /p2p\.handleP2PSignal\(signal\)/);
  assert.match(roomSession, /nextRoomSnapshot\(currentRoom\.value, signal\)/);
  assert.ok(
    roomSession.indexOf("p2p.handleP2PSignal(signal)") <
      roomSession.indexOf("nextRoomSnapshot(currentRoom.value, signal)"),
  );
});

test("room composition layer still exposes the RoomView contract", () => {
  for (const key of [
    "activeSidePanel",
    "cameraBusy",
    "cameraStateLabel",
    "cameraToggleLabel",
    "chatInput",
    "chatMessages",
    "connectionLabel",
    "currentRoom",
    "leaveRoom",
    "localCameraStream",
    "mediaReady",
    "memberVolume",
    "members",
    "ownMemberId",
    "remoteCameraStreams",
    "screenShareTitle",
    "setActiveSidePanel",
    "startScreenShare",
    "submitChatMessage",
    "toggleCamera",
    "toggleSelfMuted",
    "voiceState",
  ]) {
    assert.match(roomSession, new RegExp(`\\b${key}\\b`), `${key} remains part of the public contract`);
  }
});
