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
  startScreenShareSignal,
  stopScreenShareSignal,
  websocketUrl,
} from "/assets/room-state.mjs";
import {
  canManageMember,
  canToggleMemberListening,
  memberCanSpeakSignal,
  memberLatencySignal,
  memberLatencyView,
  memberListeningLabel,
  memberListeningSignal,
  memberPermissionLabel,
  memberSpeakingSignal,
  selfMutedSignal,
} from "/assets/room-controls.mjs";
import {
  chatMessageContentParts,
  chatMessageView,
  chatUnreadBadgeText,
  insertMentionText,
  mentionCandidates,
  mentionsForSend,
  messageMentionsCurrentMember,
  nextChatUnreadCount,
  sendChatMessageSignal,
} from "/assets/chat-controls.mjs";
import {
  clampMicrophoneGain,
  clampPlaybackVolume,
  loadMemberVolume,
  loadMicrophoneGain,
  saveMemberVolume,
  saveMicrophoneGain,
  volumePercent,
} from "/assets/audio-volume.mjs";
import { MediaSession } from "/assets/media-session.mjs";
import { RoomConnection } from "/assets/room-connection.mjs";

const roomIdNode = document.querySelector("#room-id");
const roomError = document.querySelector("#room-error");
const connection = document.querySelector("#room-connection");
const sidePanel = document.querySelector("#side-panel");
const membersTitle = document.querySelector("#members-title");
const membersMeta = document.querySelector("#members-meta");
const memberList = document.querySelector("#member-list");
const panelTabs = Array.from(document.querySelectorAll("[data-panel]"));
const chatUnread = document.querySelector("#chat-unread");
const chatPanel = document.querySelector("#chat-panel");
const chatMessagesNode = document.querySelector("#chat-messages");
const mentionReminder = document.querySelector("#mention-reminder");
const mentionReminderTitle = document.querySelector("#mention-reminder-title");
const mentionReminderText = document.querySelector("#mention-reminder-text");
const chatForm = document.querySelector("#chat-form");
const mentionPicker = document.querySelector("#mention-picker");
const chatInput = document.querySelector("#chat-input");
const screenPanel = document.querySelector("#screen-panel");
const screenShareTitle = document.querySelector("#screen-share-title");
const screenShareMeta = document.querySelector("#screen-share-meta");
const startScreenShare = document.querySelector("#start-screen-share");
const stopScreenShare = document.querySelector("#stop-screen-share");
const openScreenPopout = document.querySelector("#open-screen-popout");
const fullscreenScreenShare = document.querySelector("#fullscreen-screen-share");
const screenVideoFrame = document.querySelector("#screen-video-frame");
const screenVideo = document.querySelector("#screen-video");
const screenVideoPlaceholder = document.querySelector("#screen-video-placeholder");
const screenPopout = document.querySelector("#screen-popout");
const screenPopoutTitle = document.querySelector("#screen-popout-title");
const screenPopoutFrame = document.querySelector("#screen-popout-frame");
const screenPopoutVideo = document.querySelector("#screen-popout-video");
const closeScreenPopout = document.querySelector("#close-screen-popout");
const popoutFullscreenScreenShare = document.querySelector("#popout-fullscreen-screen-share");
const micState = document.querySelector("#mic-state");
const deviceState = document.querySelector("#device-state");
const mediaState = document.querySelector("#media-state");
const downlinkState = document.querySelector("#downlink-state");
const permissionNote = document.querySelector("#permission-note");
const microphoneGain = document.querySelector("#microphone-gain");
const microphoneGainValue = document.querySelector("#microphone-gain-value");
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
let notListeningMemberIds = new Set();
let activeSidePanel = "members";
let chatMessages = [];
let unreadChatCount = 0;
let selectedMentions = [];
let mentionPickerMembers = [];
let mentionPickerIndex = 0;
let mentionReminderTimer = null;
let latencySnapshot = { serverMs: null, members: {} };
let memberVolumes = new Map();
let microphoneGainLevel = loadMicrophoneGain(window.localStorage);
let localScreenStream = null;
let remoteScreenStream = null;
let speakingMemberIds = new Set();
let speakingTimers = new Map();
const SPEAKING_TTL_MS = 1800;
const MENTION_REMINDER_MS = 10000;
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

function setActiveSidePanel(panel) {
  activeSidePanel = panel;
  const chatActive = panel === "chat";
  const screenActive = panel === "screen";
  memberList.hidden = chatActive || screenActive;
  chatPanel.hidden = !chatActive;
  screenPanel.hidden = !screenActive;
  sidePanel.dataset.activePanel = panel;
  membersTitle.textContent = screenActive ? "共享" : chatActive ? "聊天" : "成员";
  for (const tab of panelTabs) {
    const active = tab.dataset.panel === panel;
    tab.classList.toggle("panel-tab-active", active);
    tab.setAttribute("aria-selected", String(active));
  }
  if (chatActive) {
    unreadChatCount = 0;
    renderUnreadBadge();
    clearMentionReminder();
    requestAnimationFrame(() => {
      chatMessagesNode.scrollTop = chatMessagesNode.scrollHeight;
      chatInput.focus({ preventScroll: true });
    });
  }
  if (screenActive) {
    renderScreenSharePanel();
  }
}

function renderUnreadBadge() {
  const label = chatUnreadBadgeText(unreadChatCount);
  chatUnread.hidden = !label;
  chatUnread.textContent = label;
}

function currentScreenShare() {
  return currentRoom?.screen_share ?? null;
}

function canStopScreenShare() {
  const share = currentScreenShare();
  const self = ownMember();
  return Boolean(share && (share.member_id === ownMemberId || self?.role === "owner"));
}

function activeScreenStream() {
  const share = currentScreenShare();
  if (!share) {
    return null;
  }

  return share.member_id === ownMemberId ? localScreenStream : remoteScreenStream;
}

function renderScreenVideoState() {
  const stream = activeScreenStream();
  if (screenVideo.srcObject !== stream) {
    screenVideo.srcObject = stream;
  }
  if (screenPopoutVideo.srcObject !== stream) {
    screenPopoutVideo.srcObject = stream;
  }

  screenVideo.classList.toggle("screen-video-active", Boolean(stream));
  screenVideoPlaceholder.classList.toggle("screen-video-placeholder-hidden", Boolean(stream));
}

function attachLocalScreenStream(stream) {
  localScreenStream = stream;
  renderScreenVideoState();
}

function attachRemoteScreenStream(stream) {
  remoteScreenStream = stream;
  renderScreenVideoState();
}

function renderScreenSharePanel() {
  const share = currentScreenShare();
  const sharing = Boolean(share);
  const selfSharing = share?.member_id === ownMemberId;
  const canShare = mediaSession?.canShareScreen?.() ?? Boolean(navigator.mediaDevices?.getDisplayMedia);
  const stream = activeScreenStream();

  screenShareTitle.textContent = sharing
    ? `${share.nickname || "成员"} 正在共享屏幕`
    : "当前没有屏幕共享";
  if (screenShareMeta) {
    screenShareMeta.textContent = sharing
      ? "语音沟通继续使用麦克风。"
      : "切到共享后不会影响语音连接。";
  }
  startScreenShare.hidden = sharing;
  startScreenShare.disabled = !mediaReady || !canShare;
  startScreenShare.title = canShare ? "开始共享屏幕" : "当前浏览器不支持屏幕共享";
  stopScreenShare.hidden = !canStopScreenShare();
  stopScreenShare.textContent = selfSharing ? "停止共享" : "停止对方共享";
  openScreenPopout.disabled = !sharing || !stream;
  fullscreenScreenShare.disabled = !sharing || !stream;
  screenPopoutTitle.textContent = sharing
    ? `${share.nickname || "成员"} 的屏幕共享`
    : "屏幕共享";
  renderScreenVideoState();
  if (!sharing) {
    screenPopout.hidden = true;
  }
}

function openScreenSharePopout() {
  if (!currentScreenShare() || !activeScreenStream()) {
    return;
  }
  screenPopout.hidden = false;
}

async function requestScreenFullscreen(target = screenVideoFrame) {
  try {
    if (!target?.requestFullscreen) {
      throw new Error("当前浏览器不支持全屏。");
    }
    await target.requestFullscreen();
  } catch (error) {
    showError(error.message || "无法进入全屏。");
  }
}

function startScreenShareRequestId() {
  return `screen-${Date.now()}`;
}

function clearMentionReminder() {
  if (mentionReminderTimer) {
    window.clearTimeout(mentionReminderTimer);
    mentionReminderTimer = null;
  }
  mentionReminder.hidden = true;
  mentionReminderTitle.textContent = "";
  mentionReminderText.textContent = "";
}

function showMentionReminder(message) {
  if (
    activeSidePanel === "chat" ||
    message?.member_id === ownMemberId ||
    !messageMentionsCurrentMember(message, ownMemberId)
  ) {
    return;
  }

  mentionReminderTitle.textContent = `${message.nickname || "成员"} @ 了你`;
  mentionReminderText.textContent = message.content || "";
  mentionReminder.hidden = false;
  if (mentionReminderTimer) {
    window.clearTimeout(mentionReminderTimer);
  }
  mentionReminderTimer = window.setTimeout(clearMentionReminder, MENTION_REMINDER_MS);
}

function activeMentionQuery() {
  const cursor = chatInput.selectionStart ?? chatInput.value.length;
  const prefix = chatInput.value.slice(0, cursor);
  const atIndex = prefix.lastIndexOf("@");
  if (atIndex < 0) {
    return null;
  }
  const query = prefix.slice(atIndex + 1);
  if (/\s/.test(query)) {
    return null;
  }

  return query;
}

function hideMentionPicker() {
  mentionPicker.hidden = true;
  mentionPicker.replaceChildren();
  mentionPickerMembers = [];
  mentionPickerIndex = 0;
}

function renderMentionPicker() {
  const query = activeMentionQuery();
  if (query === null || !currentRoom) {
    hideMentionPicker();
    return;
  }

  mentionPickerMembers = mentionCandidates(currentRoom, ownMemberId).filter((member) =>
    (member.nickname ?? "").toLowerCase().includes(query.toLowerCase()),
  );
  mentionPickerIndex = Math.min(mentionPickerIndex, Math.max(mentionPickerMembers.length - 1, 0));
  if (!mentionPickerMembers.length) {
    hideMentionPicker();
    return;
  }

  mentionPicker.replaceChildren(
    ...mentionPickerMembers.map((member, index) => {
      const option = textNode("button", "mention-option", "");
      option.type = "button";
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", String(index === mentionPickerIndex));
      if (index === mentionPickerIndex) {
        option.classList.add("mention-option-active");
      }
      option.append(
        textNode("span", "mention-option-avatar", avatarText(member)),
        textNode("span", "mention-option-name", member.nickname),
      );
      option.addEventListener("mousedown", (event) => {
        event.preventDefault();
      });
      option.addEventListener("click", () => {
        selectMention(member);
      });
      return option;
    }),
  );
  mentionPicker.hidden = false;
}

function selectMention(member) {
  const inserted = insertMentionText(
    chatInput.value,
    chatInput.selectionStart ?? chatInput.value.length,
    chatInput.selectionEnd ?? chatInput.value.length,
    member,
  );
  chatInput.value = inserted.value;
  selectedMentions = [...selectedMentions, inserted.mention];
  hideMentionPicker();
  chatInput.focus();
  chatInput.setSelectionRange(inserted.cursor, inserted.cursor);
}

function ownMember() {
  return currentRoom?.members?.[ownMemberId] ?? null;
}

function memberVolume(memberId) {
  if (!memberId || !currentRoom?.id) {
    return 1;
  }
  if (!memberVolumes.has(memberId)) {
    memberVolumes.set(memberId, loadMemberVolume(window.localStorage, currentRoom.id, memberId));
  }

  return memberVolumes.get(memberId);
}

function setMemberVolume(memberId, value) {
  if (!memberId || !currentRoom?.id) {
    return;
  }

  const volume = clampPlaybackVolume(value);
  memberVolumes.set(memberId, volume);
  saveMemberVolume(window.localStorage, currentRoom.id, memberId, volume);
  mediaSession?.setMemberVolume(memberId, volume);
}

function applyMemberVolumes() {
  for (const member of membersForRoom(currentRoom)) {
    if (member.id !== ownMemberId) {
      mediaSession?.setMemberVolume(member.id, memberVolume(member.id));
    }
  }
}

function renderMicrophoneGainControl() {
  microphoneGain.value = String(microphoneGainLevel);
  microphoneGainValue.textContent = volumePercent(microphoneGainLevel);
  const supported = mediaSession?.microphoneGainSupported ?? true;
  microphoneGain.disabled = !supported;
  microphoneGain.title = supported ? "调整别人听到的麦克风音量" : "当前浏览器不支持输入音量调节";
}

function setMicrophoneGain(value) {
  microphoneGainLevel = clampMicrophoneGain(value);
  saveMicrophoneGain(window.localStorage, microphoneGainLevel);
  mediaSession?.setMicrophoneGain(microphoneGainLevel);
  renderMicrophoneGainControl();
}

function rememberListeningState(memberIds = []) {
  notListeningMemberIds = new Set(memberIds);
}

function rememberLatencySnapshot(snapshot) {
  const nextMembers = { ...latencySnapshot.members };
  for (const [memberId, memberLatency] of Object.entries(snapshot?.members ?? {})) {
    nextMembers[memberId] = {
      ...nextMembers[memberId],
      ...memberLatency,
    };
  }
  latencySnapshot = {
    serverMs: Number.isFinite(snapshot?.serverMs) ? snapshot.serverMs : latencySnapshot.serverMs,
    members: nextMembers,
  };
  if (Number.isFinite(snapshot?.serverMs)) {
    sendRoomControl(memberLatencySignal(snapshot.serverMs));
  }
  if (currentRoom) {
    renderRoom(currentRoom);
  }
}

function rememberMemberLatency(memberId, serverMs) {
  if (!memberId || !Number.isFinite(serverMs)) {
    return;
  }
  if (memberId === ownMemberId) {
    latencySnapshot = {
      ...latencySnapshot,
      serverMs,
    };
  } else {
    latencySnapshot = {
      ...latencySnapshot,
      members: {
        ...latencySnapshot.members,
        [memberId]: {
          ...latencySnapshot.members?.[memberId],
          serverMs,
        },
      },
    };
  }
  if (currentRoom) {
    renderRoom(currentRoom);
  }
}

function clearSpeakingTimers() {
  for (const timer of speakingTimers.values()) {
    window.clearTimeout(timer);
  }
  speakingTimers = new Map();
}

function rememberMemberSpeaking(memberId, speaking) {
  if (!memberId) {
    return;
  }

  const existingTimer = speakingTimers.get(memberId);
  if (existingTimer) {
    window.clearTimeout(existingTimer);
    speakingTimers.delete(memberId);
  }

  if (speaking) {
    speakingMemberIds.add(memberId);
    speakingTimers.set(
      memberId,
      window.setTimeout(() => {
        speakingTimers.delete(memberId);
        speakingMemberIds.delete(memberId);
        if (currentRoom) {
          renderRoom(currentRoom);
        }
      }, SPEAKING_TTL_MS),
    );
  } else {
    speakingMemberIds.delete(memberId);
  }

  if (currentRoom) {
    renderRoom(currentRoom);
  }
}

function sendRoomControl(signal) {
  try {
    client?.send(signal);
  } catch (error) {
    showError(error.message || "房间控制发送失败。");
  }
}

function sendMemberSpeaking(speaking) {
  if (!ownMember()?.can_speak || ownMember()?.self_muted) {
    speaking = false;
  }
  sendRoomControl(memberSpeakingSignal(speaking));
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

function renderChatMessage(message) {
  const view = chatMessageView(message, ownMemberId);
  const row = document.createElement("article");
  row.className = view.own ? "chat-message chat-message-own" : "chat-message";

  const avatar = textNode("span", "chat-avatar", view.avatar);
  const bubble = textNode("div", "chat-bubble", "");
  const meta = textNode("div", "chat-message-meta", "");
  meta.append(
    textNode("strong", "", view.nickname),
    textNode("time", "", view.timeLabel),
  );
  const content = document.createElement("p");
  for (const part of chatMessageContentParts(message)) {
    if (part.type === "mention") {
      const mention = textNode("span", "chat-mention", part.text);
      if (part.memberId === ownMemberId) {
        mention.classList.add("chat-mention-self");
      }
      content.append(mention);
    } else {
      content.append(document.createTextNode(part.text));
    }
  }
  bubble.append(meta, content);
  row.append(avatar, bubble);
  return row;
}

function renderChatMessages() {
  if (!chatMessages.length) {
    const empty = textNode("div", "chat-empty", "还没有消息");
    chatMessagesNode.replaceChildren(empty);
    return;
  }

  chatMessagesNode.replaceChildren(...chatMessages.map(renderChatMessage));
  chatMessagesNode.scrollTop = chatMessagesNode.scrollHeight;
}

function rememberChatMessages(messages = []) {
  chatMessages = messages;
  selectedMentions = [];
  hideMentionPicker();
  renderChatMessages();
}

function handleChatMessage(message) {
  if (!message) {
    return;
  }
  chatMessages = [...chatMessages, message];
  unreadChatCount = nextChatUnreadCount(
    unreadChatCount,
    activeSidePanel,
    message,
    ownMemberId,
  );
  showMentionReminder(message);
  renderUnreadBadge();
  renderChatMessages();
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
  const nameLine = textNode("div", "member-name-line", "");
  const speakingIndicator = textNode("span", "member-speaking-indicator", "");
  speakingIndicator.title = "发言中";
  speakingIndicator.setAttribute("aria-label", "发言中");
  if (speakingMemberIds.has(member.id) && member.can_speak && !member.self_muted) {
    speakingIndicator.classList.add("member-speaking-indicator-active");
  }
  nameLine.append(textNode("strong", "", member.nickname), speakingIndicator);
  name.append(
    nameLine,
    textNode("span", "member-state", memberStateLabel(member)),
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

  if (canToggleMemberListening(ownMemberId, member)) {
    const notListening = notListeningMemberIds.has(member.id);
    const listening = textNode(
      "button",
      "member-toggle member-listening-toggle",
      memberListeningLabel(notListening),
    );
    listening.type = "button";
    listening.addEventListener("click", () => {
      sendRoomControl(memberListeningSignal(member.id, notListening));
    });
    signals.append(listening);
  }

  if (member.id !== ownMemberId) {
    const volume = memberVolume(member.id);
    const volumeControl = textNode("label", "volume-control member-volume-control", "");
    const volumeInput = document.createElement("input");
    volumeInput.type = "range";
    volumeInput.min = "0";
    volumeInput.max = "1";
    volumeInput.step = "0.05";
    volumeInput.value = String(volume);
    volumeInput.setAttribute("aria-label", `${member.nickname} 播放音量`);
    const volumeValue = textNode("strong", "volume-value", volumePercent(volume));
    volumeInput.addEventListener("input", () => {
      setMemberVolume(member.id, volumeInput.value);
      volumeValue.textContent = volumePercent(volumeInput.value);
    });
    volumeControl.append(textNode("span", "", "音量"), volumeInput, volumeValue);
    signals.append(volumeControl);
  }

  const latencyView = memberLatencyView(member.id, ownMemberId, latencySnapshot);
  const latency = textNode("span", latencyView.className, latencyView.label);
  latency.title = latencyView.title;
  latency.setAttribute("aria-label", latencyView.title);
  signals.append(latency);

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
  for (const member of members) {
    if (member.id !== ownMemberId) {
      memberVolume(member.id);
      mediaSession?.setMemberVolume(member.id, memberVolume(member.id));
    }
  }
  membersMeta.textContent = `${members.length} 位成员`;
  memberList.replaceChildren(...members.map((member) => renderMember(member, room)));
  renderVoiceState();
  renderScreenSharePanel();
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
    rememberListeningState();
    speakingMemberIds = new Set();
    clearSpeakingTimers();
    setConnection("房间已关闭");
    membersMeta.textContent = "房间已关闭";
    renderEmptyMembers("房主已离开。");
    rememberChatMessages();
    clearMentionReminder();
    showError("房主已离开，房间已关闭。");
    return;
  }
  if (signal.type === "member_listening_updated") {
    rememberListeningState(signal.not_listening_member_ids);
    if (currentRoom) {
      renderRoom(currentRoom);
    }
    return;
  }
  if (signal.type === "member_speaking_updated") {
    rememberMemberSpeaking(signal.member_id, signal.speaking);
    return;
  }
  if (signal.type === "member_latency_updated") {
    rememberMemberLatency(signal.member_id, signal.server_ms);
    return;
  }
  if (signal.type === "screen_share_started") {
    renderScreenSharePanel();
    if (signal.member_id === ownMemberId) {
      mediaSession
        ?.startScreenShare()
        .then((stream) => {
          attachLocalScreenStream(stream);
          renderScreenSharePanel();
        })
        .catch((error) => {
          showError(error.message || "屏幕共享启动失败。");
          sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
        });
    }
    return;
  }
  if (signal.type === "screen_share_stopped") {
    if (signal.member_id === ownMemberId) {
      attachLocalScreenStream(null);
      mediaSession?.stopScreenShare({ notify: false }).catch((error) => {
        showError(error.message || "停止屏幕共享失败。");
      });
    }
    renderScreenSharePanel();
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
  const nextClient = new RoomConnection(websocketUrl(window.location));
  client = nextClient;
  nextClient.onSignal(handleRoomSignal);
  nextClient.onChatMessage(handleChatMessage);
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
    const joined = await nextClient.enter(entrySignal(intent));
    rememberJoinedRoom(joined, intent);
    rememberListeningState(joined.not_listening_member_ids);
    memberVolumes = new Map();
    currentRoom = joined.room;
    ownMemberId = joined.member_id;
    rememberChatMessages(joined.chat_messages);
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
      rememberListeningState();
      speakingMemberIds = new Set();
      clearSpeakingTimers();
      rememberChatMessages();
    }
    setConnection("未加入");
    renderEmptyMembers("返回大厅重新进入。");
    showError(joinError.message || "无法进入房间。");
  }
}

async function startMedia() {
  mediaSession?.close();
  mediaReady = false;
  localScreenStream = null;
  remoteScreenStream = null;
  mediaSession = new MediaSession(client, {
    audioHost: remoteAudio,
    onState: renderVoiceState,
    onLatency: rememberLatencySnapshot,
    onSpeaking: sendMemberSpeaking,
    onScreenStream(stream) {
      attachRemoteScreenStream(stream);
      renderScreenSharePanel();
    },
    onScreenShareEnded() {
      sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
    },
    onError(error) {
      showError(error.message || "媒体连接发生错误。");
    },
  });
  mediaSession.setMicrophoneGain(microphoneGainLevel);
  applyMemberVolumes();
  renderMicrophoneGainControl();
  renderScreenSharePanel();

  try {
    await mediaSession.start();
    mediaSession.setMuted(Boolean(ownMember()?.self_muted));
    mediaSession.setMicrophoneGain(microphoneGainLevel);
    applyMemberVolumes();
    mediaReady = true;
    renderVoiceState();
    renderMicrophoneGainControl();
    renderScreenSharePanel();
  } catch (_error) {
    mediaReady = false;
    renderVoiceState({ media: "failed" });
    renderMicrophoneGainControl();
    renderScreenSharePanel();
  }
}

muteSelf.addEventListener("click", () => {
  const nextMuted = !ownMember()?.self_muted;
  mediaSession?.setMuted(nextMuted);
  sendRoomControl(selfMutedSignal(nextMuted));
});

microphoneGain.addEventListener("input", () => {
  setMicrophoneGain(microphoneGain.value);
});

for (const tab of panelTabs) {
  tab.addEventListener("click", () => {
    setActiveSidePanel(tab.dataset.panel || "members");
  });
}

startScreenShare.addEventListener("click", () => {
  sendRoomControl(startScreenShareSignal(startScreenShareRequestId()));
  setActiveSidePanel("screen");
});

stopScreenShare.addEventListener("click", () => {
  sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
});

openScreenPopout.addEventListener("click", openScreenSharePopout);
closeScreenPopout.addEventListener("click", () => {
  screenPopout.hidden = true;
});
fullscreenScreenShare.addEventListener("click", () => {
  void requestScreenFullscreen(screenVideoFrame);
});
popoutFullscreenScreenShare.addEventListener("click", () => {
  void requestScreenFullscreen(screenPopoutFrame);
});

chatForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  let signal;
  const mentions = mentionsForSend(chatInput.value, selectedMentions);
  try {
    signal = sendChatMessageSignal(chatInput.value, undefined, mentions);
  } catch (error) {
    showError(error.message || "聊天消息无效。");
    return;
  }

  chatInput.value = "";
  selectedMentions = [];
  hideMentionPicker();
  chatInput.focus();
  try {
    if (!client) {
      throw new Error("房间信令尚未连接。");
    }
    await client.sendChatMessage(signal.content, signal.request_id, signal.mentions ?? []);
  } catch (error) {
    chatInput.value = signal.content;
    selectedMentions = signal.mentions ?? [];
    showError(error.message || "聊天消息发送失败。");
  }
});

chatInput.addEventListener("keydown", (event) => {
  if (!mentionPicker.hidden && ["ArrowDown", "ArrowUp", "Enter", "Escape"].includes(event.key)) {
    if (event.key === "Escape") {
      event.preventDefault();
      hideMentionPicker();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      mentionPickerIndex =
        (mentionPickerIndex + delta + mentionPickerMembers.length) % mentionPickerMembers.length;
      renderMentionPicker();
      return;
    }
    if (event.key === "Enter" && mentionPickerMembers[mentionPickerIndex]) {
      event.preventDefault();
      selectMention(mentionPickerMembers[mentionPickerIndex]);
      return;
    }
  }

  if (event.key !== "Enter" || event.shiftKey || event.isComposing) {
    return;
  }

  event.preventDefault();
  chatForm.requestSubmit();
});

chatInput.addEventListener("input", () => {
  renderMentionPicker();
});

chatInput.addEventListener("blur", () => {
  window.setTimeout(hideMentionPicker, 120);
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
  rememberListeningState();
  speakingMemberIds = new Set();
  clearSpeakingTimers();
  rememberChatMessages();
  mediaSession?.close();
  client?.close();
  window.location.assign("/");
});

if (!routeRoomId) {
  setConnection("地址无效");
  membersMeta.textContent = "缺少房间号";
  renderEmptyMembers("返回大厅重新进入。");
  rememberChatMessages();
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
      rememberChatMessages();
      showError("当前标签页没有这个房间的进入信息。");
    }
  }
}

window.addEventListener("pagehide", () => {
  pageHidden = true;
  if (reconnectTimer) {
    window.clearTimeout(reconnectTimer);
  }
});
