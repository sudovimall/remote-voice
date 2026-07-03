// 将浏览器原生会话描述包装为可注入函数，便于前端单元测试替换。
function browserSessionDescription(description) {
  return new RTCSessionDescription(description);
}

// 将浏览器 ICE candidate 包装为可注入函数，P2P 和 SFU 测试可以使用同一形状。
function browserIceCandidate(candidate) {
  return new RTCIceCandidate(candidate);
}

// 创建隐藏音频节点播放远端 P2P 音频，保持和 SFU 播放行为一致。
function browserAudioElement() {
  const audio = document.createElement("audio");
  audio.hidden = true;
  document.body.append(audio);
  return audio;
}

// 转换浏览器 candidate 为可通过 WebSocket 传输的普通对象。
function serviceCandidate(candidate) {
  if (!candidate) {
    return null;
  }

  return typeof candidate.toJSON === "function" ? candidate.toJSON() : candidate;
}

// 生成 P2P fire-and-forget 信令请求号，方便服务端错误能关联到具体操作。
function requestId(type) {
  if (globalThis.crypto?.randomUUID) {
    return `${type}-${globalThis.crypto.randomUUID()}`;
  }

  return `${type}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

// 从后端规范化成员对里找出当前客户端的对端成员。
function memberPairMember(memberIds = [], ownMemberId = "") {
  if (!Array.isArray(memberIds) || !memberIds.includes(ownMemberId)) {
    return "";
  }

  return memberIds.find((memberId) => memberId && memberId !== ownMemberId) ?? "";
}

// 只保留远端视频轨道，避免屏幕共享或摄像头视频误带音频播放。
function videoOnlyStream(event, MediaStreamImpl) {
  const videoTracks = event.track?.kind === "video"
    ? [event.track]
    : (event.streams?.[0]?.getVideoTracks?.() ?? []);
  if (MediaStreamImpl && videoTracks.length > 0) {
    return new MediaStreamImpl(videoTracks);
  }

  return event.streams?.[0] ?? null;
}

// 兼容测试和 SFU 风格 track id，元数据未到达时可先识别部分视频来源。
function sourceFromTrackId(trackId = "") {
  if (trackId.includes(":camera")) {
    return "camera";
  }
  if (trackId.includes(":screen")) {
    return "screen";
  }

  return "";
}

// 限制远端播放音量，避免异常偏好值放大或静音之外的状态。
function clampPlaybackVolume(value) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return 1;
  }

  return Math.min(1, Math.max(0, numeric));
}

// 生成本地 track id 到媒体来源的映射，通过 DataChannel 交给对端区分摄像头和屏幕。
function metadataMessage(localSources) {
  return JSON.stringify({
    type: "media_metadata",
    tracks: Array.from(localSources.entries())
      .filter(([, entry]) => entry.track?.id)
      .map(([source, entry]) => ({
        track_id: entry.track.id,
        source,
      })),
  });
}

const METADATA_CHANNEL_LABEL = "remote_voice_media_metadata";
const VIDEO_SOURCES = new Set(["camera", "screen"]);

export class P2PMediaSession {
  // 创建 P2P 媒体管理器，按成员维护 PeerConnection 并复用现有房间信令客户端。
  constructor(client, ownMemberId, options = {}) {
    this.client = client;
    this.ownMemberId = ownMemberId;
    this.PeerConnectionImpl = options.PeerConnectionImpl ?? RTCPeerConnection;
    this.SessionDescriptionImpl =
      options.SessionDescriptionImpl ?? browserSessionDescription;
    this.IceCandidateImpl = options.IceCandidateImpl ?? browserIceCandidate;
    this.MediaStreamImpl = options.MediaStreamImpl ?? globalThis.MediaStream;
    this.createAudioElement = options.createAudioElement ?? browserAudioElement;
    this.audioHost = options.audioHost ?? null;
    this.onScreenStream = options.onScreenStream;
    this.onRemoteCameraStreams = options.onRemoteCameraStreams;
    this.onError = options.onError;
    this.testHooks = options.testHooks ?? globalThis.__remoteVoiceP2PTest ?? null;
    this.peers = new Map();
    this.localSources = new Map();
    this.fallbackMembers = new Set();
    this.memberVolumes = new Map();
    this.notListeningMembers = new Set();
    this.remoteCameraStreams = new Map();
    this.audioNodes = new Map();
    this.testHooks?.registerSession?.(this);
    this.emitTestEvent("session_created");
  }

  // 根据房间成员列表创建或关闭 P2P 连接，保持每个在线成员对一条浏览器直连。
  syncMembers(members = []) {
    const activeMemberIds = new Set(
      members
        .filter((member) => member?.id && member.id !== this.ownMemberId && member.connected !== false)
        .map((member) => member.id),
    );

    for (const memberId of Array.from(this.peers.keys())) {
      if (!activeMemberIds.has(memberId)) {
        this.closeMember(memberId);
      }
    }

    for (const memberId of activeMemberIds) {
      if (this.fallbackMembers.has(memberId)) {
        continue;
      }
      const entry = this.ensurePeer(memberId);
      if (this.shouldInitiate(memberId) && !entry.localOfferSent) {
        this.negotiate(memberId).catch((error) => this.handleError(error));
      }
    }
  }

  // 更新本地媒体源；已有 P2P 连接会替换对应 sender 并重新协商。
  setLocalTrack(source, track, stream = null) {
    if (!source) {
      return;
    }
    if (track) {
      this.localSources.set(source, { track, stream });
    } else {
      this.localSources.delete(source);
    }

    for (const [memberId, entry] of this.peers.entries()) {
      if (this.fallbackMembers.has(memberId)) {
        continue;
      }
      this.updateLocalSender(entry, source)
        .then(() => {
          this.sendMetadata(entry);
          return this.negotiate(memberId);
        })
        .catch((error) => this.handleError(error));
    }
  }

  // 批量同步已有本地轨道，供 SFU MediaSession 启动后把当前麦克风/视频状态交给 P2P。
  setLocalTracks(entries = []) {
    for (const entry of entries) {
      this.setLocalTrack(entry.source, entry.track, entry.stream);
    }
  }

  // 处理服务端转发来的 P2P offer，并只为该发送成员生成 answer。
  async handleOffer(fromMemberId, sdp) {
    this.emitTestEvent("signal_received", {
      signal: { type: "p2p_offer", from_member_id: fromMemberId, sdp },
    });
    if (this.ignoreFallbackSignal(fromMemberId, "p2p_offer")) {
      return;
    }
    const entry = this.ensurePeer(fromMemberId);
    await entry.peerConnection.setRemoteDescription(
      this.SessionDescriptionImpl({ type: "offer", sdp }),
    );
    const answer = await entry.peerConnection.createAnswer();
    await entry.peerConnection.setLocalDescription(answer);
    this.sendSignal({
      type: "p2p_answer",
      target_member_id: fromMemberId,
      sdp: entry.peerConnection.localDescription?.sdp ?? answer.sdp,
    });
  }

  // 处理服务端转发来的 P2P answer，只应用到对应成员的 PeerConnection。
  async handleAnswer(fromMemberId, sdp) {
    this.emitTestEvent("signal_received", {
      signal: { type: "p2p_answer", from_member_id: fromMemberId, sdp },
    });
    if (this.ignoreFallbackSignal(fromMemberId, "p2p_answer")) {
      return;
    }
    const entry = this.peers.get(fromMemberId);
    if (!entry) {
      return;
    }

    await entry.peerConnection.setRemoteDescription(
      this.SessionDescriptionImpl({ type: "answer", sdp }),
    );
  }

  // 处理服务端转发来的 P2P ICE candidate，避免进入 SFU PeerConnection。
  async handleIceCandidate(fromMemberId, candidate) {
    this.emitTestEvent("signal_received", {
      signal: { type: "p2p_ice_candidate", from_member_id: fromMemberId, candidate },
    });
    if (this.ignoreFallbackSignal(fromMemberId, "p2p_ice_candidate")) {
      return;
    }
    const entry = this.ensurePeer(fromMemberId);
    await entry.peerConnection.addIceCandidate(this.IceCandidateImpl(candidate));
  }

  // 应用后端路由更新；当前成员对回退 SFU 时关闭对应 P2P 连接。
  applyMediaRouteUpdated(signal) {
    const memberId = memberPairMember(signal?.member_ids, this.ownMemberId);
    if (!memberId) {
      return;
    }

    if (signal.route === "sfu") {
      this.fallbackMembers.add(memberId);
      this.emitTestEvent("route_updated", { memberId, route: signal.route, reason: signal.reason });
      this.closeMember(memberId);
      return;
    }

    this.fallbackMembers.delete(memberId);
  }

  // 调整某个成员 P2P 音频音量，与现有成员音量偏好保持一致。
  setMemberVolume(memberId, volume) {
    const nextVolume = clampPlaybackVolume(volume);
    this.memberVolumes.set(memberId, nextVolume);
    this.applyMemberPlaybackVolume(memberId);
  }

  // 更新当前用户对某成员的 P2P 收听状态，不听时只静音播放，不丢失原始音量偏好。
  setMemberListening(memberId, listening) {
    if (!memberId) {
      return;
    }
    if (listening) {
      this.notListeningMembers.delete(memberId);
    } else {
      this.notListeningMembers.add(memberId);
    }
    this.applyMemberPlaybackVolume(memberId);
  }

  // 清理某个成员的 P2P 连接和远端媒体，用于成员离开或回退 SFU。
  closeMember(memberId) {
    const entry = this.peers.get(memberId);
    if (entry) {
      entry.closedByUs = true;
      entry.peerConnection.close?.();
      this.peers.delete(memberId);
      this.emitTestEvent("peer_closed", { memberId });
    }
    this.clearRemoteForMember(memberId);
  }

  // 清理指定成员的远端摄像头流，配合房间摄像头停止广播释放视频 tile。
  clearRemoteCameraStream(memberId) {
    if (!memberId || !this.remoteCameraStreams.has(memberId)) {
      return;
    }
    this.remoteCameraStreams.delete(memberId);
    this.onRemoteCameraStreams?.(this.remoteCameraStreamEntries());
  }

  // 关闭所有 P2P 连接和播放节点，房间离开、重连和组件卸载都会走这里。
  close() {
    for (const memberId of Array.from(this.peers.keys())) {
      this.closeMember(memberId);
    }
    for (const entry of this.audioNodes.values()) {
      entry.audio.remove?.();
    }
    this.audioNodes.clear();
    this.remoteCameraStreams.clear();
    this.onRemoteCameraStreams?.(this.remoteCameraStreamEntries());
    this.onScreenStream?.(null, "");
  }

  // 获取或创建某个成员的 P2P PeerConnection，并补齐当前已有本地轨道。
  ensurePeer(memberId) {
    const existing = this.peers.get(memberId);
    if (existing) {
      return existing;
    }

    const peerConnection = new this.PeerConnectionImpl();
    const entry = {
      peerConnection,
      senders: new Map(),
      negotiation: Promise.resolve(),
      localOfferSent: false,
      closedByUs: false,
      failedReported: false,
      metadataChannel: null,
      remoteTrackSources: new Map(),
      pendingVideoTracks: new Map(),
    };
    this.peers.set(memberId, entry);
    this.emitTestEvent("peer_created", { memberId });
    this.bindPeerConnection(memberId, entry);
    this.openMetadataChannel(entry);

    for (const source of this.localSources.keys()) {
      this.updateLocalSender(entry, source).catch((error) => this.handleError(error));
    }

    return entry;
  }

  // 对已经回退到 SFU 的成员丢弃迟到 P2P 信令，避免重新创建直连 PeerConnection。
  ignoreFallbackSignal(memberId, signalType) {
    if (!this.fallbackMembers.has(memberId)) {
      return false;
    }
    this.emitTestEvent("signal_ignored", { memberId, signalType, reason: "fallback_sfu" });
    return true;
  }

  // 绑定单条 P2P PeerConnection 的 ICE、远端 track、失败状态和元数据通道事件。
  bindPeerConnection(memberId, entry) {
    const { peerConnection } = entry;
    peerConnection.addEventListener?.("icecandidate", (event) => {
      const candidate = serviceCandidate(event.candidate);
      if (!candidate) {
        return;
      }
      this.sendSignal({
        type: "p2p_ice_candidate",
        target_member_id: memberId,
        candidate,
      });
    });

    peerConnection.addEventListener?.("track", (event) => {
      this.handleRemoteTrack(memberId, entry, event);
    });

    peerConnection.addEventListener?.("connectionstatechange", () => {
      this.handleConnectionState(memberId, entry, peerConnection.connectionState, "connection_failed");
    });

    peerConnection.addEventListener?.("iceconnectionstatechange", () => {
      this.handleConnectionState(memberId, entry, peerConnection.iceConnectionState, "ice_failed");
    });

    peerConnection.addEventListener?.("datachannel", (event) => {
      if (event.channel?.label === METADATA_CHANNEL_LABEL) {
        this.bindMetadataChannel(entry, event.channel);
      }
    });
  }

  // 主动创建元数据 DataChannel，用于告诉对端每条视频轨道对应 camera 还是 screen。
  openMetadataChannel(entry) {
    if (typeof entry.peerConnection.createDataChannel !== "function") {
      return;
    }

    this.bindMetadataChannel(
      entry,
      entry.peerConnection.createDataChannel(METADATA_CHANNEL_LABEL),
    );
  }

  // 绑定元数据通道；通道打开后立即发送当前本地轨道映射。
  bindMetadataChannel(entry, channel) {
    entry.metadataChannel = channel;
    channel.addEventListener?.("open", () => this.sendMetadata(entry));
    channel.addEventListener?.("message", (event) => {
      this.rememberRemoteMetadata(entry, event.data);
    });
    if (channel.readyState === "open") {
      this.sendMetadata(entry);
    }
  }

  // 通过 DataChannel 发送本地媒体来源映射，通道未打开时跳过等待下一次触发。
  sendMetadata(entry) {
    const channel = entry.metadataChannel;
    if (!channel || channel.readyState !== "open") {
      return;
    }

    channel.send(metadataMessage(this.localSources));
  }

  // 保存对端发来的 track 来源映射，并回放等待元数据的视频 track。
  rememberRemoteMetadata(entry, rawMessage) {
    let message;
    try {
      message = JSON.parse(rawMessage);
    } catch (_error) {
      return;
    }
    if (message?.type !== "media_metadata" || !Array.isArray(message.tracks)) {
      return;
    }

    for (const track of message.tracks) {
      if (!track?.track_id || !VIDEO_SOURCES.has(track.source)) {
        continue;
      }
      entry.remoteTrackSources.set(track.track_id, track.source);
      const pending = entry.pendingVideoTracks.get(track.track_id);
      if (pending) {
        entry.pendingVideoTracks.delete(track.track_id);
        this.rememberRemoteVideoTrack(pending.memberId, track.source, pending.event);
      }
    }
  }

  // 更新单个本地媒体源对应的 sender，支持新增、替换和移除轨道。
  async updateLocalSender(entry, source) {
    const local = this.localSources.get(source);
    const sender = entry.senders.get(source);
    if (!local?.track) {
      if (sender && entry.peerConnection.removeTrack) {
        entry.peerConnection.removeTrack(sender);
      } else if (sender?.replaceTrack) {
        await sender.replaceTrack(null);
      }
      entry.senders.delete(source);
      return;
    }

    if (sender?.replaceTrack) {
      tagLocalTrack(local.track, source);
      await sender.replaceTrack(local.track);
      return;
    }
    if (sender && entry.peerConnection.removeTrack) {
      entry.peerConnection.removeTrack(sender);
      entry.senders.delete(source);
    }

    tagLocalTrack(local.track, source);
    const nextSender = local.stream
      ? entry.peerConnection.addTrack(local.track, local.stream)
      : entry.peerConnection.addTrack(local.track);
    entry.senders.set(source, nextSender);
  }

  // 串行执行指定成员的 P2P offer 协商，避免连续轨道变化导致 offer 交错。
  negotiate(memberId) {
    const entry = this.peers.get(memberId);
    if (!entry || this.fallbackMembers.has(memberId)) {
      return Promise.resolve();
    }

    const nextNegotiation = entry.negotiation.then(async () => {
      if (entry.closedByUs || this.fallbackMembers.has(memberId)) {
        return;
      }
      const offer = await entry.peerConnection.createOffer();
      await entry.peerConnection.setLocalDescription(offer);
      entry.localOfferSent = true;
      this.sendSignal({
        type: "p2p_offer",
        target_member_id: memberId,
        sdp: entry.peerConnection.localDescription?.sdp ?? offer.sdp,
      });
      this.sendMetadata(entry);
    });
    entry.negotiation = nextNegotiation.catch(() => {});
    return nextNegotiation;
  }

  // 用成员 ID 排序决定初始 offer 发起方，降低双方同时发 offer 的概率。
  shouldInitiate(memberId) {
    return String(this.ownMemberId) < String(memberId);
  }

  // 处理远端 P2P track；音频直接播放，视频等待元数据后分配到摄像头或屏幕。
  handleRemoteTrack(memberId, entry, event) {
    const track = event.track;
    if (track?.kind === "audio") {
      this.playRemoteStream(event.streams?.[0], memberId).catch((error) => this.handleError(error));
      return;
    }
    if (track?.kind !== "video") {
      return;
    }

    const source = entry.remoteTrackSources.get(track.id) || sourceFromTrackId(track.id);
    if (!source) {
      entry.pendingVideoTracks.set(track.id, { memberId, event });
      return;
    }
    this.rememberRemoteVideoTrack(memberId, source, event);
  }

  // 记录远端视频来源，摄像头进入宫格，屏幕共享进入共享视图。
  rememberRemoteVideoTrack(memberId, source, event) {
    const stream = videoOnlyStream(event, this.MediaStreamImpl);
    if (!stream) {
      return;
    }
    if (source === "camera") {
      this.remoteCameraStreams.set(memberId, { memberId, stream });
      this.emitTestEvent("remote_video", { memberId, source });
      event.track?.addEventListener?.("ended", () => this.clearRemoteCameraStream(memberId), {
        once: true,
      });
      event.track?.addEventListener?.("mute", () => this.clearRemoteCameraStream(memberId), {
        once: true,
      });
      this.onRemoteCameraStreams?.(this.remoteCameraStreamEntries());
      return;
    }

    this.emitTestEvent("remote_video", { memberId, source });
    this.onScreenStream?.(stream, memberId);
  }

  // 返回稳定的远端摄像头数组，供 Vue 响应式状态整体替换。
  remoteCameraStreamEntries() {
    return Array.from(this.remoteCameraStreams.values());
  }

  // 播放远端 P2P 音频流，并应用当前成员音量偏好。
  async playRemoteStream(stream, memberId = "") {
    if (!stream) {
      return;
    }

    const key = stream.id ?? `${memberId}:${this.audioNodes.size}`;
    const existing = this.audioNodes.get(key);
    const audio = existing?.audio ?? this.createAudioElement();
    if (!existing) {
      audio.autoplay = true;
      audio.srcObject = stream;
      audio.volume = this.memberPlaybackVolume(memberId);
      this.audioHost?.append(audio);
      this.audioNodes.set(key, { audio, memberId });
    }

    await audio.play?.();
  }

  // 计算成员的实际播放音量，“不听”优先级高于用户保存的音量滑块。
  memberPlaybackVolume(memberId) {
    if (this.notListeningMembers.has(memberId)) {
      return 0;
    }
    return clampPlaybackVolume(this.memberVolumes.get(memberId) ?? 1);
  }

  // 将某成员当前有效音量写入已有音频节点，偏好变化无需重建媒体流。
  applyMemberPlaybackVolume(memberId) {
    for (const entry of this.audioNodes.values()) {
      if (entry.memberId === memberId) {
        entry.audio.volume = this.memberPlaybackVolume(memberId);
      }
    }
  }

  // 监听 P2P 失败状态并只上报一次，避免重复触发 SFU 回退。
  handleConnectionState(memberId, entry, state, reason) {
    if (!["failed", "disconnected", "closed"].includes(state) || entry.closedByUs) {
      return;
    }
    if (entry.failedReported) {
      return;
    }
    entry.failedReported = true;
    this.reportConnectionFailed(memberId, reason);
  }

  // 向后端报告单个成员对 P2P 失败，让服务端广播该对回退 SFU。
  reportConnectionFailed(memberId, reason) {
    this.fallbackMembers.add(memberId);
    this.emitTestEvent("fallback_reported", { memberId, reason });
    this.sendSignal({
      type: "p2p_connection_failed",
      target_member_id: memberId,
      reason,
    });
    this.closeMember(memberId);
  }

  // 清理某个对端成员产生的 P2P 音视频节点。
  clearRemoteForMember(memberId) {
    this.clearRemoteCameraStream(memberId);
    for (const [key, entry] of Array.from(this.audioNodes.entries())) {
      if (entry.memberId === memberId) {
        entry.audio.remove?.();
        this.audioNodes.delete(key);
      }
    }
  }

  // 发送 P2P 客户端信令；成功不等待 ack，失败由服务端 error 按 request_id 返回。
  sendSignal(signal) {
    const body = {
      ...signal,
      request_id: signal.request_id ?? requestId(signal.type),
    };
    if (typeof this.client?.send === "function") {
      this.client.send(body);
      this.emitTestEvent("signal_sent", { signal: body });
      return;
    }

    void this.client?.request?.(body);
    this.emitTestEvent("signal_sent", { signal: body });
  }

  // 统一上报 P2P 内部异步错误，避免未捕获 promise 影响页面运行。
  handleError(error) {
    this.onError?.(error);
  }

  // 向浏览器测试暴露 P2P 事件；正常用户环境没有 hook 时保持零副作用。
  emitTestEvent(type, payload = {}) {
    try {
      this.testHooks?.record?.({
        type,
        ownMemberId: this.ownMemberId,
        ...payload,
      });
    } catch (_error) {
      // 测试遥测不能影响真实媒体会话。
    }
  }
}

// 给真实 MediaStreamTrack 附加来源标记，浏览器测试 fake PeerConnection 可据此生成 SDP。
function tagLocalTrack(track, source) {
  try {
    Object.defineProperty(track, "__remoteVoiceP2PSource", {
      configurable: true,
      value: source,
    });
  } catch (_error) {
    // 个别浏览器对象可能不可扩展，生产逻辑仍有 DataChannel 元数据兜底。
  }
}
