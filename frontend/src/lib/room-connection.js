import { SignalingClient } from "./signaling-client.js";

function subscribe(listeners, listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function notify(listeners, value) {
  for (const listener of listeners) {
    listener(value);
  }
}

export class RoomConnection {
  constructor(url, options = {}) {
    this.client = options.client ?? new SignalingClient(url, options);
    this.room = null;
    this.memberId = "";
    this.resumeToken = "";
    this.notListeningMemberIds = [];
    this.chatMessages = [];
    this.signalListeners = new Set();
    this.chatMessageListeners = new Set();

    this.client.onSignal((signal) => this.handleSignal(signal));
  }

  connect() {
    return this.client.connect();
  }

  request(signal) {
    return this.client.request(signal);
  }

  send(signal) {
    this.client.send(signal);
  }

  close() {
    this.client.close();
  }

  onSignal(listener) {
    return subscribe(this.signalListeners, listener);
  }

  onChatMessage(listener) {
    return subscribe(this.chatMessageListeners, listener);
  }

  onClose(listener) {
    return this.client.onClose(listener);
  }

  onError(listener) {
    return this.client.onError(listener);
  }

  onProtocolError(listener) {
    return this.client.onProtocolError(listener);
  }

  async enter(signal) {
    const joined = await this.client.request(signal);
    this.applyJoinedRoom(joined);
    return joined;
  }

  async sendChatMessage(content, requestId, mentions = []) {
    const signal = {
      type: "send_chat_message",
      request_id: requestId,
      content,
    };
    if (mentions.length) {
      signal.mentions = mentions;
    }

    const response = await this.client.request(signal);
    if (response?.message) {
      this.rememberChatMessage(response.message);
      notify(this.chatMessageListeners, response.message);
    }

    return response?.message ?? null;
  }

  handleSignal(signal) {
    if (signal.type === "joined_room") {
      this.applyJoinedRoom(signal);
    } else if (signal.type === "chat_message" && signal.message) {
      this.rememberChatMessage(signal.message);
      notify(this.chatMessageListeners, signal.message);
    }

    notify(this.signalListeners, signal);
  }

  applyJoinedRoom(joined) {
    this.room = joined.room ?? null;
    this.memberId = joined.member_id ?? "";
    this.resumeToken = joined.resume_token ?? "";
    this.notListeningMemberIds = joined.not_listening_member_ids ?? [];
    this.chatMessages = joined.chat_messages ?? [];
  }

  rememberChatMessage(message) {
    this.chatMessages = [...this.chatMessages, message];
  }
}
