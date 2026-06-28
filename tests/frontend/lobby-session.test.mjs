import assert from "node:assert/strict";
import test from "node:test";

import { ENTRY_INTENT_KEY } from "../../frontend/src/lib/room-entry.js";
import { useLobbySession } from "../../frontend/src/composables/useLobbySession.js";

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

function lobbyHarness({ search = "", localStorage = memoryStorage(), fetchImpl } = {}) {
  const sessionStorage = memoryStorage();
  const navigations = [];
  const requests = [];
  const fetch =
    fetchImpl ??
    (async (path, options) => {
      requests.push([path, options]);
      if (path === "/api/auth/me") {
        return {
          ok: true,
          async json() {
            return { auth_enabled: false };
          },
        };
      }
      return {
        ok: true,
        async json() {
          return [];
        },
      };
    });
  const lobby = useLobbySession({
    fetchImpl: fetch,
    localStorage,
    location: { search },
    navigate(path) {
      navigations.push(path);
    },
    sessionStorage,
  });

  return { lobby, localStorage, navigations, requests, sessionStorage };
}

test("lobby initializes nickname and prefilled room id", () => {
  const localStorage = memoryStorage();
  localStorage.setItem("remote-voice.nickname", " 队友 ");

  const { lobby } = lobbyHarness({ localStorage, search: "?room=abc123" });

  assert.equal(lobby.nickname.value, "队友");
  assert.equal(lobby.roomId.value, "ABC123");
});

test("empty nickname blocks create and join without writing an entry intent", () => {
  const { lobby, navigations, sessionStorage } = lobbyHarness();

  lobby.createRoom();
  assert.equal(lobby.errorMessage.value, "先输入昵称。");
  assert.equal(sessionStorage.value(ENTRY_INTENT_KEY), undefined);
  assert.deepEqual(navigations, []);

  lobby.roomId.value = "ABC123";
  lobby.joinEnteredRoom();
  assert.equal(lobby.errorMessage.value, "先输入昵称。");
  assert.equal(sessionStorage.value(ENTRY_INTENT_KEY), undefined);
  assert.deepEqual(navigations, []);
});

test("creating a room stores create intent and navigates to the new room route", () => {
  const { lobby, localStorage, navigations, sessionStorage } = lobbyHarness();
  lobby.nickname.value = " 房主 ";

  lobby.createRoom();

  assert.equal(localStorage.value("remote-voice.nickname"), "房主");
  assert.deepEqual(JSON.parse(sessionStorage.value(ENTRY_INTENT_KEY)), {
    mode: "create",
    nickname: "房主",
  });
  assert.deepEqual(navigations, ["/rooms/new"]);
  assert.equal(lobby.statusMessage.value, "正在建立房间连接。");
});

test("joining a typed room stores join intent and navigates to the normalized room route", () => {
  const { lobby, navigations, sessionStorage } = lobbyHarness();
  lobby.nickname.value = " 队友 ";
  lobby.roomId.value = " abc123 ";

  lobby.joinEnteredRoom();

  assert.deepEqual(JSON.parse(sessionStorage.value(ENTRY_INTENT_KEY)), {
    mode: "join",
    roomId: "ABC123",
    nickname: "队友",
  });
  assert.deepEqual(navigations, ["/rooms/ABC123"]);
  assert.equal(lobby.statusMessage.value, "正在连接房间 ABC123。");
});

test("room list refresh renders success and failure states", async () => {
  const { lobby } = lobbyHarness({
    fetchImpl: async (path) => {
      if (path === "/api/auth/me") {
        return { ok: false };
      }
      return {
        ok: true,
        async json() {
          return [{ id: "abc123", member_count: 2 }];
        },
      };
    },
  });

  await lobby.refreshRooms();

  assert.equal(lobby.roomsMeta.value, "1 个房间");
  assert.deepEqual(lobby.rooms.value, [{ id: "ABC123", memberCount: 2 }]);

  const failing = lobbyHarness({
    fetchImpl: async () => ({ ok: false }),
  }).lobby;
  await failing.refreshRooms();

  assert.equal(failing.roomsMeta.value, "刷新失败");
  assert.equal(failing.roomListMessage.value, "房间列表刷新失败。");
});

test("auth state can fill an empty nickname and expose admin controls", async () => {
  const { lobby } = lobbyHarness({
    fetchImpl: async () => ({
      ok: true,
      async json() {
        return {
          auth_enabled: true,
          user: { display_name: "管理员", role: "admin", username: "admin" },
        };
      },
    }),
  });

  await lobby.loadAuthState();

  assert.equal(lobby.nickname.value, "管理员");
  assert.equal(lobby.authName.value, "管理员");
  assert.equal(lobby.showAdminLink.value, true);
});
