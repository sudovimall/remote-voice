import assert from "node:assert/strict";
import test from "node:test";

import {
  chatAvatarText,
  chatMessageContentParts,
  chatMessageView,
  chatUnreadBadgeText,
  insertMentionText,
  mentionCandidates,
  mentionsForSend,
  messageMentionsCurrentMember,
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

test("mention candidates exclude self and sort owner first", () => {
  const candidates = mentionCandidates(
    {
      owner_member_id: "m_owner",
      members: {
        m_b: { id: "m_b", nickname: "周末" },
        m_owner: { id: "m_owner", nickname: "房主" },
        m_a: { id: "m_a", nickname: "阿木" },
      },
    },
    "m_b",
  );

  assert.deepEqual(
    candidates.map((member) => member.id),
    ["m_owner", "m_a"],
  );
});

test("mention insertion replaces the active at token", () => {
  assert.deepEqual(insertMentionText("晚上 @a 打哪张图", 5, 5, { nickname: "阿木" }), {
    value: "晚上 @阿木 打哪张图",
    cursor: 7,
    mention: { member_id: undefined, nickname: "阿木" },
  });

  assert.deepEqual(insertMentionText("晚上打哪张图", 2, 2, { id: "m_a", nickname: "阿木" }), {
    value: "晚上@阿木 打哪张图",
    cursor: 6,
    mention: { member_id: "m_a", nickname: "阿木" },
  });
});

test("mentions for send filters deleted mentions and deduplicates members", () => {
  assert.deepEqual(
    mentionsForSend("@阿木 @阿木 普通 @文字", [
      { member_id: "m_a", nickname: "阿木" },
      { member_id: "m_a", nickname: "阿木" },
      { member_id: "m_missing", nickname: "不存在" },
    ]),
    [{ member_id: "m_a", nickname: "阿木" }],
  );
});

test("message mention helpers detect current member and split highlighted parts", () => {
  const message = {
    content: "收到 @阿木 和 @周末",
    mentions: [
      { member_id: "m_a", nickname: "阿木" },
      { member_id: "m_b", nickname: "周末" },
    ],
  };

  assert.equal(messageMentionsCurrentMember(message, "m_a"), true);
  assert.equal(messageMentionsCurrentMember(message, "m_x"), false);
  assert.deepEqual(chatMessageContentParts(message), [
    { type: "text", text: "收到 " },
    { type: "mention", text: "@阿木", memberId: "m_a" },
    { type: "text", text: " 和 " },
    { type: "mention", text: "@周末", memberId: "m_b" },
  ]);
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

  assert.deepEqual(
    sendChatMessageSignal(" @阿木 晚上打哪张图？ ", "chat-1", [
      { member_id: "m_a", nickname: "阿木" },
    ]),
    {
      type: "send_chat_message",
      request_id: "chat-1",
      content: "@阿木 晚上打哪张图？",
      mentions: [{ member_id: "m_a", nickname: "阿木" }],
    },
  );

  assert.throws(() => sendChatMessageSignal("   "), /不能为空/);
  assert.throws(() => sendChatMessageSignal("a".repeat(501)), /500/);
});
