import assert from "node:assert/strict";
import test from "node:test";

import { ref } from "vue";

import { useRoomMediaSession } from "../../frontend/src/composables/useRoomMediaSession.js";
import { useRoomP2PSession } from "../../frontend/src/composables/useRoomP2PSession.js";

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

    assert.equal(createdSession.options.initialMuted, true);
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

test("room media session merges sfu and p2p remote camera streams", async () => {
  const restoreDocument = withDocument();
  try {
    let createdSession = null;

    class FakeMediaSession {
      // 保存媒体回调，测试组合层如何合并不同来源的远端摄像头流。
      constructor(_client, options) {
        this.options = options;
        this.microphoneGainSupported = true;
        createdSession = this;
      }

      // 本测试只关注远端摄像头状态，不需要模拟真实本地媒体采集。
      async start() {}

      // 假媒体会话没有真实浏览器资源，关闭时保持空操作。
      close() {}

      // 本测试不关心远端成员音量，保留空实现满足组合层调用。
      setMemberVolume() {}

      // 本测试不关心麦克风增益，保留空实现满足组合层调用。
      setMicrophoneGain() {}

      // 本测试不关心本地静音，保留空实现满足组合层启动流程。
      setMuted() {}

      // 记录组合层会按房间发布状态清理 SFU 过期摄像头流。
      clearRemoteCameraStream(memberId) {
        this.clearedMemberId = memberId;
      }

      // 本测试不关心码率，只记录调用满足组合层发布人数同步流程。
      async setVideoCallPublisherCount(count) {
        this.publisherCount = count;
      }
    }

    const media = useRoomMediaSession({
      applyMemberVolumes() {},
      clientRef: ref({}),
      currentRoom: ref({ id: "ROOM1", video_call_publishers: {} }),
      localScreenStream: ref(null),
      mediaSessionRef: ref(null),
      microphoneGainLevel: ref(1),
      onError() {},
      onLocalMediaTrack() {},
      ownMember: ref({ id: "m_self", can_speak: true, self_muted: false }),
      sendRoomControl() {},
      startScreenShareRequestId: () => "screen-1",
    });

    await media.startMedia(async () => ({}), () => {}, FakeMediaSession);
    const sfuA = { id: "sfu-a" };
    const sfuB = { id: "sfu-b" };
    const p2pB = { id: "p2p-b" };
    const p2pC = { id: "p2p-c" };

    createdSession.options.onRemoteCameraStreams([
      { memberId: "m_a", stream: sfuA },
      { memberId: "m_b", stream: sfuB },
    ]);
    media.p2pRemoteCameraStreams.value = [
      { memberId: "m_b", stream: p2pB },
      { memberId: "m_c", stream: p2pC },
    ];

    assert.deepEqual(
      media.remoteCameraStreams.value.map((entry) => [entry.memberId, entry.stream.id]),
      [
        ["m_a", "sfu-a"],
        ["m_b", "p2p-b"],
        ["m_c", "p2p-c"],
      ],
    );

    media.syncVideoCallPublishers({
      video_call_publishers: {
        m_a: {},
        m_b: {},
      },
    });
    assert.equal(createdSession.publisherCount, 2);
  } finally {
    restoreDocument();
  }
});

test("room p2p session removes stale remote camera streams from publisher snapshot", () => {
  const clearedMembers = [];
  const p2pSessionRef = ref({
    // 记录组合层是否调用 P2P 会话清理过期远端摄像头。
    clearRemoteCameraStream(memberId) {
      clearedMembers.push(memberId);
    },
  });
  const media = {
    p2pRemoteCameraStreams: ref([
      { memberId: "m_live", stream: { id: "live" } },
      { memberId: "m_stale", stream: { id: "stale" } },
    ]),
  };
  const p2p = useRoomP2PSession({
    clientRef: ref(null),
    currentRoom: ref({
      video_call_publishers: {
        m_live: {},
      },
    }),
    media,
    ownMemberId: ref("m_self"),
    p2pSessionRef,
    onError() {},
  });

  p2p.syncRemoteCameraPublishers();

  assert.deepEqual(clearedMembers, ["m_stale"]);
  assert.deepEqual(media.p2pRemoteCameraStreams.value, [
    { memberId: "m_live", stream: { id: "live" } },
  ]);
});
