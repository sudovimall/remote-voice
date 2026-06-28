export const NICKNAME_KEY = "remote-voice.nickname";
export const ENTRY_INTENT_KEY = "remote-voice.room-entry-intent";
export const ROOM_SESSION_KEY = "remote-voice.room-session";
const ROOM_PANEL_KEY_PREFIX = "remote-voice.room-panel.";
const ROOM_NOT_LISTENING_KEY_PREFIX = "remote-voice.room-not-listening.";
const ROOM_PANELS = new Set(["members", "chat", "screen"]);

function trimmed(value) {
  return typeof value === "string" ? value.trim() : "";
}

function normalizedRoomId(value) {
  return trimmed(value).toUpperCase();
}

function prefilledLobbyPath(roomId) {
  return `/?room=${encodeURIComponent(roomId)}`;
}

function normalizeIntent(intent) {
  const nickname = trimmed(intent?.nickname);
  if (!nickname) {
    return null;
  }

  if (intent?.mode === "create") {
    return {
      mode: "create",
      nickname,
    };
  }

  if (intent?.mode === "join") {
    const roomId = normalizedRoomId(intent.roomId);
    if (!roomId) {
      return null;
    }

    return {
      mode: "join",
      roomId,
      nickname,
    };
  }

  return null;
}

function normalizeRoomSession(session) {
  const roomId = normalizedRoomId(session?.roomId);
  const memberId = trimmed(session?.memberId);
  const resumeToken = trimmed(session?.resumeToken);
  const nickname = trimmed(session?.nickname);

  if (!roomId || !memberId || !resumeToken || !nickname) {
    return null;
  }

  return {
    roomId,
    memberId,
    resumeToken,
    nickname,
  };
}

function normalizedPanel(panel) {
  return ROOM_PANELS.has(panel) ? panel : "members";
}

function roomPanelKey(roomId) {
  const normalized = normalizedRoomId(roomId);
  return normalized ? `${ROOM_PANEL_KEY_PREFIX}${normalized}` : "";
}

function roomNotListeningKey(roomId) {
  const normalized = normalizedRoomId(roomId);
  return normalized ? `${ROOM_NOT_LISTENING_KEY_PREFIX}${normalized}` : "";
}

function normalizedMemberIds(memberIds) {
  const seen = new Set();
  const values = [];
  for (const memberId of memberIds ?? []) {
    const value = trimmed(memberId);
    if (value && !seen.has(value)) {
      seen.add(value);
      values.push(value);
    }
  }

  return values;
}

export function loadNickname(storage) {
  try {
    return trimmed(storage.getItem(NICKNAME_KEY));
  } catch (_error) {
    return "";
  }
}

export function lobbyRoomId(search) {
  try {
    return normalizedRoomId(new URLSearchParams(search).get("room"));
  } catch (_error) {
    return "";
  }
}

export function directRoomEntry(storage, routeRoomId) {
  const roomId = normalizedRoomId(routeRoomId);
  if (!roomId || roomId === "NEW") {
    return null;
  }

  const nickname = loadNickname(storage);
  if (!nickname) {
    return {
      lobbyPath: prefilledLobbyPath(roomId),
    };
  }

  return {
    mode: "join",
    roomId,
    nickname,
  };
}

export function saveNickname(storage, nickname) {
  const value = trimmed(nickname);
  storage.setItem(NICKNAME_KEY, value);
  return value;
}

export function saveRoomEntryIntent(storage, intent) {
  const normalized = normalizeIntent(intent);
  if (!normalized) {
    throw new Error("房间进入信息无效。");
  }

  storage.setItem(ENTRY_INTENT_KEY, JSON.stringify(normalized));
  return normalized;
}

export function loadRoomEntryIntent(storage, routeRoomId) {
  try {
    const intent = normalizeIntent(JSON.parse(storage.getItem(ENTRY_INTENT_KEY)));
    if (!intent) {
      return null;
    }

    if (intent.mode === "create") {
      return normalizedRoomId(routeRoomId) === "NEW" ? intent : null;
    }

    return intent.roomId === normalizedRoomId(routeRoomId) ? intent : null;
  } catch (_error) {
    return null;
  }
}

export function clearRoomEntryIntent(storage) {
  try {
    storage.removeItem(ENTRY_INTENT_KEY);
  } catch (_error) {
    // Session cleanup should not break an already joined room.
  }
}

export function saveRoomSession(storage, session) {
  const normalized = normalizeRoomSession(session);
  if (!normalized) {
    throw new Error("房间恢复信息无效。");
  }

  storage.setItem(ROOM_SESSION_KEY, JSON.stringify(normalized));
  return normalized;
}

export function loadRoomSession(storage, routeRoomId) {
  try {
    const session = normalizeRoomSession(JSON.parse(storage.getItem(ROOM_SESSION_KEY)));
    if (!session || session.roomId !== normalizedRoomId(routeRoomId)) {
      return null;
    }

    return session;
  } catch (_error) {
    return null;
  }
}

export function clearRoomSession(storage) {
  try {
    storage.removeItem(ROOM_SESSION_KEY);
  } catch (_error) {
    // Losing cleanup should not block the browser from leaving the page.
  }
}

export function saveRoomNotListening(storage, roomId, memberIds) {
  const key = roomNotListeningKey(roomId);
  const values = normalizedMemberIds(memberIds);
  if (!key) {
    return values;
  }

  try {
    storage.setItem(key, JSON.stringify(values));
  } catch (_error) {
    // Listening persistence is only a local convenience.
  }
  return values;
}

export function loadRoomNotListening(storage, roomId) {
  const key = roomNotListeningKey(roomId);
  if (!key) {
    return [];
  }

  try {
    return normalizedMemberIds(JSON.parse(storage.getItem(key)));
  } catch (_error) {
    return [];
  }
}

export function clearRoomNotListening(storage, roomId) {
  const key = roomNotListeningKey(roomId);
  if (!key) {
    return;
  }

  try {
    storage.removeItem(key);
  } catch (_error) {
    // Losing cleanup should not block navigation away from a closed room.
  }
}

export function saveRoomPanel(storage, roomId, panel) {
  const key = roomPanelKey(roomId);
  const value = normalizedPanel(panel);
  if (!key) {
    return value;
  }

  try {
    storage.setItem(key, value);
  } catch (_error) {
    // Panel persistence is only a convenience for refresh/resume.
  }
  return value;
}

export function clearRoomPanel(storage, roomId) {
  const key = roomPanelKey(roomId);
  if (!key) {
    return;
  }

  try {
    storage.removeItem(key);
  } catch (_error) {
    // Panel cleanup is best-effort.
  }
}

export function loadRoomPanel(storage, roomId) {
  const key = roomPanelKey(roomId);
  if (!key) {
    return "members";
  }

  try {
    return normalizedPanel(storage.getItem(key));
  } catch (_error) {
    return "members";
  }
}
