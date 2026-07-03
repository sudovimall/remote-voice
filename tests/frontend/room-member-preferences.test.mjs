import assert from "node:assert/strict";
import test from "node:test";

import { ref } from "vue";

import { useRoomMemberPreferences } from "../../frontend/src/composables/useRoomMemberPreferences.js";

// 构造内存版 Storage，测试成员偏好时避免依赖真实浏览器本地存储。
function memoryStorage() {
  const values = new Map();
  return {
    // 读取指定键的字符串值，保持和 Storage.getItem 的 null 语义一致。
    getItem(key) {
      return values.get(key) ?? null;
    },
    // 按插入顺序返回键名，供清理逻辑遍历存储项。
    key(index) {
      return Array.from(values.keys())[index] ?? null;
    },
    // 删除指定键，模拟离开房间时清理本地偏好。
    removeItem(key) {
      values.delete(key);
    },
    // 写入字符串值，和浏览器 Storage 一样会把值转成字符串。
    setItem(key, value) {
      values.set(key, String(value));
    },
    // 返回当前键数量，供遍历清理逻辑判断边界。
    get length() {
      return values.size;
    },
  };
}

// 注入最小 window 对象，组合函数初始化时可以读取 localStorage/sessionStorage。
function withWindow() {
  const previousWindow = globalThis.window;
  globalThis.window = {
    localStorage: memoryStorage(),
    sessionStorage: memoryStorage(),
  };
  return () => {
    globalThis.window = previousWindow;
  };
}

test("member preferences sync not-listening state to p2p without losing saved volume", () => {
  const restoreWindow = withWindow();
  try {
    const mediaVolumeCalls = [];
    const p2pVolumeCalls = [];
    const p2pListeningCalls = [];
    const preferences = useRoomMemberPreferences({
      currentRoom: ref({
        id: "ROOM1",
        members: {
          m_a: { id: "m_a", connected: true, nickname: "自己" },
          m_b: { id: "m_b", connected: true, nickname: "成员" },
        },
      }),
      mediaSessionRef: ref({
        setMemberVolume(memberId, volume) {
          mediaVolumeCalls.push({ memberId, volume });
        },
      }),
      ownMemberId: ref("m_a"),
      p2pSessionRef: ref({
        setMemberListening(memberId, listening) {
          p2pListeningCalls.push({ memberId, listening });
        },
        setMemberVolume(memberId, volume) {
          p2pVolumeCalls.push({ memberId, volume });
        },
      }),
      routeRoomId: "ROOM1",
      sendRoomControl() {},
    });

    preferences.setMemberVolume("m_b", 0.7);
    preferences.rememberListeningState(["m_b"]);

    assert.deepEqual(mediaVolumeCalls.at(-1), { memberId: "m_b", volume: 0.7 });
    assert.deepEqual(p2pVolumeCalls.at(-1), { memberId: "m_b", volume: 0.7 });
    assert.deepEqual(p2pListeningCalls.at(-1), { memberId: "m_b", listening: false });
    assert.equal(preferences.memberVolume("m_b"), 0.7);

    preferences.applyMemberVolumes();
    assert.deepEqual(p2pVolumeCalls.at(-1), { memberId: "m_b", volume: 0.7 });
    assert.deepEqual(p2pListeningCalls.at(-1), { memberId: "m_b", listening: false });

    preferences.rememberListeningState([]);
    assert.deepEqual(p2pListeningCalls.at(-1), { memberId: "m_b", listening: true });
    assert.equal(preferences.memberVolume("m_b"), 0.7);
  } finally {
    restoreWindow();
  }
});
