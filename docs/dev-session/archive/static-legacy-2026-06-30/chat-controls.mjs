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

export function mentionCandidates(room, ownMemberId) {
  return Object.values(room?.members ?? {})
    .filter((member) => member.id && member.id !== ownMemberId)
    .sort((left, right) => {
      if (left.id === room?.owner_member_id) {
        return -1;
      }
      if (right.id === room?.owner_member_id) {
        return 1;
      }

      return (left.nickname ?? "").localeCompare(right.nickname ?? "", "zh-CN");
    });
}

export function insertMentionText(inputValue, selectionStart, selectionEnd, member) {
  const value = typeof inputValue === "string" ? inputValue : "";
  const start = Number.isInteger(selectionStart) ? selectionStart : value.length;
  const end = Number.isInteger(selectionEnd) ? selectionEnd : start;
  const prefix = value.slice(0, start);
  const atIndex = prefix.lastIndexOf("@");
  const replaceStart = atIndex >= 0 ? atIndex : start;
  const mentionText = `@${member?.nickname ?? ""} `;
  const replaceEnd = value[end] === " " ? end + 1 : end;
  const nextValue = `${value.slice(0, replaceStart)}${mentionText}${value.slice(replaceEnd)}`;

  return {
    value: nextValue,
    cursor: replaceStart + mentionText.length,
    mention: {
      member_id: member?.id,
      nickname: member?.nickname,
    },
  };
}

export function mentionsForSend(content, selectedMentions = []) {
  const text = typeof content === "string" ? content : "";
  const seen = new Set();
  const mentions = [];
  for (const mention of selectedMentions) {
    const memberId = mention?.member_id;
    const nickname = mention?.nickname;
    if (!memberId || !nickname || seen.has(memberId) || !text.includes(`@${nickname}`)) {
      continue;
    }

    seen.add(memberId);
    mentions.push({ member_id: memberId, nickname });
  }

  return mentions;
}

export function messageMentionsCurrentMember(message, ownMemberId) {
  if (!ownMemberId) {
    return false;
  }

  return (message?.mentions ?? []).some((mention) => mention?.member_id === ownMemberId);
}

export function chatMessageContentParts(message) {
  const content = typeof message?.content === "string" ? message.content : "";
  const mentions = message?.mentions ?? [];
  const matches = [];

  for (const mention of mentions) {
    const nickname = mention?.nickname;
    if (!nickname) {
      continue;
    }
    const text = `@${nickname}`;
    let index = content.indexOf(text);
    while (index >= 0) {
      matches.push({
        index,
        end: index + text.length,
        text,
        memberId: mention.member_id,
      });
      index = content.indexOf(text, index + text.length);
    }
  }

  matches.sort((left, right) => left.index - right.index || right.end - left.end);
  const parts = [];
  let cursor = 0;
  for (const match of matches) {
    if (match.index < cursor) {
      continue;
    }
    if (match.index > cursor) {
      parts.push({ type: "text", text: content.slice(cursor, match.index) });
    }
    parts.push({ type: "mention", text: match.text, memberId: match.memberId });
    cursor = match.end;
  }
  if (cursor < content.length) {
    parts.push({ type: "text", text: content.slice(cursor) });
  }

  return parts;
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

export function sendChatMessageSignal(content, requestId, mentions = []) {
  const trimmed = typeof content === "string" ? content.trim() : "";
  if (!trimmed) {
    throw new Error("聊天消息不能为空。");
  }
  if (Array.from(trimmed).length > CHAT_MESSAGE_MAX_CHARS) {
    throw new Error(`聊天消息不能超过 ${CHAT_MESSAGE_MAX_CHARS} 个字符。`);
  }

  const signal = {
    type: "send_chat_message",
    request_id: requestId,
    content: trimmed,
  };
  if (mentions.length) {
    signal.mentions = mentions;
  }

  return signal;
}
