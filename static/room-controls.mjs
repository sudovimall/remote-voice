export function selfMutedSignal(selfMuted) {
  return {
    type: "set_self_muted",
    self_muted: selfMuted,
  };
}

export function memberCanSpeakSignal(memberId, canSpeak) {
  return {
    type: "set_member_can_speak",
    member_id: memberId,
    can_speak: canSpeak,
  };
}

export function memberListeningSignal(memberId, listening) {
  return {
    type: "set_member_listening",
    member_id: memberId,
    listening,
  };
}

export function memberSpeakingSignal(speaking) {
  return {
    type: "set_member_speaking",
    speaking,
  };
}

export function memberLatencySignal(serverMs) {
  return {
    type: "set_member_latency",
    server_ms: serverMs,
  };
}

export function canManageMember(room, ownMemberId, member) {
  return room?.owner_member_id === ownMemberId && member?.id !== ownMemberId;
}

export function canToggleMemberListening(ownMemberId, member) {
  return Boolean(ownMemberId && member?.id && member.id !== ownMemberId);
}

export function memberPermissionLabel(member) {
  return member?.can_speak ? "禁言" : "允许发言";
}

export function memberListeningLabel(notListening) {
  return notListening ? "接收" : "不听";
}

function roundedLatency(value) {
  return Math.round(value);
}

function latencyLabel(value) {
  if (!Number.isFinite(value)) {
    return "-- ms";
  }

  return `${roundedLatency(value)} ms`;
}

export function memberLatencyClass(value) {
  if (!Number.isFinite(value)) {
    return "member-latency member-latency-unknown";
  }
  if (value < 20) {
    return "member-latency member-latency-good";
  }
  if (value < 100) {
    return "member-latency member-latency-warn";
  }

  return "member-latency member-latency-bad";
}

export function memberLatencyView(memberId, ownMemberId, latency = {}) {
  const serverMs = latency.serverMs;
  if (memberId === ownMemberId) {
    return {
      label: latencyLabel(serverMs),
      title: `服务器延迟 ${latencyLabel(serverMs)}`,
      className: memberLatencyClass(serverMs),
    };
  }

  const memberLatency = latency.members?.[memberId];
  const remoteServerMs = memberLatency?.serverMs;
  if (!Number.isFinite(remoteServerMs)) {
    return {
      label: "-- ms",
      title: `该用户服务器延迟 -- ms；当前用户服务器延迟 ${latencyLabel(serverMs)}；总延迟 暂无该用户延迟统计`,
      className: memberLatencyClass(null),
    };
  }

  const receiveMs = Number.isFinite(memberLatency?.receiveMs) ? memberLatency.receiveMs : 0;
  const totalMs = Number.isFinite(serverMs) ? remoteServerMs + serverMs + receiveMs : null;

  return {
    label: latencyLabel(totalMs),
    title: [
      `该用户服务器延迟 ${latencyLabel(remoteServerMs)}`,
      `当前用户服务器延迟 ${latencyLabel(serverMs)}`,
      `接收缓冲 ${latencyLabel(receiveMs)}`,
      `总延迟 ${latencyLabel(totalMs)}`,
    ].join("；"),
    className: memberLatencyClass(totalMs),
  };
}
