import assert from "node:assert/strict";
import test from "node:test";

import {
  chatAvatarText,
  chatMessageView,
  chatUnreadBadgeText,
  nextChatUnreadCount,
  sendChatMessageSignal,
} from "../../static/chat-controls.mjs";

test("chat message view includes time avatar and own-member marker", () => {
  const view = chatMessageView(
    {
      id: "c_1",
      member_id: "m_owner",
      nickname: "房主",
      content: "晚上打哪张图？",
      sent_at_epoch_millis: Date.UTC(2026, 4, 23, 8, 9),
    },
    "m_owner",
    { timeZone: "UTC" },
  );

  assert.deepEqual(view, {
    id: "c_1",
    memberId: "m_owner",
    nickname: "房主",
    content: "晚上打哪张图？",
    timeLabel: "08:09",
    avatar: "房",
    own: true,
  });
});

test("chat avatar falls back to question mark and unread badge caps count", () => {
  assert.equal(chatAvatarText({ nickname: " 队友" }), "队");
  assert.equal(chatAvatarText({ nickname: "" }), "?");
  assert.equal(chatUnreadBadgeText(0), "");
  assert.equal(chatUnreadBadgeText(8), "8");
  assert.equal(chatUnreadBadgeText(120), "99+");
});

test("chat unread count ignores open chat and own messages", () => {
  const message = { member_id: "m_member" };
  assert.equal(nextChatUnreadCount(4, "chat", message, "m_owner"), 0);
  assert.equal(nextChatUnreadCount(4, "members", message, "m_owner"), 5);
  assert.equal(nextChatUnreadCount(4, "members", { member_id: "m_owner" }, "m_owner"), 4);
});

test("send chat message signal trims content and rejects invalid messages", () => {
  assert.deepEqual(sendChatMessageSignal(" 晚上打哪张图？ ", "chat-1"), {
    type: "send_chat_message",
    request_id: "chat-1",
    content: "晚上打哪张图？",
  });

  assert.throws(() => sendChatMessageSignal("   "), /不能为空/);
  assert.throws(() => sendChatMessageSignal("a".repeat(501)), /500/);
});
