import { computed, ref } from "vue";
import { MediaSession as DefaultMediaSession } from "../lib/media-session.js";
import {
  memberLatencySignal,
  memberSpeakingSignal,
  selfMutedSignal,
} from "../lib/room-controls.js";
import { stopScreenShareSignal } from "../lib/room-state.js";

export function createMediaSession(client, options, MediaSession = DefaultMediaSession) {
  return new MediaSession(client, options);
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

  return labels[group]?.[state] ?? state;
}

export function useRoomMediaSession({
  applyMemberVolumes,
  clientRef,
  currentRoom,
  localScreenStream,
  mediaSessionRef,
  microphoneGainLevel,
  onError,
  ownMember,
  sendRoomControl,
  startScreenShareRequestId,
}) {
  const latencySnapshot = ref({ serverMs: null, members: {} });
  const voiceState = ref({
    device: "idle",
    media: "waiting",
    downlink: "waiting",
  });
  const mediaReady = ref(false);
  const remoteScreenStream = ref(null);

  const deviceStateLabel = computed(() => voiceLabel("device", voiceState.value.device));
  const mediaStateLabel = computed(() => voiceLabel("media", voiceState.value.media));
  const downlinkStateLabel = computed(() => voiceLabel("downlink", voiceState.value.downlink));
  const microphoneGainSupported = computed(() => mediaSessionRef.value?.microphoneGainSupported ?? true);
  const muteSelfLabel = computed(() => (ownMember.value?.self_muted ? "取消静音" : "静音"));
  const micStateLabel = computed(() =>
    voiceState.value.media === "connected" ? "麦克风已连接" : "麦克风未连接",
  );
  const permissionNote = computed(() => {
    if (ownMember.value && !ownMember.value.can_speak) {
      return "房主已禁言，当前麦克风上行不会转发。";
    }
    if (voiceState.value.device === "denied") {
      return "麦克风权限被拒绝，房间状态仍会同步。";
    }
    if (voiceState.value.media === "connected") {
      return "语音链路已连接。";
    }
    return "麦克风权限待确认。";
  });

  function rememberLatencySnapshot(snapshot) {
    const nextMembers = { ...latencySnapshot.value.members };
    for (const [memberId, memberLatency] of Object.entries(snapshot?.members ?? {})) {
      nextMembers[memberId] = {
        ...nextMembers[memberId],
        ...memberLatency,
      };
    }
    latencySnapshot.value = {
      serverMs: Number.isFinite(snapshot?.serverMs) ? snapshot.serverMs : latencySnapshot.value.serverMs,
      members: nextMembers,
    };
    if (Number.isFinite(snapshot?.serverMs)) {
      sendRoomControl(memberLatencySignal(snapshot.serverMs));
    }
  }

  function rememberMemberLatency(memberId, serverMs, ownMemberId) {
    if (!memberId || !Number.isFinite(serverMs)) {
      return;
    }
    if (memberId === ownMemberId) {
      latencySnapshot.value = {
        ...latencySnapshot.value,
        serverMs,
      };
    } else {
      latencySnapshot.value = {
        ...latencySnapshot.value,
        members: {
          ...latencySnapshot.value.members,
          [memberId]: {
            ...latencySnapshot.value.members?.[memberId],
            serverMs,
          },
        },
      };
    }
  }

  function sendMemberSpeaking(speaking) {
    let nextSpeaking = speaking;
    if (!ownMember.value?.can_speak || ownMember.value?.self_muted) {
      nextSpeaking = false;
    }
    sendRoomControl(memberSpeakingSignal(nextSpeaking));
  }

  function renderVoiceState(patch = {}) {
    voiceState.value = {
      ...voiceState.value,
      ...patch,
    };
  }

  async function startMedia(loadClientConfig, syncScreenViewingState, MediaSession = DefaultMediaSession) {
    mediaSessionRef.value?.close();
    mediaReady.value = false;
    localScreenStream.value = null;
    remoteScreenStream.value = null;
    const clientConfig = await loadClientConfig();
    mediaSessionRef.value = createMediaSession(clientRef.value, {
      screenShare: clientConfig?.screen_share,
      audioHost: document.querySelector("#remote-audio"),
      onState: renderVoiceState,
      onLatency: rememberLatencySnapshot,
      onSpeaking: sendMemberSpeaking,
      onScreenStream(stream) {
        remoteScreenStream.value = stream;
      },
      onScreenShareEnded() {
        sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
      },
      onError(error) {
        onError(error.message || "媒体连接发生错误。");
      },
    }, MediaSession);
    mediaSessionRef.value.setMicrophoneGain(microphoneGainLevel.value);
    applyMemberVolumes();

    try {
      await mediaSessionRef.value.start();
      mediaSessionRef.value.setMuted(Boolean(ownMember.value?.self_muted));
      mediaSessionRef.value.setMicrophoneGain(microphoneGainLevel.value);
      applyMemberVolumes();
      mediaReady.value = true;
      renderVoiceState();
      syncScreenViewingState(true);
    } catch (_error) {
      mediaReady.value = false;
      renderVoiceState({ media: "failed" });
    }
  }

  function closeMedia() {
    mediaSessionRef.value?.close();
  }

  function resetMediaState() {
    mediaSessionRef.value?.close();
    mediaSessionRef.value = null;
    mediaReady.value = false;
    renderVoiceState({ media: "waiting", downlink: "waiting" });
  }

  function toggleSelfMuted() {
    const nextMuted = !ownMember.value?.self_muted;
    mediaSessionRef.value?.setMuted(nextMuted);
    sendRoomControl(selfMutedSignal(nextMuted));
  }

  return {
    closeMedia,
    deviceStateLabel,
    downlinkStateLabel,
    latencySnapshot,
    mediaReady,
    mediaStateLabel,
    micStateLabel,
    microphoneGainSupported,
    muteSelfLabel,
    permissionNote,
    remoteScreenStream,
    rememberMemberLatency,
    rememberLatencySnapshot,
    renderVoiceState,
    resetMediaState,
    startMedia,
    toggleSelfMuted,
    voiceState,
  };
}
