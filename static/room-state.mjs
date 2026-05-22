export function createRoomSignal(nickname, requestId) {
  return {
    type: "create_room",
    request_id: requestId,
    nickname,
  };
}

export function joinRoomSignal(intent, requestId) {
  return {
    type: "join_room",
    request_id: requestId,
    room_id: intent.roomId,
    nickname: intent.nickname,
  };
}

export function resumeRoomSignal(session, requestId) {
  return {
    type: "resume_room",
    request_id: requestId,
    room_id: session.roomId,
    member_id: session.memberId,
    resume_token: session.resumeToken,
  };
}

export function websocketUrl(location) {
  const url = new URL("/ws", location.href);
  url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
  return url.href;
}

export function membersForRoom(room) {
  return Object.values(room?.members ?? {}).sort((left, right) => {
    if (left.id === room.owner_member_id) {
      return -1;
    }
    if (right.id === room.owner_member_id) {
      return 1;
    }

    return left.nickname.localeCompare(right.nickname, "zh-CN");
  });
}

export function nextRoomSnapshot(currentRoom, signal) {
  if (signal.type === "room_closed") {
    return null;
  }
  if (signal.room) {
    return signal.room;
  }

  return currentRoom;
}
