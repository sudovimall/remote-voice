import assert from "node:assert/strict";
import test from "node:test";

import { ref } from "vue";

import { useRoomMediaSession } from "../../frontend/src/composables/useRoomMediaSession.js";

// 注入最小 document 对象，媒体组合层创建会话时可以查询远端音频容器。
function withDocument() {
  const previousDocument = globalThis.document;
  globalThis.document = {
    // 测试不渲染真实 DOM，返回 null 让媒体会话使用空音频宿主。
    querySelector() {
      return null;
    },
  };
  return () => {
    globalThis.document = previousDocument;
  };
}

test("room media session mutes local track when can_speak is false", async () => {
  const restoreDocument = withDocument();
  try {
    const sentSignals = [];
    const localTracks = [];
    let createdSession = null;

    class FakeMediaSession {
      // 保存媒体回调和静音记录，测试组合层是否把权限应用到媒体会话。
      constructor(_client, options) {
        this.options = options;
        this.mutedStates = [];
        this.microphoneGainSupported = true;
        createdSession = this;
      }

      // 模拟启动麦克风采集，并把同一条音轨通过回调暴露给 P2P。
      async start() {
        this.track = { enabled: true, kind: "audio" };
        this.stream = {
          getAudioTracks: () => [this.track],
          getTracks: () => [this.track],
        };
        this.options.onLocalMediaTrack({
          source: "audio",
          stream: this.stream,
          track: this.track,
        });
      }

      // 假媒体会话没有真实浏览器资源，关闭时保持空操作。
      close() {}

      // 本测试不关心远端成员音量，保留空实现满足组合层调用。
      setMemberVolume() {}

      // 本测试不关心麦克风增益，保留空实现满足组合层调用。
      setMicrophoneGain() {}

      // 记录静音状态并更新本地音轨 enabled，验证 SFU/P2P 共享音轨都被禁用。
      setMuted(muted) {
        this.mutedStates.push(muted);
        if (this.track) {
          this.track.enabled = !muted;
        }
      }
    }

    const ownMember = ref({ id: "m_self", can_speak: false, self_muted: false });
    const media = useRoomMediaSession({
      applyMemberVolumes() {},
      clientRef: ref({}),
      currentRoom: ref({ id: "ROOM1", video_call_publishers: {} }),
      localScreenStream: ref(null),
      mediaSessionRef: ref(null),
      microphoneGainLevel: ref(1),
      onError() {},
      onLocalMediaTrack(entry) {
        localTracks.push(entry);
      },
      ownMember,
      sendRoomControl(signal) {
        sentSignals.push(signal);
      },
      startScreenShareRequestId: () => "screen-1",
    });

    await media.startMedia(async () => ({}), () => {}, FakeMediaSession);

    assert.deepEqual(createdSession.mutedStates, [true]);
    assert.equal(createdSession.track.enabled, false);
    assert.equal(localTracks[0].track.enabled, false);

    ownMember.value = { id: "m_self", can_speak: true, self_muted: false };
    media.syncEffectiveSelfMuted();
    assert.equal(createdSession.mutedStates.at(-1), false);
    assert.equal(createdSession.track.enabled, true);

    ownMember.value = { id: "m_self", can_speak: false, self_muted: true };
    media.toggleSelfMuted();
    assert.equal(createdSession.mutedStates.at(-1), true);
    assert.equal(createdSession.track.enabled, false);
    assert.equal(sentSignals.at(-1).type, "set_self_muted");
    assert.equal(sentSignals.at(-1).self_muted, false);
  } finally {
    restoreDocument();
  }
});
