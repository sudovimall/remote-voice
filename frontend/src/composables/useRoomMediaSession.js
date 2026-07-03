import { computed, ref } from "vue";
import { MediaSession as DefaultMediaSession } from "../lib/media-session.js";
import {
  memberLatencySignal,
  memberSpeakingSignal,
  selfMutedSignal,
} from "../lib/room-controls.js";
import {
  startVideoCallSignal,
  stopScreenShareSignal,
  stopVideoCallSignal,
} from "../lib/room-state.js";

// 创建可注入的媒体会话实例，让生产环境使用真实 WebRTC，测试环境替换为假实现。
export function createMediaSession(client, options, MediaSession = DefaultMediaSession) {
  return new MediaSession(client, options);
}

// 将内部状态枚举转换为用户可读文案，避免模板层散落状态判断。
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

// 管理房间内 SFU 媒体会话和本地媒体状态，集中处理权限、静音和视频发布副作用。
export function useRoomMediaSession({
  applyMemberVolumes,
  clientRef,
  currentRoom,
  localScreenStream,
  mediaSessionRef,
  microphoneGainLevel,
  onLocalMediaTrack,
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
  const localCameraStream = ref(null);
  const remoteCameraStreams = ref([]);
  const cameraState = ref("idle");
  const cameraBusy = ref(false);

  const deviceStateLabel = computed(() => voiceLabel("device", voiceState.value.device));
  const mediaStateLabel = computed(() => voiceLabel("media", voiceState.value.media));
  const downlinkStateLabel = computed(() => voiceLabel("downlink", voiceState.value.downlink));
  const microphoneGainSupported = computed(() => mediaSessionRef.value?.microphoneGainSupported ?? true);
  const muteSelfLabel = computed(() => (ownMember.value?.self_muted ? "取消静音" : "静音"));
  const canUseCamera = computed(
    () => mediaSessionRef.value?.canUseCamera?.() ?? Boolean(globalThis.navigator?.mediaDevices?.getUserMedia),
  );
  const ownVideoCallPublisher = computed(() =>
    Boolean(currentRoom.value?.video_call_publishers?.[ownMember.value?.id]),
  );
  const cameraToggleLabel = computed(() => {
    if (cameraBusy.value) {
      return cameraState.value === "requesting" ? "正在开启" : "正在处理";
    }
    return ownVideoCallPublisher.value || localCameraStream.value ? "关闭摄像头" : "开启摄像头";
  });
  const cameraStateLabel = computed(() => {
    const labels = {
      idle: "摄像头未开启",
      requesting: "正在请求摄像头权限",
      active: "摄像头已开启",
      denied: "摄像头权限被拒绝",
      failed: "摄像头连接失败",
    };
    return labels[cameraState.value] ?? "摄像头未开启";
  });
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

  // 合并媒体层延迟快照并上报本机到服务端的延迟，成员列表展示复用这份状态。
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

  // 记录其他成员广播来的延迟值，保持成员列表中的质量指标随信令更新。
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

  // 发送发言状态前再次套用本地权限，防止禁言或自我静音时显示正在说话。
  function sendMemberSpeaking(speaking) {
    let nextSpeaking = speaking;
    if (!ownMember.value?.can_speak || ownMember.value?.self_muted) {
      nextSpeaking = false;
    }
    sendRoomControl(memberSpeakingSignal(nextSpeaking));
  }

  // 合并媒体状态补丁，媒体层多处异步回调都通过这里刷新 UI 标签。
  function renderVoiceState(patch = {}) {
    voiceState.value = {
      ...voiceState.value,
      ...patch,
    };
  }

  // 计算实际上行静音；房主禁言必须像自我静音一样禁用本地轨道，覆盖 SFU 和 P2P。
  function effectiveSelfMuted(selfMuted = ownMember.value?.self_muted) {
    return Boolean(selfMuted || ownMember.value?.can_speak === false);
  }

  // 将有效静音状态应用到当前媒体会话，启动、重连和权限广播都复用同一规则。
  function syncEffectiveSelfMuted(selfMuted = ownMember.value?.self_muted) {
    mediaSessionRef.value?.setMuted(effectiveSelfMuted(selfMuted));
  }

  // 为摄像头控制生成独立请求号，便于服务端错误能定位到本次操作。
  function startVideoCallRequestId() {
    return `camera-${Date.now()}`;
  }

  // 启动 SFU 媒体会话并把本地轨道变化透传给 P2P 管理器，两个链路共享浏览器权限结果。
  async function startMedia(loadClientConfig, syncScreenViewingState, MediaSession = DefaultMediaSession) {
    mediaSessionRef.value?.close();
    mediaReady.value = false;
    localScreenStream.value = null;
    remoteScreenStream.value = null;
    localCameraStream.value = null;
    remoteCameraStreams.value = [];
    cameraState.value = "idle";
    cameraBusy.value = false;
    const clientConfig = await loadClientConfig();
    mediaSessionRef.value = createMediaSession(clientRef.value, {
      screenShare: clientConfig?.screen_share,
      videoCall: clientConfig?.video_call,
      audioHost: document.querySelector("#remote-audio"),
      onState: renderVoiceState,
      onLatency: rememberLatencySnapshot,
      onSpeaking: sendMemberSpeaking,
      // 接收远端屏幕共享流后只更新展示状态，停止逻辑由服务端广播驱动。
      onScreenStream(stream) {
        remoteScreenStream.value = stream;
      },
      // 本地屏幕轨道自然结束时通知服务端释放房间共享状态。
      onScreenShareEnded() {
        sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
      },
      // 保存本地摄像头流用于自预览，实际成员状态仍以服务端广播为准。
      onLocalCameraStream(stream) {
        localCameraStream.value = stream;
      },
      onLocalMediaTrack,
      // 用整体替换方式刷新远端摄像头流，保证 Vue 响应式列表能稳定更新。
      onRemoteCameraStreams(entries) {
        remoteCameraStreams.value = entries;
      },
      // 本地摄像头轨道结束时通知服务端清除发布者状态。
      onCameraEnded() {
        sendRoomControl(stopVideoCallSignal(startVideoCallRequestId()));
      },
      // 媒体内部错误转为房间错误提示，避免未捕获异常打断页面状态。
      onError(error) {
        onError(error.message || "媒体连接发生错误。");
      },
    }, MediaSession);
    mediaSessionRef.value.setMicrophoneGain(microphoneGainLevel.value);
    applyMemberVolumes();

    try {
      await mediaSessionRef.value.start();
      syncEffectiveSelfMuted();
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

  // 关闭当前媒体会话，保留 ref 由上层决定是否重建。
  function closeMedia() {
    mediaSessionRef.value?.close();
  }

  // 重置媒体 UI 状态并释放浏览器媒体资源，用于重连和离开房间。
  function resetMediaState() {
    mediaSessionRef.value?.close();
    mediaSessionRef.value = null;
    mediaReady.value = false;
    localCameraStream.value = null;
    remoteCameraStreams.value = [];
    cameraState.value = "idle";
    cameraBusy.value = false;
    renderVoiceState({ media: "waiting", downlink: "waiting" });
  }

  // 切换自我静音并立即更新本地轨道；禁言状态下即使取消自我静音也不能打开上行。
  function toggleSelfMuted() {
    const nextMuted = !ownMember.value?.self_muted;
    syncEffectiveSelfMuted(nextMuted);
    sendRoomControl(selfMutedSignal(nextMuted));
  }

  // 切换摄像头发布状态；先发信令占用房间状态，再由广播回执触发本地采集。
  function toggleCamera() {
    if (!mediaReady.value || !canUseCamera.value || cameraBusy.value) {
      return;
    }
    if (ownVideoCallPublisher.value || localCameraStream.value) {
      cameraBusy.value = true;
      sendRoomControl(stopVideoCallSignal(startVideoCallRequestId()));
      return;
    }

    cameraBusy.value = true;
    cameraState.value = "requesting";
    sendRoomControl(startVideoCallSignal(startVideoCallRequestId()));
  }

  // 处理服务端确认摄像头开启；只有当前成员收到自己的确认后才请求浏览器权限。
  function handleVideoCallStarted(signal) {
    if (signal.member_id !== ownMember.value?.id) {
      return;
    }
    const mediaSession = mediaSessionRef.value;
    if (!mediaSession?.startCamera) {
      cameraState.value = "failed";
      cameraBusy.value = false;
      onError("媒体会话尚未连接。");
      sendRoomControl(stopVideoCallSignal(startVideoCallRequestId()));
      return;
    }
    cameraBusy.value = true;
    cameraState.value = "requesting";
    mediaSession
      .startCamera()
      .then(() => {
        cameraState.value = "active";
      })
      .catch((error) => {
        cameraState.value = error?.name === "NotAllowedError" ? "denied" : "failed";
        onError(error.message || "摄像头启动失败。");
        sendRoomControl(stopVideoCallSignal(startVideoCallRequestId()));
      })
      .finally(() => {
        cameraBusy.value = false;
      });
  }

  // 处理摄像头停止广播；本地成员释放采集资源，远端成员清理宫格流。
  function handleVideoCallStopped(signal) {
    mediaSessionRef.value?.clearRemoteCameraStream?.(signal.member_id);
    if (signal.member_id !== ownMember.value?.id) {
      return;
    }
    cameraBusy.value = false;
    cameraState.value = "idle";
    localCameraStream.value = null;
    mediaSessionRef.value?.stopCamera({ notify: false }).catch((error) => {
      onError(error.message || "停止摄像头失败。");
    });
  }

  // 根据房间当前摄像头发布人数调整本地 sender 码率，降低多人视频压力。
  function applyVideoCallPublisherCount(publisherCount) {
    mediaSessionRef.value?.setVideoCallPublisherCount(publisherCount).catch((error) => {
      onError(error.message || "摄像头码率调整失败。");
    });
  }

  // 用房间快照校正远端摄像头流，避免断线恢复后保留过期视频 tile。
  function syncVideoCallPublishers(room) {
    const publishers = room?.video_call_publishers ?? {};
    applyVideoCallPublisherCount(Object.keys(publishers).length);
    for (const entry of remoteCameraStreams.value) {
      if (!publishers[entry.memberId]) {
        mediaSessionRef.value?.clearRemoteCameraStream?.(entry.memberId);
      }
    }
  }

  return {
    applyVideoCallPublisherCount,
    cameraBusy,
    cameraState,
    cameraStateLabel,
    cameraToggleLabel,
    canUseCamera,
    closeMedia,
    deviceStateLabel,
    downlinkStateLabel,
    handleVideoCallStarted,
    handleVideoCallStopped,
    latencySnapshot,
    localCameraStream,
    mediaReady,
    mediaStateLabel,
    micStateLabel,
    microphoneGainSupported,
    muteSelfLabel,
    permissionNote,
    remoteScreenStream,
    remoteCameraStreams,
    rememberMemberLatency,
    rememberLatencySnapshot,
    renderVoiceState,
    resetMediaState,
    startMedia,
    syncEffectiveSelfMuted,
    syncVideoCallPublishers,
    toggleCamera,
    toggleSelfMuted,
    voiceState,
  };
}
