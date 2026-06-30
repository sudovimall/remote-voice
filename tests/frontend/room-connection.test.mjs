import assert from "node:assert/strict";
import test from "node:test";

import { RoomConnection } from "../../frontend/src/lib/room-connection.js";

class FakeClient {
  constructor() {
    this.sentRequests = [];
    this.sentSignals = [];
    this.signalListeners = [];
    this.closeListeners = [];
    this.errorListeners = [];
    this.protocolErrorListeners = [];
    this.connected = false;
    this.nextResponse = null;
  }

  connect() {
    this.connected = true;
    return Promise.resolve();
  }

  request(signal) {
    this.sentRequests.push(signal);
    return Promise.resolve(this.nextResponse);
  }

  send(signal) {
    this.sentSignals.push(signal);
  }

  onSignal(listener) {
    this.signalListeners.push(listener);
    return () => {};
  }

  onClose(listener) {
    this.closeListeners.push(listener);
    return () => {};
  }

  onError(listener) {
    this.errorListeners.push(listener);
    return () => {};
  }

  onProtocolError(listener) {
    this.protocolErrorListeners.push(listener);
    return () => {};
  }

  close() {
    this.closed = true;
  }

  emit(signal) {
    for (const listener of this.signalListeners) {
      listener(signal);
    }
  }
}

test("room connection records joined room state and chat history", async () => {
  const client = new FakeClient();
  client.nextResponse = {
    type: "joined_room",
    room: { id: "ABC123", members: {} },
    member_id: "m_owner",
    resume_token: "r_owner",
    not_listening_member_ids: ["m_muted"],
    chat_messages: [{ id: "c_1", content: "历史消息" }],
  };
  const connection = new RoomConnection("ws://voice.test/ws", { client });

  const joined = await connection.enter({
    type: "create_room",
    nickname: "房主",
  });

  assert.equal(joined.member_id, "m_owner");
  assert.deepEqual(client.sentRequests[0], {
    type: "create_room",
    nickname: "房主",
  });
  assert.equal(connection.room.id, "ABC123");
  assert.equal(connection.memberId, "m_owner");
  assert.equal(connection.resumeToken, "r_owner");
  assert.deepEqual(connection.notListeningMemberIds, ["m_muted"]);
  assert.deepEqual(connection.chatMessages, [{ id: "c_1", content: "历史消息" }]);
});

test("room connection sends chat messages and emits the confirmed message", async () => {
  const client = new FakeClient();
  client.nextResponse = {
    type: "chat_message_sent",
    request_id: "chat-1",
    message: { id: "c_1", content: "晚上打哪张图？" },
  };
  const connection = new RoomConnection("ws://voice.test/ws", { client });
  const seen = [];
  connection.onChatMessage((message) => seen.push(message));

  const message = await connection.sendChatMessage("晚上打哪张图？", "chat-1", [
    { member_id: "m_member", nickname: "队友" },
  ]);

  assert.deepEqual(client.sentRequests[0], {
    type: "send_chat_message",
    request_id: "chat-1",
    content: "晚上打哪张图？",
    mentions: [{ member_id: "m_member", nickname: "队友" }],
  });
  assert.deepEqual(message, { id: "c_1", content: "晚上打哪张图？" });
  assert.deepEqual(seen, [message]);
});

test("room connection emits broadcast chat messages while preserving raw signals", () => {
  const client = new FakeClient();
  const connection = new RoomConnection("ws://voice.test/ws", { client });
  const chats = [];
  const signals = [];
  connection.onChatMessage((message) => chats.push(message));
  connection.onSignal((signal) => signals.push(signal));

  client.emit({
    type: "chat_message",
    message: { id: "c_2", content: "收到" },
  });

  assert.deepEqual(chats, [{ id: "c_2", content: "收到" }]);
  assert.deepEqual(signals, [
    {
      type: "chat_message",
      message: { id: "c_2", content: "收到" },
    },
  ]);
  assert.deepEqual(connection.chatMessages, [{ id: "c_2", content: "收到" }]);
});
