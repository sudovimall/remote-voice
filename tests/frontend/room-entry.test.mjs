import assert from "node:assert/strict";
import test from "node:test";

import {
  ENTRY_INTENT_KEY,
  NICKNAME_KEY,
  ROOM_SESSION_KEY,
  clearRoomEntryIntent,
  clearRoomSession,
  directRoomEntry,
  loadNickname,
  lobbyRoomId,
  loadRoomEntryIntent,
  loadRoomSession,
  saveNickname,
  saveRoomEntryIntent,
  saveRoomSession,
} from "../../static/room-entry.mjs";

function memoryStorage() {
  const values = new Map();
  return {
    getItem(key) {
      return values.get(key) ?? null;
    },
    removeItem(key) {
      values.delete(key);
    },
    setItem(key, value) {
      values.set(key, value);
    },
    value(key) {
      return values.get(key);
    },
  };
}

test("nickname storage keeps a trimmed lobby nickname", () => {
  const storage = memoryStorage();

  assert.equal(loadNickname(storage), "");
  assert.equal(saveNickname(storage, "  房主  "), "房主");
  assert.equal(storage.value(NICKNAME_KEY), "房主");
  assert.equal(loadNickname(storage), "房主");
});

test("room entry intent stores create and join actions for the next page", () => {
  const storage = memoryStorage();

  assert.deepEqual(saveRoomEntryIntent(storage, { mode: "create", nickname: " 房主 " }), {
    mode: "create",
    nickname: "房主",
  });
  assert.deepEqual(loadRoomEntryIntent(storage, "new"), {
    mode: "create",
    nickname: "房主",
  });

  assert.deepEqual(
    saveRoomEntryIntent(storage, {
      mode: "join",
      roomId: " abc123 ",
      nickname: " 队友 ",
    }),
    {
      mode: "join",
      roomId: "ABC123",
      nickname: "队友",
    },
  );
  assert.deepEqual(loadRoomEntryIntent(storage, "ABC123"), {
    mode: "join",
    roomId: "ABC123",
    nickname: "队友",
  });
});

test("room entry intent rejects a mismatched join route and can be cleared", () => {
  const storage = memoryStorage();
  saveRoomEntryIntent(storage, {
    mode: "join",
    roomId: "ABC123",
    nickname: "队友",
  });

  assert.equal(loadRoomEntryIntent(storage, "OTHER"), null);
  clearRoomEntryIntent(storage);
  assert.equal(storage.value(ENTRY_INTENT_KEY), undefined);
});

test("direct room links join with a saved nickname or return to a prefilled lobby", () => {
  const storage = memoryStorage();
  saveNickname(storage, " 队友 ");

  assert.deepEqual(directRoomEntry(storage, " uin90k "), {
    mode: "join",
    roomId: "UIN90K",
    nickname: "队友",
  });
  assert.deepEqual(directRoomEntry(memoryStorage(), " uin90k "), {
    lobbyPath: "/?room=UIN90K",
  });
  assert.equal(lobbyRoomId("?room=uin90k"), "UIN90K");
  assert.equal(lobbyRoomId("?other=value"), "");
});

test("room session stores resume credentials for the matching route", () => {
  const storage = memoryStorage();

  assert.deepEqual(
    saveRoomSession(storage, {
      roomId: " abc123 ",
      memberId: " m_owner ",
      resumeToken: " token ",
      nickname: " 房主 ",
    }),
    {
      roomId: "ABC123",
      memberId: "m_owner",
      resumeToken: "token",
      nickname: "房主",
    },
  );
  assert.deepEqual(loadRoomSession(storage, "abc123"), {
    roomId: "ABC123",
    memberId: "m_owner",
    resumeToken: "token",
    nickname: "房主",
  });
  assert.equal(loadRoomSession(storage, "other"), null);
  assert.match(storage.value(ROOM_SESSION_KEY), /"resumeToken":"token"/);
});

test("room session ignores invalid resume credentials and can be cleared", () => {
  const storage = memoryStorage();
  storage.setItem(ROOM_SESSION_KEY, JSON.stringify({ roomId: "ABC123", memberId: "m_owner" }));

  assert.equal(loadRoomSession(storage, "ABC123"), null);

  saveRoomSession(storage, {
    roomId: "ABC123",
    memberId: "m_owner",
    resumeToken: "token",
    nickname: "房主",
  });
  clearRoomSession(storage);
  assert.equal(storage.value(ROOM_SESSION_KEY), undefined);
});
