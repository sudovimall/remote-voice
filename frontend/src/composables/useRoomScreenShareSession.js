import { computed, ref } from "vue";
import {
  setScreenViewingSignal,
  startScreenShareSignal,
  stopScreenShareSignal,
} from "../lib/room-state.js";

export function useRoomScreenShareSession({
  activeSidePanel,
  currentRoom,
  localScreenStream,
  mediaReady,
  mediaSessionRef,
  onError,
  ownMember,
  ownMemberId,
  remoteScreenStream,
  sendRoomControl,
}) {
  const localStream = localScreenStream ?? ref(null);
  const lastScreenViewing = ref(false);

  const currentScreenShare = computed(() => currentRoom.value?.screen_share ?? null);
  const activeScreenStream = computed(() => {
    const share = currentScreenShare.value;
    if (!share) {
      return null;
    }
    return share.member_id === ownMemberId.value ? localStream.value : remoteScreenStream.value;
  });
  const canStopScreenShare = computed(() => {
    const share = currentScreenShare.value;
    return Boolean(share && (share.member_id === ownMemberId.value || ownMember.value?.role === "owner"));
  });
  const canShareScreen = computed(
    () => mediaSessionRef.value?.canShareScreen?.() ?? Boolean(navigator.mediaDevices?.getDisplayMedia),
  );
  const screenShareTitle = computed(() =>
    currentScreenShare.value
      ? `${currentScreenShare.value.nickname || "成员"} 正在共享屏幕`
      : "当前没有屏幕共享",
  );
  const screenPopoutTitle = computed(() =>
    currentScreenShare.value
      ? `${currentScreenShare.value.nickname || "成员"} 的屏幕共享`
      : "屏幕共享",
  );

  function shouldReceiveScreenShare() {
    const share = currentScreenShare.value;
    return activeSidePanel.value === "screen" && share?.member_id !== ownMemberId.value;
  }

  function syncScreenViewingState(force = false) {
    const viewing = shouldReceiveScreenShare();
    if (!force && viewing === lastScreenViewing.value) {
      return;
    }

    lastScreenViewing.value = viewing;
    sendRoomControl(setScreenViewingSignal(viewing));
  }

  function applyScreenShareViewerCount(memberId, viewerCount) {
    if (memberId !== ownMemberId.value) {
      return;
    }

    mediaSessionRef.value?.setScreenShareViewerCount(viewerCount).catch((error) => {
      onError(error.message || "屏幕共享码率调整失败。");
    });
  }

  function startScreenShareRequestId() {
    return `screen-${Date.now()}`;
  }

  function stopLocalScreenShare() {
    localStream.value = null;
    mediaSessionRef.value?.stopScreenShare({ notify: false }).catch((error) => {
      onError(error.message || "停止屏幕共享失败。");
    });
  }

  function handleScreenShareStarted(signal) {
    syncScreenViewingState(true);
    if (signal.member_id === ownMemberId.value) {
      mediaSessionRef.value
        ?.startScreenShare()
        .then((stream) => {
          localStream.value = stream;
        })
        .catch((error) => {
          onError(error.message || "屏幕共享启动失败。");
          sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
        });
    }
  }

  function handleScreenShareStopped(signal) {
    if (signal.member_id === ownMemberId.value) {
      stopLocalScreenShare();
    }
    syncScreenViewingState(true);
  }

  function resetScreenStreams() {
    localStream.value = null;
    remoteScreenStream.value = null;
    lastScreenViewing.value = false;
  }

  function startScreenShare() {
    if (!mediaReady.value) {
      return;
    }
    sendRoomControl(startScreenShareSignal(startScreenShareRequestId()));
    activeSidePanel.value = "screen";
  }

  function stopScreenShare() {
    sendRoomControl(stopScreenShareSignal(startScreenShareRequestId()));
  }

  return {
    activeScreenStream,
    applyScreenShareViewerCount,
    canShareScreen,
    canStopScreenShare,
    currentScreenShare,
    handleScreenShareStarted,
    handleScreenShareStopped,
    localScreenStream: localStream,
    resetScreenStreams,
    screenPopoutTitle,
    screenShareTitle,
    startScreenShare,
    startScreenShareRequestId,
    stopLocalScreenShare,
    stopScreenShare,
    syncScreenViewingState,
  };
}
