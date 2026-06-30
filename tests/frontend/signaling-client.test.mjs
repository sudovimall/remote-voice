import assert from "node:assert/strict";
import test from "node:test";

import { SignalingClient } from "../../frontend/src/lib/signaling-client.js";

class FakeWebSocket {
  static OPEN = 1;
  static instances = [];

  constructor(url) {
    this.url = url;
    this.readyState = 0;
    this.listeners = new Map();
    this.sent = [];
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  close() {
    this.readyState = 3;
    this.emit("close", {});
  }

  emit(type, event) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }

  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.emit("open", {});
  }

  receive(signal) {
    this.emit("message", { data: JSON.stringify(signal) });
  }

  send(text) {
    this.sent.push(JSON.parse(text));
  }
}

test("signaling client resolves request messages by request id", async () => {
  FakeWebSocket.instances = [];
  const client = new SignalingClient("ws://voice.test/ws", {
    requestId: () => "create-1",
    WebSocketImpl: FakeWebSocket,
  });

  const connection = client.connect();
  const socket = FakeWebSocket.instances[0];
  assert.equal(socket.url, "ws://voice.test/ws");
  socket.open();
  await connection;

  const response = client.request({ type: "create_room", nickname: "房主" });
  assert.deepEqual(socket.sent[0], {
    type: "create_room",
    request_id: "create-1",
    nickname: "房主",
  });

  socket.receive({
    type: "joined_room",
    request_id: "create-1",
    member_id: "m_owner",
  });

  assert.equal((await response).member_id, "m_owner");
});

test("signaling client delivers broadcast events to room listeners", async () => {
  FakeWebSocket.instances = [];
  const client = new SignalingClient("ws://voice.test/ws", {
    requestId: () => "unused",
    WebSocketImpl: FakeWebSocket,
  });
  const seen = [];
  client.onSignal((signal) => seen.push(signal));

  const connection = client.connect();
  const socket = FakeWebSocket.instances[0];
  socket.open();
  await connection;
  socket.receive({
    type: "member_joined",
    member_id: "m_member",
  });

  assert.deepEqual(seen, [
    {
      type: "member_joined",
      member_id: "m_member",
    },
  ]);
});
