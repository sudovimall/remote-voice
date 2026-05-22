import {
  clearRoomEntryIntent,
  clearRoomSession,
  directRoomEntry,
  loadRoomEntryIntent,
  loadRoomSession,
  saveRoomSession,
} from "/assets/room-entry.mjs";
import {
  createRoomSignal,
  joinRoomSignal,
  membersForRoom,
  nextRoomSnapshot,
  resumeRoomSignal,
  websocketUrl,
} from "/assets/room-state.mjs";
import {
  canManageMember,
  memberCanSpeakSignal,
  memberPermissionLabel,
  selfMutedSignal,
} from "/assets/room-controls.mjs";
import { MediaSession } from "/assets/media-session.mjs";
import { SignalingClient } from "/assets/signaling-client.mjs";

const roomIdNode = document.querySelector("#room-id");
const roomError = document.querySelector("#room-error");
const connection = document.querySelector("#room-connection");
const membersMeta = document.querySelector("#members-meta");
const memberList = document.querySelector("#member-list");
const micState = document.querySelector("#mic-state");
const deviceState = document.querySelector("#device-state");
const mediaState = document.querySelector("#media-state");
const downlinkState = document.querySelector("#downlink-state");
const permissionNote = document.querySelector("#permission-note");
const muteSelf = document.querySelector("#mute-self");
const leaveRoom = document.querySelector("#leave-room");
const remoteAudio = document.querySelector("#remote-audio");
const roomSegments = window.location.pathname.split("/").filter(Boolean);
const routeRoomId = roomSegments[0] === "rooms" ? decodeRoomId(roomSegments[1]) : "";
let currentRoom = null;
let ownMemberId = "";
let client = null;
let mediaSession = null;
let mediaReady = false;
let roomSession = null;
let intentionalShutdown = false;
let pageHidden = false;
let reconnectTimer = null;
const voiceState = {
  device: "idle",
  media: "waiting",
  downlink: "waiting",
};

function decodeRoomId(rawRoomId) {
  try {
    return decodeURIComponent(rawRoomId ?? "").toUpperCase();
  } catch (_error) {
    return "";
  }
}

function roomPath(roomId) {
  return `/rooms/${encodeURIComponent(roomId)}`;
}

function showError(message) {
  roomError.hidden = false;
  roomError.textContent = message;
}

function setConnection(message) {
  connection.textContent = message;
}

function ownMember() {
  return currentRoom?.members?.[ownMemberId] ?? null;
}

function sendRoomControl(signal) {
  try {
    client?.send(signal);
  } catch (error) {
    showError(error.message || "房间控制发送失败。");
  }
}

function voiceLabel(group, state) {
  const labels = {
    device: {
      idle: "未请求权限",
      requesting: "请求中",
      authorized: "已授权",
      denied: "权限被拒绝",
    },
    media: {
      waiting: "等待连接",
      negotiating: "协商中",
      connected: "已连接",
      failed: "连接失败",
    },
    downlink: {
      waiting: "等待其他成员",
      track: "已收到音轨",
      playback_failed: "播放异常",
    },
  };

  return labels[group][state] ?? state;
}

function renderVoiceState(patch = {}) {
  Object.assign(voiceState, patch);
  deviceState.textContent = voiceLabel("device", voiceState.device);
  mediaState.textContent = voiceLabel("media", voiceState.media);
  downlinkState.textContent = voiceLabel("downlink", voiceState.downlink);

  const self = ownMember();
  if (self && !self.can_speak) {
    permissionNote.textContent = "房主已禁言，当前麦克风上行不会转发。";
  } else if (voiceState.device === "denied") {
    permissionNote.textContent = "麦克风权限被拒绝，房间状态仍会同步。";
  } else if (voiceState.media === "connected") {
    permissionNote.textContent = "语音链路已连接。";
  } else {
    permissionNote.textContent = "麦克风权限待确认。";
  }

  micState.lastChild.textContent =
    voiceState.media === "connected" ? " 麦克风已连接" : " 麦克风未连接";
  muteSelf.disabled = !mediaReady;
  muteSelf.textContent = self?.self_muted ? "取消静音" : "静音";
}

function avatarText(member) {
  return Array.from(member.nickname || "?")[0] ?? "?";
}

function speakingLabel(member) {
  if (!member.can_speak) {
    return "已禁言";
  }
  if (member.self_muted) {
    return "已静音";
  }

  return "可发言";
}

function memberStateLabel(member) {
  if (member.id === ownMemberId) {
    return "当前成员";
  }
  if (!member.connected) {
    return "待连接";
  }

  return "已连接";
}

function textNode(tag, className, text) {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  node.textContent = text;
  return node;
}

function renderMember(member, room) {
  const row = document.createElement("article");
  row.className = "member-row";
  if (member.id === room.owner_member_id) {
    row.classList.add("member-row-owner");
  }

  const identity = textNode("div", "member-identity", "");
  const avatar = textNode("span", "member-avatar", avatarText(member));
  if (member.id !== room.owner_member_id) {
    avatar.classList.add("member-avatar-muted");
  }

  const name = document.createElement("div");
  name.append(
    textNode("strong", "", member.nickname),
    textNode("span", "", memberStateLabel(member)),
  );
  identity.append(avatar, name);

  const signals = textNode("div", "member-signals", "");
  const owner = member.id === room.owner_member_id;
  signals.append(
    textNode("span", owner ? "role-chip" : "role-chip role-chip-muted", owner ? "房主" : "成员"),
  );

  const speaking = textNode("span", "signal-chip", speakingLabel(member));
  if (member.can_speak && !member.self_muted) {
    speaking.classList.add("signal-chip-ready");
  }
  signals.append(speaking);

  const manageable = canManageMember(room, ownMemberId, member);
  const permission = textNode(
    "button",
    "member-toggle",
    manageable ? memberPermissionLabel(member) : "权限",
  );
  permission.type = "button";
  permission.disabled = !manageable;
  if (manageable) {
    permission.addEventListener("click", () => {
      sendRoomControl(memberCanSpeakSignal(member.id, !member.can_speak));
    });
  }
  signals.append(permission);

  row.append(identity, signals);
  return row;
}

function renderEmptyMembers(message) {
  const row = textNode("article", "member-row member-row-ghost", "");
  const identity = textNode("div", "member-identity", "");
  identity.append(textNode("span", "member-avatar member-avatar-empty", "+"));

  const content = document.createElement("div");
  content.append(
    textNode("strong", "", "等待成员状态"),
    textNode("span", "", message),
  );
  identity.append(content);
  row.append(identity);
  memberList.replaceChildren(row);
}

function renderRoom(room) {
  const members = membersForRoom(room);
  membersMeta.textContent = `${members.length} 位成员`;
  memberList.replaceChildren(...members.map((member) => renderMember(member, room)));
  renderVoiceState();
}

function handleRoomSignal(signal) {
  if (signal.type === "ice_candidate") {
    mediaSession?.addRemoteIceCandidate(signal.candidate).catch((error) => {
      showError(error.message || "服务端 ICE candidate 处理失败。");
    });
    return;
  }
  if (signal.type === "renegotiation_needed") {
    mediaSession?.renegotiate().catch((error) => {
      showError(error.message || "媒体重新协商失败。");
    });
    return;
  }

  currentRoom = nextRoomSnapshot(currentRoom, signal);
  if (signal.type === "room_closed") {
    mediaSession?.close();
    clearRoomSession(window.sessionStorage);
    roomSession = null;
    setConnection("房间已关闭");
    membersMeta.textContent = "房间已关闭";
    renderEmptyMembers("房主已离开。");
    showError("房主已离开，房间已关闭。");
    return;
  }

  if (signal.type === "error") {
    showError(signal.message || "房间信令发生错误。");
    return;
  }

  if (currentRoom) {
    renderRoom(currentRoom);
  }
}

function entrySignal(intent) {
  if (intent.mode === "create") {
    return createRoomSignal(intent.nickname);
  }
  if (intent.mode === "resume") {
    return resumeRoomSignal(intent.session);
  }

  return joinRoomSignal(intent);
}

function joinedNickname(joined, intent) {
  return (
    joined.room?.members?.[joined.member_id]?.nickname ||
    intent.nickname ||
    intent.session?.nickname ||
    ""
  );
}

function rememberJoinedRoom(joined, intent) {
  roomSession = saveRoomSession(window.sessionStorage, {
    roomId: joined.room.id,
    memberId: joined.member_id,
    resumeToken: joined.resume_token,
    nickname: joinedNickname(joined, intent),
  });
}

function scheduleReconnect() {
  if (reconnectTimer || intentionalShutdown || pageHidden || !roomSession) {
    return;
  }

  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null;
    connectRoom({ mode: "resume", session: roomSession });
  }, 1000);
}

async function connectRoom(intent) {
  setConnection("连接中");
  const nextClient = new SignalingClient(websocketUrl(window.location));
  client = nextClient;
  nextClient.onSignal(handleRoomSignal);
  nextClient.onProtocolError(() => showError("收到无法解析的房间信令。"));
  nextClient.onError(() => showError("房间信令连接失败。"));
  nextClient.onClose(() => {
    if (client !== nextClient || intentionalShutdown || pageHidden) {
      return;
    }
    if (connection.textContent === "房间已关闭") {
      return;
    }

    mediaSession?.close();
    mediaSession = null;
    mediaReady = false;
    renderVoiceState({ media: "waiting", downlink: "waiting" });
    setConnection(roomSession ? "重连中" : "已断开");
    scheduleReconnect();
  });

  try {
    await nextClient.connect();
    const joined = await nextClient.request(entrySignal(intent));
    rememberJoinedRoom(joined, intent);
    currentRoom = joined.room;
    ownMemberId = joined.member_id;
    clearRoomEntryIntent(window.sessionStorage);
    roomIdNode.textContent = joined.room.id;
    renderRoom(joined.room);
    setConnection("已连接");
    void startMedia();

    if (intent.mode === "create") {
      window.history.replaceState(null, "", roomPath(joined.room.id));
    }
  } catch (joinError) {
    if (client === nextClient) {
      nextClient.close();
    }
    if (intent.mode === "resume" && joinError.signal?.code === "invalid_message") {
      setConnection("重连中");
      scheduleReconnect();
      return;
    }
    if (intent.mode === "resume") {
      clearRoomSession(window.sessionStorage);
      roomSession = null;
    }
    setConnection("未加入");
    renderEmptyMembers("返回大厅重新进入。");
    showError(joinError.message || "无法进入房间。");
  }
}

async function startMedia() {
  mediaSession?.close();
  mediaReady = false;
  mediaSession = new MediaSession(client, {
    audioHost: remoteAudio,
    onState: renderVoiceState,
    onError(error) {
      showError(error.message || "媒体连接发生错误。");
    },
  });

  try {
    await mediaSession.start();
    mediaSession.setMuted(Boolean(ownMember()?.self_muted));
    mediaReady = true;
    renderVoiceState();
  } catch (_error) {
    mediaReady = false;
    renderVoiceState({ device: "denied", media: "failed" });
  }
}

muteSelf.addEventListener("click", () => {
  const nextMuted = !ownMember()?.self_muted;
  mediaSession?.setMuted(nextMuted);
  sendRoomControl(selfMutedSignal(nextMuted));
});

leaveRoom.addEventListener("click", () => {
  intentionalShutdown = true;
  if (reconnectTimer) {
    window.clearTimeout(reconnectTimer);
  }
  try {
    client?.send({ type: "leave_room" });
  } catch (_error) {
    // The server will handle a closed socket as a recoverable disconnect.
  }
  clearRoomSession(window.sessionStorage);
  roomSession = null;
  mediaSession?.close();
  client?.close();
  window.location.assign("/");
});

if (!routeRoomId) {
  setConnection("地址无效");
  membersMeta.textContent = "缺少房间号";
  renderEmptyMembers("返回大厅重新进入。");
  showError("房间地址缺少房间号。");
} else {
  roomIdNode.textContent = routeRoomId === "NEW" ? "创建中" : routeRoomId;
  const intent = loadRoomEntryIntent(window.sessionStorage, routeRoomId);
  const session = intent ? null : loadRoomSession(window.sessionStorage, routeRoomId);
  if (intent) {
    connectRoom(intent);
  } else if (session) {
    roomSession = session;
    connectRoom({ mode: "resume", session });
  } else {
    const directEntry = directRoomEntry(window.localStorage, routeRoomId);
    if (directEntry?.mode === "join") {
      connectRoom(directEntry);
    } else if (directEntry?.lobbyPath) {
      window.location.replace(directEntry.lobbyPath);
    } else {
      setConnection("未加入");
      membersMeta.textContent = "缺少进入信息";
      renderEmptyMembers("从大厅创建或加入房间后再进入。");
      showError("当前标签页没有这个房间的进入信息。");
    }
  }
}

window.addEventListener("pagehide", () => {
  pageHidden = true;
  if (reconnectTimer) {
    window.clearTimeout(reconnectTimer);
  }
  mediaSession?.close();
  client?.close();
});
