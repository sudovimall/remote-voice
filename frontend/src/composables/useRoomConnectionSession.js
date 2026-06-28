import { RoomConnection as DefaultRoomConnection } from "../lib/room-connection.js";
import {
  createRoomSignal,
  joinRoomSignal,
  resumeRoomSignal,
  websocketUrl,
} from "../lib/room-state.js";

export function createRoomConnection(url, RoomConnection = DefaultRoomConnection) {
  return new RoomConnection(url);
}

export function useRoomConnectionSession({
  clientRef,
  connectionLabel,
  onChatMessage,
  onClose,
  onError,
  onProtocolError,
  onSignal,
}) {
  function setConnection(message) {
    connectionLabel.value = message;
  }

  function sendRoomControl(signal) {
    try {
      clientRef.value?.send(signal);
    } catch (error) {
      onError(error.message || "房间控制发送失败。");
    }
  }

  function entrySignal(intent) {
    if (intent.mode === "create") {
      return createRoomSignal(intent.nickname);
    }
    if (intent.mode === "resume") {
      return resumeRoomSignal(intent.session);
    }

    return joinRoomSignal(intent);
  }

  async function openRoomConnection(intent, RoomConnection = DefaultRoomConnection) {
    setConnection("连接中");
    const nextClient = createRoomConnection(websocketUrl(window.location), RoomConnection);
    clientRef.value = nextClient;
    nextClient.onSignal(onSignal);
    nextClient.onChatMessage(onChatMessage);
    nextClient.onProtocolError(onProtocolError);
    nextClient.onError(() => onError("房间信令连接失败。"));
    nextClient.onClose(() => onClose(nextClient));

    await nextClient.connect();
    return nextClient.enter(entrySignal(intent));
  }

  return {
    openRoomConnection,
    sendRoomControl,
    setConnection,
  };
}
