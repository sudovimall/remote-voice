import {
  lobbyRoomId,
  loadNickname,
  saveNickname,
  saveRoomEntryIntent,
} from "/assets/room-entry.mjs";
import { fetchRoomSummaries } from "/assets/lobby-rooms.mjs";

const nickname = document.querySelector("#nickname");
const roomId = document.querySelector("#room-id");
const error = document.querySelector("#lobby-error");
const status = document.querySelector("#lobby-status");
const createRoomForm = document.querySelector("#create-room");
const joinRoomForm = document.querySelector("#join-room");
const roomBrowserMeta = document.querySelector("#room-browser-meta");
const roomBrowserList = document.querySelector("#room-browser-list");
const refreshRooms = document.querySelector("#refresh-rooms");

nickname.value = loadNickname(window.localStorage);
roomId.value = lobbyRoomId(window.location.search);

function showError(message) {
  error.textContent = message;
  error.hidden = false;
}

function showPending(message) {
  error.hidden = true;
  status.textContent = message;
}

function submittedNickname(event) {
  event?.preventDefault();

  const value = nickname.value.trim();
  if (!value) {
    showError("先输入昵称。");
    nickname.focus();
    return "";
  }

  try {
    saveNickname(window.localStorage, value);
  } catch (_error) {
    showError("无法保存昵称，请检查浏览器本地存储。");
  }

  return value;
}

function enterRoom(intent, path, pendingMessage) {
  try {
    saveRoomEntryIntent(window.sessionStorage, intent);
    showPending(pendingMessage);
    window.location.assign(path);
  } catch (entryError) {
    showError(entryError.message || "无法准备房间入口。");
  }
}

function joinRoom(roomIdValue, nicknameValue) {
  enterRoom(
    {
      mode: "join",
      roomId: roomIdValue,
      nickname: nicknameValue,
    },
    `/rooms/${encodeURIComponent(roomIdValue)}`,
    `正在连接房间 ${roomIdValue}。`,
  );
}

function roomSummaryRow(room) {
  const row = document.createElement("article");
  row.className = "lobby-room-row";

  const copy = document.createElement("div");
  copy.append(
    textNode("strong", room.id),
    textNode("span", `${room.memberCount} 位成员`),
  );

  const join = textNode("button", "加入");
  join.type = "button";
  join.addEventListener("click", () => {
    const nicknameValue = submittedNickname();
    if (nicknameValue) {
      joinRoom(room.id, nicknameValue);
    }
  });

  row.append(copy, join);
  return row;
}

function textNode(tag, text) {
  const node = document.createElement(tag);
  node.textContent = text;
  return node;
}

function renderRooms(rooms) {
  roomBrowserMeta.textContent = `${rooms.length} 个房间`;
  if (rooms.length === 0) {
    roomBrowserList.replaceChildren(textNode("p", "当前没有房间。"));
    return;
  }

  roomBrowserList.replaceChildren(...rooms.map(roomSummaryRow));
}

async function loadRooms() {
  refreshRooms.disabled = true;
  roomBrowserMeta.textContent = "正在刷新";
  try {
    renderRooms(await fetchRoomSummaries(window.fetch.bind(window)));
  } catch (roomsError) {
    roomBrowserMeta.textContent = "刷新失败";
    roomBrowserList.replaceChildren(textNode("p", roomsError.message || "房间列表不可用。"));
  } finally {
    refreshRooms.disabled = false;
  }
}

createRoomForm.addEventListener("submit", (event) => {
  const nicknameValue = submittedNickname(event);
  if (!nicknameValue) {
    return;
  }

  enterRoom(
    {
      mode: "create",
      nickname: nicknameValue,
    },
    "/rooms/new",
    "正在建立房间连接。",
  );
});

joinRoomForm.addEventListener("submit", (event) => {
  const nicknameValue = submittedNickname(event);
  if (!nicknameValue) {
    return;
  }

  roomId.value = roomId.value.trim().toUpperCase();
  if (!roomId.value) {
    showError("输入房间号后再加入。");
    roomId.focus();
    return;
  }

  joinRoom(roomId.value, nicknameValue);
});

refreshRooms.addEventListener("click", () => {
  void loadRooms();
});

void loadRooms();
