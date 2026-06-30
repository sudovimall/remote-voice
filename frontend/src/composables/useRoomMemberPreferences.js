import { ref } from "vue";
import {
  clearMemberVolumesForRoom,
  clampMicrophoneGain,
  clampPlaybackVolume,
  loadMemberVolume,
  loadMicrophoneGain,
  saveMemberVolume,
  saveMicrophoneGain,
  volumePercent,
} from "../lib/audio-volume.js";
import {
  clearRoomNotListening,
  clearRoomPanel,
  clearRoomSession,
  loadRoomNotListening,
  saveRoomNotListening,
} from "../lib/room-entry.js";
import { membersForRoom } from "../lib/room-state.js";

export const VOICE_PANE_COLLAPSED_KEY = "remote-voice.voice-pane-collapsed";

function existingPublisherIds(room, memberIds, ownMemberId) {
  return Array.from(new Set(memberIds)).filter(
    (memberId) => memberId && memberId !== ownMemberId && room?.members?.[memberId],
  );
}

export function useRoomMemberPreferences({
  currentRoom,
  mediaSessionRef,
  ownMemberId,
  p2pSessionRef,
  routeRoomId,
  sendRoomControl,
}) {
  const notListeningMemberIds = ref(new Set());
  const microphoneGainLevel = ref(loadMicrophoneGain(window.localStorage));
  const memberVolumes = ref(new Map());
  const voicePaneCollapsed = ref(window.localStorage.getItem(VOICE_PANE_COLLAPSED_KEY) === "1");

  function setVoicePaneCollapsed(collapsed, persist = true) {
    voicePaneCollapsed.value = collapsed;
    if (persist) {
      window.localStorage.setItem(VOICE_PANE_COLLAPSED_KEY, collapsed ? "1" : "0");
    }
  }

  function memberVolume(memberId) {
    if (!memberId || !currentRoom.value?.id) {
      return 1;
    }
    if (!memberVolumes.value.has(memberId)) {
      const nextVolumes = new Map(memberVolumes.value);
      nextVolumes.set(memberId, loadMemberVolume(window.localStorage, currentRoom.value.id, memberId));
      memberVolumes.value = nextVolumes;
    }

    return memberVolumes.value.get(memberId);
  }

  // 保存并应用单个成员音量，SFU 和 P2P 播放节点都要同步更新。
  function setMemberVolume(memberId, value) {
    if (!memberId || !currentRoom.value?.id) {
      return;
    }

    const volume = clampPlaybackVolume(value);
    const nextVolumes = new Map(memberVolumes.value);
    nextVolumes.set(memberId, volume);
    memberVolumes.value = nextVolumes;
    saveMemberVolume(window.localStorage, currentRoom.value.id, memberId, volume);
    mediaSessionRef.value?.setMemberVolume(memberId, volume);
    p2pSessionRef.value?.setMemberVolume(memberId, volume);
  }

  // 将当前房间所有成员音量应用到活跃媒体会话，重连或新建 P2P 后复用。
  function applyMemberVolumes() {
    for (const member of membersForRoom(currentRoom.value)) {
      if (member.id !== ownMemberId.value) {
        mediaSessionRef.value?.setMemberVolume(member.id, memberVolume(member.id));
        p2pSessionRef.value?.setMemberVolume(member.id, memberVolume(member.id));
      }
    }
  }

  // 保存麦克风增益偏好；P2P 复用 MediaSession 输出的增益后音频轨道。
  function setMicrophoneGain(value) {
    microphoneGainLevel.value = clampMicrophoneGain(value);
    saveMicrophoneGain(window.localStorage, microphoneGainLevel.value);
    mediaSessionRef.value?.setMicrophoneGain(microphoneGainLevel.value);
  }

  function rememberListeningState(memberIds = [], persist = true) {
    const values = Array.from(new Set(memberIds.filter(Boolean)));
    notListeningMemberIds.value = new Set(values);
    if (persist && currentRoom.value?.id) {
      saveRoomNotListening(window.localStorage, currentRoom.value.id, values);
    }
  }

  function applyStoredListeningState(room, serverMemberIds = [], memberListeningSignal) {
    const storedMemberIds = existingPublisherIds(
      room,
      loadRoomNotListening(window.localStorage, room?.id),
      ownMemberId.value,
    );
    const mergedMemberIds = Array.from(new Set([...serverMemberIds, ...storedMemberIds]));
    rememberListeningState(mergedMemberIds);

    for (const memberId of storedMemberIds) {
      if (!serverMemberIds.includes(memberId)) {
        sendRoomControl(memberListeningSignal(memberId, false));
      }
    }
  }

  function clearRoomScopedSettings(roomId = currentRoom.value?.id || routeRoomId) {
    if (!roomId) {
      return;
    }

    clearRoomSession(window.sessionStorage);
    clearRoomPanel(window.sessionStorage, roomId);
    clearRoomNotListening(window.localStorage, roomId);
    clearMemberVolumesForRoom(window.localStorage, roomId);
  }

  function resetMemberVolumes() {
    memberVolumes.value = new Map();
  }

  return {
    applyMemberVolumes,
    applyStoredListeningState,
    clearRoomScopedSettings,
    memberVolume,
    memberVolumes,
    microphoneGainLevel,
    notListeningMemberIds,
    rememberListeningState,
    resetMemberVolumes,
    setMemberVolume,
    setMicrophoneGain,
    setVoicePaneCollapsed,
    voicePaneCollapsed,
    volumePercent,
  };
}
