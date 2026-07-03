import { P2PMediaSession as DefaultP2PMediaSession } from "../lib/p2p-media-session.js";
import { membersForRoom } from "../lib/room-state.js";

// 创建 P2P 媒体会话，测试可注入 fake 实现以验证组合层边界。
export function createP2PMediaSession(
  client,
  ownMemberId,
  options,
  P2PMediaSession = DefaultP2PMediaSession,
) {
  return new P2PMediaSession(client, ownMemberId, options);
}

// 封装房间 P2P 生命周期，让 useRoomSession 只负责分发信令和同步房间状态。
export function useRoomP2PSession({
  clientRef,
  currentRoom,
  media,
  ownMemberId,
  p2pSessionRef,
  onError,
}) {
  // 创建当前房间的 P2P 管理器；它独立于 SFU MediaSession，避免复用 webrtc_* 信令。
  function startP2PSession(P2PMediaSession = DefaultP2PMediaSession) {
    closeP2P();
    if (!clientRef.value || !ownMemberId.value) {
      return;
    }

    const testHooks = globalThis.__remoteVoiceP2PTest ?? null;
    p2pSessionRef.value = createP2PMediaSession(
      clientRef.value,
      ownMemberId.value,
      {
        audioHost: document.querySelector("#remote-audio"),
        PeerConnectionImpl: testHooks?.PeerConnectionImpl,
        testHooks,
        onScreenStream(stream, memberId) {
          if (!stream || currentRoom.value?.screen_share?.member_id === memberId) {
            media.remoteScreenStream.value = stream;
            testHooks?.record?.({
              type: "screen_stream_applied",
              ownMemberId: ownMemberId.value,
              memberId,
              hasStream: Boolean(stream),
            });
          }
        },
        onRemoteCameraStreams(entries) {
          media.p2pRemoteCameraStreams.value = entries;
          testHooks?.record?.({
            type: "camera_streams_applied",
            ownMemberId: ownMemberId.value,
            memberIds: entries.map((entry) => entry.memberId),
          });
        },
        onError(error) {
          onError(error.message || "P2P 媒体连接发生错误。");
        },
      },
      P2PMediaSession,
    );
    syncP2PMembers(currentRoom.value);
  }

  // 根据最新房间快照同步 P2P 成员连接，离线或离开的成员会被关闭。
  function syncP2PMembers(room = currentRoom.value) {
    p2pSessionRef.value?.syncMembers(membersForRoom(room));
  }

  // 把 SFU MediaSession 暴露出的本地轨道同步给所有可用 P2P 连接。
  function handleLocalMediaTrack(entry) {
    p2pSessionRef.value?.setLocalTrack(entry.source, entry.track, entry.stream);
  }

  // 处理后端转发的 P2P 信令，并阻止它进入普通房间快照处理。
  function handleP2PSignal(signal) {
    if (signal.type === "p2p_offer") {
      p2pSessionRef.value?.handleOffer(signal.from_member_id, signal.sdp).catch((error) => {
        onError(error.message || "P2P offer 处理失败。");
      });
      return true;
    }
    if (signal.type === "p2p_answer") {
      p2pSessionRef.value?.handleAnswer(signal.from_member_id, signal.sdp).catch((error) => {
        onError(error.message || "P2P answer 处理失败。");
      });
      return true;
    }
    if (signal.type === "p2p_ice_candidate") {
      p2pSessionRef.value?.handleIceCandidate(signal.from_member_id, signal.candidate).catch((error) => {
        onError(error.message || "P2P ICE candidate 处理失败。");
      });
      return true;
    }
    if (signal.type === "media_route_updated") {
      p2pSessionRef.value?.applyMediaRouteUpdated(signal);
      return true;
    }

    return false;
  }

  // 成员摄像头停止时同步清理 P2P 远端 tile，避免等待浏览器 track ended。
  function clearRemoteCameraStream(memberId) {
    p2pSessionRef.value?.clearRemoteCameraStream(memberId);
  }

  // 用房间发布者快照清理 P2P 摄像头流，断线恢复或全量快照不会保留旧 tile。
  function syncRemoteCameraPublishers(room = currentRoom.value) {
    const publishers = room?.video_call_publishers ?? {};
    for (const entry of media.p2pRemoteCameraStreams.value) {
      if (!publishers[entry.memberId]) {
        p2pSessionRef.value?.clearRemoteCameraStream(entry.memberId);
      }
    }
    media.p2pRemoteCameraStreams.value = media.p2pRemoteCameraStreams.value.filter(
      (entry) => publishers[entry.memberId],
    );
  }

  // 成员离开或路由回退时关闭单个成员的 P2P 连接，不影响其他成员对。
  function closeP2PMember(memberId) {
    p2pSessionRef.value?.closeMember(memberId);
  }

  // 关闭所有 P2P 连接；房间重连、离开和组件卸载都需要释放浏览器资源。
  function closeP2P() {
    p2pSessionRef.value?.close();
    p2pSessionRef.value = null;
    media.p2pRemoteCameraStreams.value = [];
  }

  return {
    clearRemoteCameraStream,
    closeP2P,
    closeP2PMember,
    handleLocalMediaTrack,
    handleP2PSignal,
    startP2PSession,
    syncRemoteCameraPublishers,
    syncP2PMembers,
  };
}
