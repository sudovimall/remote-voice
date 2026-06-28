import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const lobbyView = readFileSync(
  new URL("../../frontend/src/components/LobbyView.vue", import.meta.url),
  "utf8",
);

test("Vue lobby owns behavior instead of loading the legacy lobby script", () => {
  assert.doesNotMatch(lobbyView, /\/assets\/lobby\.js/);
  assert.doesNotMatch(lobbyView, /document\.querySelector/);
  assert.doesNotMatch(lobbyView, /replaceChildren/);
  assert.doesNotMatch(lobbyView, /addEventListener/);
});

test("Vue lobby uses reactive form and room list bindings", () => {
  assert.match(lobbyView, /useLobbySession\(/);
  assert.match(lobbyView, /v-model="lobby\.nickname"/);
  assert.match(lobbyView, /v-model="lobby\.roomId"/);
  assert.match(lobbyView, /@submit\.prevent="lobby\.createRoom"/);
  assert.match(lobbyView, /@submit\.prevent="lobby\.joinEnteredRoom"/);
  assert.match(lobbyView, /v-for="room in lobby\.rooms"/);
  assert.match(lobbyView, /@click="lobby\.refreshRooms"/);
  assert.match(lobbyView, /:disabled="lobby\.roomsLoading"/);
});
