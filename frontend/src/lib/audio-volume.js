const DEFAULT_VOLUME = 1;
const MIN_VOLUME = 0;
const MAX_PLAYBACK_VOLUME = 1;
const MAX_MICROPHONE_GAIN = 2;
const MEMBER_VOLUME_PREFIX = "remote_voice_member_volume:v1";
const MICROPHONE_GAIN_KEY = "remote_voice_microphone_gain";

function clamp(value, max) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return DEFAULT_VOLUME;
  }

  return Math.min(max, Math.max(MIN_VOLUME, numeric));
}

export function clampPlaybackVolume(value) {
  return clamp(value, MAX_PLAYBACK_VOLUME);
}

export function clampMicrophoneGain(value) {
  return clamp(value, MAX_MICROPHONE_GAIN);
}

export function volumePercent(value) {
  const numeric = Number(value);
  const volume = Number.isFinite(numeric) ? numeric : DEFAULT_VOLUME;
  return `${Math.round(volume * 100)}%`;
}

export function memberVolumeKey(roomId, memberId) {
  return `${MEMBER_VOLUME_PREFIX}:${roomId}:${memberId}`;
}

function safeGet(storage, key) {
  try {
    return storage?.getItem(key) ?? null;
  } catch (_error) {
    return null;
  }
}

function safeSet(storage, key, value) {
  try {
    storage?.setItem(key, String(value));
  } catch (_error) {
    // Storage can be disabled; the current in-memory UI state still works.
  }
}

function safeRemove(storage, key) {
  try {
    storage?.removeItem(key);
  } catch (_error) {
    // Storage cleanup is best-effort.
  }
}

function storageKeys(storage) {
  try {
    return Array.from({ length: storage?.length ?? 0 }, (_, index) => storage.key(index)).filter(Boolean);
  } catch (_error) {
    return [];
  }
}

function loadVolume(storage, key) {
  const raw = safeGet(storage, key);
  if (raw === null) {
    return DEFAULT_VOLUME;
  }

  return clampMicrophoneGain(raw);
}

export function loadMemberVolume(storage, roomId, memberId) {
  if (!roomId || !memberId) {
    return DEFAULT_VOLUME;
  }

  const raw = safeGet(storage, memberVolumeKey(roomId, memberId));
  if (raw === null) {
    return DEFAULT_VOLUME;
  }

  return clampPlaybackVolume(raw);
}

export function saveMemberVolume(storage, roomId, memberId, value) {
  if (!roomId || !memberId) {
    return;
  }

  safeSet(storage, memberVolumeKey(roomId, memberId), clampPlaybackVolume(value));
}

export function clearMemberVolumesForRoom(storage, roomId) {
  if (!roomId) {
    return;
  }

  const prefix = `${MEMBER_VOLUME_PREFIX}:${roomId}:`;
  for (const key of storageKeys(storage)) {
    if (key.startsWith(prefix)) {
      safeRemove(storage, key);
    }
  }
}

export function loadMicrophoneGain(storage) {
  return loadVolume(storage, MICROPHONE_GAIN_KEY);
}

export function saveMicrophoneGain(storage, value) {
  safeSet(storage, MICROPHONE_GAIN_KEY, clampMicrophoneGain(value));
}
