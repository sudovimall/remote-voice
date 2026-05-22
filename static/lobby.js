import {
  loadNickname,
  saveNickname,
  saveRoomEntryIntent,
} from "/assets/room-entry.mjs";

const nickname = document.querySelector("#nickname");
const roomId = document.querySelector("#room-id");
const error = document.querySelector("#lobby-error");
const status = document.querySelector("#lobby-status");
const createRoomForm = document.querySelector("#create-room");
const joinRoomForm = document.querySelector("#join-room");

nickname.value = loadNickname(window.localStorage);

function showError(message) {
  error.textContent = message;
  error.hidden = false;
}

function showPending(message) {
  error.hidden = true;
  status.textContent = message;
}

function submittedNickname(event) {
  event.preventDefault();

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

  enterRoom(
    {
      mode: "join",
      roomId: roomId.value,
      nickname: nicknameValue,
    },
    `/rooms/${encodeURIComponent(roomId.value)}`,
    `正在连接房间 ${roomId.value}。`,
  );
});
