function defaultRequestId(type) {
  if (globalThis.crypto?.randomUUID) {
    return `${type}-${globalThis.crypto.randomUUID()}`;
  }

  return `${type}-${Date.now()}`;
}

function signalError(signal) {
  const error = new Error(signal.message || "信令请求失败。");
  error.signal = signal;
  return error;
}

export class SignalingClient {
  constructor(url, options = {}) {
    this.url = url;
    this.WebSocketImpl = options.WebSocketImpl ?? globalThis.WebSocket;
    this.requestId = options.requestId ?? defaultRequestId;
    this.socket = null;
    this.pending = new Map();
    this.signalListeners = new Set();
    this.closeListeners = new Set();
    this.errorListeners = new Set();
    this.protocolErrorListeners = new Set();
  }

  connect() {
    if (!this.WebSocketImpl) {
      return Promise.reject(new Error("当前环境没有 WebSocket。"));
    }
    if (this.socket) {
      return Promise.resolve();
    }

    this.socket = new this.WebSocketImpl(this.url);

    return new Promise((resolve, reject) => {
      let opened = false;
      this.socket.addEventListener("open", () => {
        opened = true;
        resolve();
      });
      this.socket.addEventListener("message", (event) => {
        this.handleMessage(event.data);
      });
      this.socket.addEventListener("error", (event) => {
        if (!opened) {
          reject(new Error("WebSocket 连接失败。"));
        }
        this.notify(this.errorListeners, event);
      });
      this.socket.addEventListener("close", (event) => {
        if (!opened) {
          reject(new Error("WebSocket 在连接前关闭。"));
        }
        this.rejectPending(new Error("WebSocket 已关闭。"));
        this.notify(this.closeListeners, event);
      });
    });
  }

  request(signal) {
    const openState = this.WebSocketImpl.OPEN ?? 1;
    if (!this.socket || this.socket.readyState !== openState) {
      return Promise.reject(new Error("WebSocket 尚未连接。"));
    }

    const requestId = signal.request_id ?? this.requestId(signal.type);
    const body = {
      ...signal,
      request_id: requestId,
    };

    return new Promise((resolve, reject) => {
      this.pending.set(requestId, { resolve, reject });
      try {
        this.socket.send(JSON.stringify(body));
      } catch (error) {
        this.pending.delete(requestId);
        reject(error);
      }
    });
  }

  send(signal) {
    const openState = this.WebSocketImpl.OPEN ?? 1;
    if (!this.socket || this.socket.readyState !== openState) {
      throw new Error("WebSocket 尚未连接。");
    }

    this.socket.send(JSON.stringify(signal));
  }

  onSignal(listener) {
    return this.subscribe(this.signalListeners, listener);
  }

  onClose(listener) {
    return this.subscribe(this.closeListeners, listener);
  }

  onError(listener) {
    return this.subscribe(this.errorListeners, listener);
  }

  onProtocolError(listener) {
    return this.subscribe(this.protocolErrorListeners, listener);
  }

  close() {
    this.socket?.close();
  }

  handleMessage(text) {
    let signal;
    try {
      signal = JSON.parse(text);
    } catch (error) {
      this.notify(this.protocolErrorListeners, error);
      return;
    }

    const pending = signal.request_id ? this.pending.get(signal.request_id) : null;
    if (pending) {
      this.pending.delete(signal.request_id);
      if (signal.type === "error") {
        pending.reject(signalError(signal));
      } else {
        pending.resolve(signal);
      }
      return;
    }

    this.notify(this.signalListeners, signal);
  }

  rejectPending(error) {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  subscribe(listeners, listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  }

  notify(listeners, value) {
    for (const listener of listeners) {
      listener(value);
    }
  }
}
