import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  canToggleMemberListening,
  memberCanSpeakSignal,
  memberListeningSignal,
} from "../lib/room-controls.js";
import {
  clearRoomEntryIntent,
  clearRoomSession,
  directRoomEntry,
  loadRoomEntryIntent,
  loadRoomPanel,
  loadRoomSession,
  saveRoomPanel,
  saveRoomSession,
} from "../lib/room-entry.js";
import {
  membersForRoom,
  nextRoomSnapshot,
  stopScreenShareSignal,
} from "../lib/room-state.js";
import { useRoomChatSession } from "./useRoomChatSession.js";
import { useRoomConnectionSession } from "./useRoomConnectionSession.js";
import { useRoomMediaSession } from "./useRoomMediaSession.js";
import { useRoomMemberPreferences } from "./useRoomMemberPreferences.js";
import { useRoomP2PSession } from "./useRoomP2PSession.js";
import { useRoomScreenShareSession } from "./useRoomScreenShareSession.js";

const SPEAKING_TTL_MS = 1800;

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

export function useRoomSession() {
  const roomSegments = window.location.pathname.split("/").filter(Boolean);
  const routeRoomId = roomSegments[0] === "rooms" ? decodeRoomId(roomSegments[1]) : "";

  // Shared core state owned by the composition layer.
  const currentRoom = ref(null);
  const ownMemberId = ref("");
  const roomIdLabel = ref("--");
  const connectionLabel = ref("未连接");
  const errorMessage = ref("");
  const activeSidePanel = ref("members");
  const speakingMemberIds = ref(new Set());
  const pageHidden = ref(false);

  // Transport handles exposed to the boundary composables as refs.
  const clientRef = ref(null);
  const mediaSessionRef = ref(null);
  const p2pSessionRef = ref(null);

  // Session bookkeeping that stays with the orchestrator.
  let roomSession = null;
  let intentionalShutdown = false;
  let reconnectTimer = null;
  let clientConfigPromise = null;
  let speakingTimers = new Map();

  const members = computed(() => membersForRoom(currentRoom.value));
  const membersMeta = computed(() => {
    if (!currentRoom.value) {
      return routeRoomId ? "等待房间状态" : "缺少房间号";
    }
    return `${members.value.length} 位成员`;
  });
  const ownMember = computed(() => currentRoom.value?.members?.[ownMemberId.value] ?? null);
  const panelTitle = computed(() => {
    if (activeSidePanel.value === "screen") {
      return "共享";
    }
    if (activeSidePanel.value === "chat") {
      return "聊天";
    }
    return "成员";
  });

  function showError(message) {
    errorMessage.value = message;
  }

  // --- Boundary: member preferences (volumes, gain, listening, cleanup) ---
  const preferences = useRoomMemberPreferences({
    currentRoom,
    mediaSessionRef,
    ownMemberId,
    p2pSessionRef,
    routeRoomId,
    sendRoomControl: (signal) => sendRoomControl(signal),
  });

  // --- Boundary: connection (ws lifecycle + control send) ---
  const connection = useRoomConnectionSession({
    clientRef,
    connectionLabel,
    onChatMessage: (message) => chat.handleChatMessage(message),
    onClose: (nextClient) => handleConnectionClose(nextClient),
    onError: (message) => showError(message),
    onProtocolError: () => showError("收到无法解析的房间信令。"),
    onSignal: (signal) => handleRoomSignal(signal),
  });

  function sendRoomControl(signal) {
    connection.sendRoomControl(signal);
  }

  // Shared between the media and screen-share boundaries: media resets it on
  // (re)start, screen share populates it when sharing begins. Owned here so
  // neither boundary has to construct the other first.
  const localScreenStream = ref(null);

  // --- Boundary: media (WebRTC, latency, mute, voice state) ---
  const media = useRoomMediaSession({
    applyMemberVolumes: preferences.applyMemberVolumes,
    clientRef,
    currentRoom,
    localScreenStream,
    mediaSessionRef,
    microphoneGainLevel: preferences.microphoneGainLevel,
    onLocalMediaTrack: (entry) => p2p.handleLocalMediaTrack(entry),
    onError: (message) => showError(message),
    ownMember,
    sendRoomControl,
    startScreenShareRequestId: () => screenShare.startScreenShareRequestId(),
  });

  // --- Boundary: screen share (viewing, titles, controls) ---
  const screenShare = useRoomScreenShareSession({
    activeSidePanel,
    currentRoom,
    localScreenStream,
    mediaReady: media.mediaReady,
    mediaSessionRef,
    onError: (message) => showError(message),
    ownMember,
    ownMemberId,
    remoteScreenStream: media.remoteScreenStream,
    sendRoomControl,
  });

  // --- Boundary: chat (messages, mentions, unread, toasts) ---
  const chat = useRoomChatSession({
    activeSidePanel,
    clientRef,
    currentRoom,
    onError: (message) => showError(message),
    ownMemberId,
  });

  // --- Boundary: P2P media (browser-to-browser PeerConnections + route fallback) ---
  const p2p = useRoomP2PSession({
    clientRef,
    currentRoom,
    media,
    ownMemberId,
    p2pSessionRef,
    onError: (message) => showError(message),
  });

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

    const nextSpeaking = new Set(speakingMemberIds.value);
    if (speaking) {
      nextSpeaking.add(memberId);
      speakingTimers.set(
        memberId,
        window.setTimeout(() => {
          speakingTimers.delete(memberId);
          const expiredSpeaking = new Set(speakingMemberIds.value);
          expiredSpeaking.delete(memberId);
          speakingMemberIds.value = expiredSpeaking;
        }, SPEAKING_TTL_MS),
      );
    } else {
      nextSpeaking.delete(memberId);
    }
    speakingMemberIds.value = nextSpeaking;
  }

  function setActiveSidePanel(panel) {
    activeSidePanel.value = panel;
    saveRoomPanel(window.sessionStorage, currentRoom.value?.id, panel);
    if (panel === "chat") {
      chat.activateChatPanel();
    }
    screenShare.syncScreenViewingState();
  }

  // 处理 WebSocket 普通断开；SFU 和 P2P 都释放，随后用恢复凭据重连。
  function handleConnectionClose(nextClient) {
    if (clientRef.value !== nextClient || intentionalShutdown || pageHidden.value) {
      return;
    }
    if (connectionLabel.value === "房间已关闭") {
      return;
    }

    p2p.closeP2P();
    media.resetMediaState();
    setConnection(roomSession ? "重连中" : "已断开");
    scheduleReconnect();
  }

  function setConnection(message) {
    connectionLabel.value = message;
  }

  // 按信令类型分发房间事件；P2P 信令先处理，避免进入普通房间快照逻辑。
  function handleRoomSignal(signal) {
    if (p2p.handleP2PSignal(signal)) {
      return;
    }
    if (signal.type === "ice_candidate") {
      mediaSessionRef.value?.addRemoteIceCandidate(signal.candidate).catch((error) => {
        showError(error.message || "服务端 ICE candidate 处理失败。");
      });
      return;
    }
    if (signal.type === "renegotiation_needed") {
      mediaSessionRef.value?.renegotiate().catch((error) => {
        showError(error.message || "媒体重新协商失败。");
      });
      return;
    }

    const previousRoomId = currentRoom.value?.id || signal.room_id || routeRoomId;
    currentRoom.value = nextRoomSnapshot(currentRoom.value, signal);
    if (signal.room || ["member_joined", "member_updated", "member_left"].includes(signal.type)) {
      p2p.syncP2PMembers(currentRoom.value);
    }
    if (signal.type === "room_closed") {
      p2p.closeP2P();
      mediaSessionRef.value?.close();
      preferences.clearRoomScopedSettings(previousRoomId);
      roomSession = null;
      preferences.rememberListeningState([], false);
      speakingMemberIds.value = new Set();
      clearSpeakingTimers();
      setConnection("房间已关闭");
      chat.rememberChatMessages();
      chat.clearMentionReminder();
      showError("房主已离开，房间已关闭。");
      return;
    }
    if (signal.type === "member_listening_updated") {
      preferences.rememberListeningState(signal.not_listening_member_ids);
      return;
    }
    if (signal.type === "member_left") {
      p2p.closeP2PMember(signal.member_id);
      return;
    }
    if (signal.type === "member_speaking_updated") {
      rememberMemberSpeaking(signal.member_id, signal.speaking);
      return;
    }
    if (signal.type === "member_latency_updated") {
      media.rememberMemberLatency(signal.member_id, signal.server_ms, ownMemberId.value);
      return;
    }
    if (signal.type === "screen_share_viewer_count_updated") {
      screenShare.applyScreenShareViewerCount(signal.member_id, signal.viewer_count);
      return;
    }
    if (signal.type === "screen_share_started") {
      screenShare.handleScreenShareStarted(signal);
      return;
    }
    if (signal.type === "screen_share_stopped") {
      screenShare.handleScreenShareStopped(signal);
      return;
    }
    if (signal.type === "video_call_started") {
      media.handleVideoCallStarted(signal);
      return;
    }
    if (signal.type === "video_call_stopped") {
      p2p.clearRemoteCameraStream(signal.member_id);
      media.handleVideoCallStopped(signal);
      return;
    }
    if (signal.type === "video_call_publisher_count_updated") {
      media.applyVideoCallPublisherCount(signal.publisher_count);
      return;
    }
    if (signal.type === "error") {
      showError(signal.message || "房间信令发生错误。");
    }
  }

  function joinedNickname(joined, intent) {
    return joined.room?.members?.[joined.member_id]?.nickname || intent.nickname || intent.session?.nickname || "";
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
    if (reconnectTimer || intentionalShutdown || pageHidden.value || !roomSession) {
      return;
    }

    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = null;
      connectRoom({
        mode: "join",
        roomId: roomSession.roomId,
        nickname: roomSession.nickname,
      });
    }, 1000);
  }

  async function loadClientConfig() {
    if (!clientConfigPromise) {
      clientConfigPromise = fetch("/api/client-config", { cache: "no-store" })
        .then((response) => {
          if (!response.ok) {
            throw new Error(`客户端配置加载失败：${response.status}`);
          }
          return response.json();
        })
        .catch((error) => {
          console.warn(error);
          return null;
        });
    }

    return clientConfigPromise;
  }

  function syncRoomSideEffects(room) {
    const validNotListening = existingPublisherIds(
      room,
      Array.from(preferences.notListeningMemberIds.value),
      ownMemberId.value,
    );
    preferences.rememberListeningState(validNotListening);
    preferences.applyMemberVolumes();
    media.renderVoiceState();
    media.syncVideoCallPublishers(room);
    screenShare.syncScreenViewingState();
  }

  // 建立或恢复房间连接；房间状态就绪后创建 P2P 管理器，再启动本地媒体。
  async function connectRoom(intent) {
    try {
      const nextClient = await connection.openRoomConnection(intent);
      rememberJoinedRoom(nextClient, intent);
      preferences.resetMemberVolumes();
      currentRoom.value = nextClient.room;
      ownMemberId.value = nextClient.member_id;
      preferences.applyStoredListeningState(
        nextClient.room,
        nextClient.not_listening_member_ids ?? [],
        memberListeningSignal,
      );
      chat.rememberChatMessages(nextClient.chat_messages);
      clearRoomEntryIntent(window.sessionStorage);
      roomIdLabel.value = nextClient.room.id;
      setActiveSidePanel(loadRoomPanel(window.sessionStorage, nextClient.room.id));
      p2p.startP2PSession();
      syncRoomSideEffects(nextClient.room);
      setConnection("已连接");
      void startMedia();

      if (intent.mode === "create") {
        window.history.replaceState(null, "", roomPath(nextClient.room.id));
      }
    } catch (joinError) {
      const nextClient = clientRef.value;
      if (nextClient) {
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
        preferences.rememberListeningState([], false);
        speakingMemberIds.value = new Set();
        clearSpeakingTimers();
        chat.rememberChatMessages();
      }
      setConnection("未加入");
      showError(joinError.message || "无法进入房间。");
    }
  }

  // 启动浏览器媒体采集；成功后本地轨道会通过回调同步给 P2P。
  async function startMedia() {
    await media.startMedia(loadClientConfig, (force) => screenShare.syncScreenViewingState(force));
  }

  function toggleMemberPermission(member) {
    sendRoomControl(memberCanSpeakSignal(member.id, !member.can_speak));
  }

  function toggleMemberListening(member) {
    if (!canToggleMemberListening(ownMemberId.value, member)) {
      return;
    }
    const notListening = preferences.notListeningMemberIds.value.has(member.id);
    sendRoomControl(memberListeningSignal(member.id, notListening));
  }

  // 主动离开房间并释放 SFU/P2P 资源，避免浏览器后台继续占用媒体设备。
  function leaveRoom() {
    intentionalShutdown = true;
    const leavingRoomId = currentRoom.value?.id || routeRoomId;
    const leavingEndsRoom = currentRoom.value?.owner_member_id === ownMemberId.value;
    if (reconnectTimer) {
      window.clearTimeout(reconnectTimer);
    }
    try {
      clientRef.value?.send({ type: "leave_room" });
    } catch (_error) {
      // The server will handle a closed socket as a recoverable disconnect.
    }
    if (leavingEndsRoom) {
      preferences.clearRoomScopedSettings(leavingRoomId);
    } else {
      clearRoomSession(window.sessionStorage);
    }
    roomSession = null;
    preferences.rememberListeningState([], false);
    speakingMemberIds.value = new Set();
    clearSpeakingTimers();
    chat.rememberChatMessages();
    p2p.closeP2P();
    mediaSessionRef.value?.close();
    clientRef.value?.close();
    window.location.assign("/");
  }

  function handlePageHide() {
    pageHidden.value = true;
    if (reconnectTimer) {
      window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function bootRoom() {
    document.body.dataset.page = "voice-room";
    preferences.setVoicePaneCollapsed(preferences.voicePaneCollapsed.value, false);
    if (!routeRoomId) {
      setConnection("地址无效");
      chat.rememberChatMessages();
      showError("房间地址缺少房间号。");
      return;
    }

    roomIdLabel.value = routeRoomId === "NEW" ? "创建中" : routeRoomId;
    const intent = loadRoomEntryIntent(window.sessionStorage, routeRoomId);
    const session = intent ? null : loadRoomSession(window.sessionStorage, routeRoomId);
    if (intent) {
      void connectRoom(intent);
    } else if (session) {
      roomSession = session;
      void connectRoom({
        mode: "join",
        roomId: session.roomId,
        nickname: session.nickname,
      });
    } else {
      const directEntry = directRoomEntry(window.localStorage, routeRoomId);
      if (directEntry?.mode === "join") {
        void connectRoom(directEntry);
      } else if (directEntry?.lobbyPath) {
        window.location.replace(directEntry.lobbyPath);
      } else {
        setConnection("未加入");
        chat.rememberChatMessages();
        showError("当前标签页没有这个房间的进入信息。");
      }
    }
  }

  onMounted(() => {
    bootRoom();
    window.addEventListener("pagehide", handlePageHide);
  });

  onBeforeUnmount(() => {
    pageHidden.value = true;
    window.removeEventListener("pagehide", handlePageHide);
    if (reconnectTimer) {
      window.clearTimeout(reconnectTimer);
    }
    chat.disposeChatSession();
    clearSpeakingTimers();
    p2p.closeP2P();
    mediaSessionRef.value?.close();
    clientRef.value?.close();
  });

  return {
    activeScreenStream: screenShare.activeScreenStream,
    activeSidePanel,
    cameraBusy: media.cameraBusy,
    cameraStateLabel: media.cameraStateLabel,
    cameraToggleLabel: media.cameraToggleLabel,
    canUseCamera: media.canUseCamera,
    canShareScreen: screenShare.canShareScreen,
    canStopScreenShare: screenShare.canStopScreenShare,
    chatInput: chat.chatInput,
    chatMessages: chat.chatMessages,
    chatToast: chat.chatToast,
    clearChatToast: chat.clearChatToast,
    clearMentionReminder: chat.clearMentionReminder,
    connectionLabel,
    currentRoom,
    currentScreenShare: screenShare.currentScreenShare,
    deviceStateLabel: media.deviceStateLabel,
    downlinkStateLabel: media.downlinkStateLabel,
    errorMessage,
    hideMentionPicker: chat.hideMentionPicker,
    latencySnapshot: media.latencySnapshot,
    leaveRoom,
    localCameraStream: media.localCameraStream,
    localScreenStream,
    mediaReady: media.mediaReady,
    mediaStateLabel: media.mediaStateLabel,
    memberVolume: preferences.memberVolume,
    members,
    membersMeta,
    mentionPickerIndex: chat.mentionPickerIndex,
    mentionPickerMembers: chat.mentionPickerMembers,
    mentionReminder: chat.mentionReminder,
    micStateLabel: media.micStateLabel,
    microphoneGainLevel: preferences.microphoneGainLevel,
    microphoneGainPercent: computed(() => preferences.volumePercent(preferences.microphoneGainLevel.value)),
    microphoneGainSupported: media.microphoneGainSupported,
    muteSelfLabel: media.muteSelfLabel,
    notListeningMemberIds: preferences.notListeningMemberIds,
    ownMember,
    ownMemberId,
    panelTitle,
    permissionNote: media.permissionNote,
    remoteCameraStreams: media.remoteCameraStreams,
    renderMentionPicker: chat.renderMentionPicker,
    roomIdLabel,
    screenPopoutTitle: screenShare.screenPopoutTitle,
    screenShareTitle: screenShare.screenShareTitle,
    selectMention: chat.selectMention,
    setActiveSidePanel,
    setMemberVolume: preferences.setMemberVolume,
    setMentionPickerIndex: chat.setMentionPickerIndex,
    setMicrophoneGain: preferences.setMicrophoneGain,
    setVoicePaneCollapsed: preferences.setVoicePaneCollapsed,
    showMentionReminder: chat.showMentionReminder,
    speakingMemberIds,
    startScreenShare: screenShare.startScreenShare,
    stopScreenShare: screenShare.stopScreenShare,
    submitChatMessage: chat.submitChatMessage,
    toggleMemberListening,
    toggleMemberPermission,
    toggleCamera: media.toggleCamera,
    toggleSelfMuted: media.toggleSelfMuted,
    unreadBadgeLabel: chat.unreadBadgeLabel,
    voicePaneCollapsed: preferences.voicePaneCollapsed,
    voiceState: media.voiceState,
    volumePercent: preferences.volumePercent,
  };
}

function existingPublisherIds(room, memberIds, ownMemberId) {
  return Array.from(new Set(memberIds)).filter(
    (memberId) => memberId && memberId !== ownMemberId && room?.members?.[memberId],
  );
}
