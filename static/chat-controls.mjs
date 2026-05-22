export const CHAT_MESSAGE_MAX_CHARS = 500;

export function chatAvatarText(message) {
  const nickname = typeof message?.nickname === "string" ? message.nickname.trim() : "";
  return Array.from(nickname)[0] ?? "?";
}

export function chatTimeLabel(epochMillis, options = {}) {
  const date = new Date(epochMillis ?? Date.now());
  return date.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: options.timeZone,
  });
}

export function chatMessageView(message, ownMemberId, options = {}) {
  return {
    id: message.id,
    memberId: message.member_id,
    nickname: message.nickname,
    content: message.content,
    timeLabel: chatTimeLabel(message.sent_at_epoch_millis, options),
    avatar: chatAvatarText(message),
    own: message.member_id === ownMemberId,
  };
}

export function chatUnreadBadgeText(count) {
  if (!count) {
    return "";
  }
  return count > 99 ? "99+" : String(count);
}

export function nextChatUnreadCount(current, activePanel, message, ownMemberId) {
  if (activePanel === "chat") {
    return 0;
  }
  if (message?.member_id === ownMemberId) {
    return current;
  }

  return current + 1;
}

export function sendChatMessageSignal(content, requestId) {
  const trimmed = typeof content === "string" ? content.trim() : "";
  if (!trimmed) {
    throw new Error("聊天消息不能为空。");
  }
  if (Array.from(trimmed).length > CHAT_MESSAGE_MAX_CHARS) {
    throw new Error(`聊天消息不能超过 ${CHAT_MESSAGE_MAX_CHARS} 个字符。`);
  }

  return {
    type: "send_chat_message",
    request_id: requestId,
    content: trimmed,
  };
}
