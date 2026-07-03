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

// 过滤本房间仍存在且不是自己的发布者，避免恢复不存在成员的不听偏好。
function existingPublisherIds(room, memberIds, ownMemberId) {
  return Array.from(new Set(memberIds)).filter(
    (memberId) => memberId && memberId !== ownMemberId && room?.members?.[memberId],
  );
}

// 管理成员音量、麦克风增益和“不听”偏好，并把这些偏好同步到 SFU/P2P 播放层。
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

  // 保存成员面板折叠状态，刷新页面后保持用户对语音区域密度的选择。
  function setVoicePaneCollapsed(collapsed, persist = true) {
    voicePaneCollapsed.value = collapsed;
    if (persist) {
      window.localStorage.setItem(VOICE_PANE_COLLAPSED_KEY, collapsed ? "1" : "0");
    }
  }

  // 懒加载成员音量偏好，避免进入房间时一次性读取所有历史成员设置。
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

  // 将“不听”名单同步到 P2P 播放层，保留原始音量偏好以便恢复收听时还原。
  function applyP2PListeningState(memberIds = notListeningMemberIds.value) {
    const blockedMemberIds = memberIds instanceof Set ? memberIds : new Set(memberIds);
    for (const member of membersForRoom(currentRoom.value)) {
      if (member.id !== ownMemberId.value) {
        p2pSessionRef.value?.setMemberListening?.(member.id, !blockedMemberIds.has(member.id));
      }
    }
  }

  // 将当前房间所有成员音量应用到活跃媒体会话，重连或新建 P2P 后复用。
  function applyMemberVolumes() {
    for (const member of membersForRoom(currentRoom.value)) {
      if (member.id !== ownMemberId.value) {
        mediaSessionRef.value?.setMemberVolume(member.id, memberVolume(member.id));
        p2pSessionRef.value?.setMemberVolume(member.id, memberVolume(member.id));
        p2pSessionRef.value?.setMemberListening?.(
          member.id,
          !notListeningMemberIds.value.has(member.id),
        );
      }
    }
  }

  // 保存麦克风增益偏好；P2P 复用 MediaSession 输出的增益后音频轨道。
  function setMicrophoneGain(value) {
    microphoneGainLevel.value = clampMicrophoneGain(value);
    saveMicrophoneGain(window.localStorage, microphoneGainLevel.value);
    mediaSessionRef.value?.setMicrophoneGain(microphoneGainLevel.value);
  }

  // 记录当前用户不收听的成员名单，并立即同步 P2P 音频以避免直连绕过偏好。
  function rememberListeningState(memberIds = [], persist = true) {
    const values = Array.from(new Set(memberIds.filter(Boolean)));
    notListeningMemberIds.value = new Set(values);
    if (persist && currentRoom.value?.id) {
      saveRoomNotListening(window.localStorage, currentRoom.value.id, values);
    }
    applyP2PListeningState(notListeningMemberIds.value);
  }

  // 合并服务端和本地存储的不听名单，并把本地独有偏好补发给服务端持久化到房间状态。
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

  // 清理离开房间后不应继续复用的会话级偏好，避免下一次进入沿用旧房间状态。
  function clearRoomScopedSettings(roomId = currentRoom.value?.id || routeRoomId) {
    if (!roomId) {
      return;
    }

    clearRoomSession(window.sessionStorage);
    clearRoomPanel(window.sessionStorage, roomId);
    clearRoomNotListening(window.localStorage, roomId);
    clearMemberVolumesForRoom(window.localStorage, roomId);
  }

  // 清空内存音量缓存，进入新房间时重新按房间和成员读取本地存储。
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
