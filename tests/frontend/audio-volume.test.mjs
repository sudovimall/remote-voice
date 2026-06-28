import assert from "node:assert/strict";
import test from "node:test";

import {
  clearMemberVolumesForRoom,
  clampMicrophoneGain,
  clampPlaybackVolume,
  loadMemberVolume,
  loadMicrophoneGain,
  memberVolumeKey,
  saveMemberVolume,
  saveMicrophoneGain,
  volumePercent,
} from "../../static/audio-volume.mjs";

class MemoryStorage {
  constructor(values = {}) {
    this.values = new Map(Object.entries(values));
  }

  getItem(key) {
    return this.values.has(key) ? this.values.get(key) : null;
  }

  setItem(key, value) {
    this.values.set(key, String(value));
  }

  removeItem(key) {
    this.values.delete(key);
  }

  key(index) {
    return Array.from(this.values.keys())[index] ?? null;
  }

  get length() {
    return this.values.size;
  }
}

test("volume helpers clamp and format percentages", () => {
  assert.equal(clampPlaybackVolume(-0.5), 0);
  assert.equal(clampPlaybackVolume(0.75), 0.75);
  assert.equal(clampPlaybackVolume(3), 1);
  assert.equal(clampPlaybackVolume(Number.NaN), 1);
  assert.equal(clampMicrophoneGain(3), 2);
  assert.equal(volumePercent(0), "0%");
  assert.equal(volumePercent(1), "100%");
  assert.equal(volumePercent(2), "200%");
});

test("member volume key is scoped by room and member", () => {
  assert.equal(
    memberVolumeKey("ABC123", "m_member"),
    "remote_voice_member_volume:v1:ABC123:m_member",
  );
});

test("member volume storage loads valid values and falls back for invalid values", () => {
  const storage = new MemoryStorage({
    [memberVolumeKey("ABC123", "m_member")]: "0.4",
    [memberVolumeKey("ABC123", "m_bad")]: "loud",
    [memberVolumeKey("ABC123", "m_clamped")]: "5",
  });

  assert.equal(loadMemberVolume(storage, "ABC123", "m_member"), 0.4);
  assert.equal(loadMemberVolume(storage, "ABC123", "m_bad"), 1);
  assert.equal(loadMemberVolume(storage, "ABC123", "m_missing"), 1);
  assert.equal(loadMemberVolume(storage, "ABC123", "m_clamped"), 1);
});

test("member volume storage saves clamped values", () => {
  const storage = new MemoryStorage();

  saveMemberVolume(storage, "ABC123", "m_member", 0.5);
  saveMemberVolume(storage, "ABC123", "m_loud", 5);

  assert.equal(storage.getItem(memberVolumeKey("ABC123", "m_member")), "0.5");
  assert.equal(storage.getItem(memberVolumeKey("ABC123", "m_loud")), "1");
});

test("member volume cleanup removes only values for one room", () => {
  const storage = new MemoryStorage({
    [memberVolumeKey("ABC123", "m_a")]: "0.2",
    [memberVolumeKey("ABC123", "m_b")]: "0.3",
    [memberVolumeKey("OTHER", "m_a")]: "0.4",
    remote_voice_microphone_gain: "0.8",
  });

  clearMemberVolumesForRoom(storage, "ABC123");

  assert.equal(storage.getItem(memberVolumeKey("ABC123", "m_a")), null);
  assert.equal(storage.getItem(memberVolumeKey("ABC123", "m_b")), null);
  assert.equal(storage.getItem(memberVolumeKey("OTHER", "m_a")), "0.4");
  assert.equal(storage.getItem("remote_voice_microphone_gain"), "0.8");
});

test("microphone gain storage uses a global browser preference", () => {
  const storage = new MemoryStorage({
    remote_voice_microphone_gain: "0.65",
  });

  assert.equal(loadMicrophoneGain(storage), 0.65);
  saveMicrophoneGain(storage, 3);
  assert.equal(loadMicrophoneGain(storage), 2);
});
