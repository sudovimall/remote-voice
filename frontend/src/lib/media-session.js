function browserSessionDescription(description) {
  return new RTCSessionDescription(description);
}

function browserIceCandidate(candidate) {
  return new RTCIceCandidate(candidate);
}

function browserAudioElement() {
  const audio = document.createElement("audio");
  audio.hidden = true;
  document.body.append(audio);
  return audio;
}

function statePatch(callback, patch) {
  callback?.(patch);
}

function serviceCandidate(candidate) {
  if (!candidate) {
    return null;
  }

  return typeof candidate.toJSON === "function" ? candidate.toJSON() : candidate;
}

// The first sendrecv audio m-line carries one remote track; reserve the rest for a full 8-member room.
const EXTRA_REMOTE_AUDIO_SLOTS = 6;
const REMOTE_CAMERA_VIDEO_SLOTS = 7;
const DEFAULT_LATENCY_INTERVAL_MS = 1500;
const DEFAULT_SPEAKING_INTERVAL_MS = 250;
const SPEAKING_AUDIO_LEVEL = 0.035;
const DEFAULT_VOLUME = 1;
const DEFAULT_SCREEN_SHARE_CONFIG = {
  maxWidth: 1280,
  maxHeight: 720,
  maxFrameRate: 12,
  bitrateRules: [
    { maxViewers: 1, maxBitrateBps: 2_000_000 },
    { maxViewers: 2, maxBitrateBps: 1_200_000 },
    { maxViewers: Number.POSITIVE_INFINITY, maxBitrateBps: 800_000 },
  ],
};
const DEFAULT_VIDEO_CALL_CONFIG = {
  maxWidth: 640,
  maxHeight: 360,
  maxFrameRate: 15,
  bitrateRules: [
    { maxPublishers: 1, maxBitrateBps: 800_000 },
    { maxPublishers: 4, maxBitrateBps: 500_000 },
    { maxPublishers: Number.POSITIVE_INFINITY, maxBitrateBps: 300_000 },
  ],
};

function clamp(value, max) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return DEFAULT_VOLUME;
  }

  return Math.min(max, Math.max(0, numeric));
}

function clampPlaybackVolume(value) {
  return clamp(value, 1);
}

function clampMicrophoneGain(value) {
  return clamp(value, 2);
}

function roundedStatMs(value) {
  if (!Number.isFinite(value)) {
    return null;
  }

  return Number(value.toFixed(1));
}

function memberIdFromTrackId(trackId = "") {
  const separator = trackId.indexOf(":");
  if (separator <= 0) {
    return "";
  }

  return trackId.slice(0, separator);
}

function positiveInteger(value, fallback) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) {
    return fallback;
  }

  return Math.floor(numeric);
}

function configValue(source, snakeName, camelName, fallback) {
  return positiveInteger(source?.[snakeName] ?? source?.[camelName], fallback);
}

function normalizeScreenShareConfig(config = {}) {
  const rules = Array.isArray(config?.bitrate_rules)
    ? config.bitrate_rules
    : Array.isArray(config?.bitrateRules)
      ? config.bitrateRules
      : [];
  const bitrateRules = rules
    .map((rule) => ({
      maxViewers: positiveInteger(rule?.max_viewers ?? rule?.maxViewers, 0),
      maxBitrateBps: positiveInteger(rule?.max_bitrate_bps ?? rule?.maxBitrateBps, 0),
    }))
    .filter((rule) => rule.maxViewers > 0 && rule.maxBitrateBps > 0)
    .sort((left, right) => left.maxViewers - right.maxViewers);

  return {
    maxWidth: configValue(config, "max_width", "maxWidth", DEFAULT_SCREEN_SHARE_CONFIG.maxWidth),
    maxHeight: configValue(config, "max_height", "maxHeight", DEFAULT_SCREEN_SHARE_CONFIG.maxHeight),
    maxFrameRate: configValue(
      config,
      "max_frame_rate",
      "maxFrameRate",
      DEFAULT_SCREEN_SHARE_CONFIG.maxFrameRate,
    ),
    bitrateRules: bitrateRules.length ? bitrateRules : DEFAULT_SCREEN_SHARE_CONFIG.bitrateRules,
  };
}

function normalizeVideoCallConfig(config = {}) {
  const rules = Array.isArray(config?.bitrate_rules)
    ? config.bitrate_rules
    : Array.isArray(config?.bitrateRules)
      ? config.bitrateRules
      : [];
  const bitrateRules = rules
    .map((rule) => ({
      maxPublishers: positiveInteger(rule?.max_publishers ?? rule?.maxPublishers, 0),
      maxBitrateBps: positiveInteger(rule?.max_bitrate_bps ?? rule?.maxBitrateBps, 0),
    }))
    .filter((rule) => rule.maxPublishers > 0 && rule.maxBitrateBps > 0)
    .sort((left, right) => left.maxPublishers - right.maxPublishers);

  return {
    maxWidth: configValue(config, "max_width", "maxWidth", DEFAULT_VIDEO_CALL_CONFIG.maxWidth),
    maxHeight: configValue(config, "max_height", "maxHeight", DEFAULT_VIDEO_CALL_CONFIG.maxHeight),
    maxFrameRate: configValue(
      config,
      "max_frame_rate",
      "maxFrameRate",
      DEFAULT_VIDEO_CALL_CONFIG.maxFrameRate,
    ),
    bitrateRules: bitrateRules.length ? bitrateRules : DEFAULT_VIDEO_CALL_CONFIG.bitrateRules,
  };
}

function screenShareVideoConstraints(config) {
  return {
    width: { max: config.maxWidth },
    height: { max: config.maxHeight },
    frameRate: { max: config.maxFrameRate },
  };
}

function cameraVideoConstraints(config) {
  return {
    width: { max: config.maxWidth },
    height: { max: config.maxHeight },
    frameRate: { max: config.maxFrameRate },
  };
}

function normalizedScreenShareViewerCount(viewerCount) {
  const numeric = Number(viewerCount);
  if (!Number.isFinite(numeric)) {
    return 1;
  }

  return Math.max(1, Math.floor(numeric));
}

function screenShareBitrate(viewerCount = 1, config = DEFAULT_SCREEN_SHARE_CONFIG) {
  const viewers = normalizedScreenShareViewerCount(viewerCount);
  return config.bitrateRules.find((rule) => viewers <= rule.maxViewers)?.maxBitrateBps
    ?? config.bitrateRules.at(-1)?.maxBitrateBps
    ?? DEFAULT_SCREEN_SHARE_CONFIG.bitrateRules.at(-1).maxBitrateBps;
}

function normalizedVideoCallPublisherCount(publisherCount) {
  const numeric = Number(publisherCount);
  if (!Number.isFinite(numeric)) {
    return 1;
  }

  return Math.max(1, Math.floor(numeric));
}

function videoCallBitrate(publisherCount = 1, config = DEFAULT_VIDEO_CALL_CONFIG) {
  const publishers = normalizedVideoCallPublisherCount(publisherCount);
  return config.bitrateRules.find((rule) => publishers <= rule.maxPublishers)?.maxBitrateBps
    ?? config.bitrateRules.at(-1)?.maxBitrateBps
    ?? DEFAULT_VIDEO_CALL_CONFIG.bitrateRules.at(-1).maxBitrateBps;
}

function videoOnlyStream(event, MediaStreamImpl) {
  const videoTracks = event.track?.kind === "video"
    ? [event.track]
    : (event.streams?.[0]?.getVideoTracks?.() ?? []);
  if (MediaStreamImpl && videoTracks.length > 0) {
    return new MediaStreamImpl(videoTracks);
  }

  return event.streams?.[0] ?? null;
}

function remoteTrackInfo(trackId = "") {
  const parts = trackId.split(":");
  if (parts.length < 2) {
    return { memberId: "", source: "" };
  }
  const [memberId = "", source = ""] = parts;
  return { memberId, source };
}

function selectedCandidatePair(stats) {
  for (const report of stats.values()) {
    if (report.type !== "transport" || !report.selectedCandidatePairId) {
      continue;
    }

    const pair = stats.get(report.selectedCandidatePairId);
    if (pair) {
      return pair;
    }
  }

  for (const report of stats.values()) {
    if (
      report.type === "candidate-pair" &&
      report.state === "succeeded" &&
      (report.selected || report.nominated || candidatePairRoundTripMs(report) !== null)
    ) {
      return report;
    }
  }

  return null;
}

function candidatePairRoundTripMs(pair) {
  if (!pair) {
    return null;
  }
  if (Number.isFinite(pair.currentRoundTripTime)) {
    return roundedStatMs(pair.currentRoundTripTime * 1000);
  }
  if (Number.isFinite(pair.totalRoundTripTime) && pair.responsesReceived > 0) {
    return roundedStatMs((pair.totalRoundTripTime / pair.responsesReceived) * 1000);
  }

  return null;
}

export class MediaSession {
  constructor(client, options = {}) {
    this.client = client;
    this.mediaDevices = options.mediaDevices ?? navigator.mediaDevices;
    this.PeerConnectionImpl = options.PeerConnectionImpl ?? RTCPeerConnection;
    this.SessionDescriptionImpl =
      options.SessionDescriptionImpl ?? browserSessionDescription;
    this.IceCandidateImpl = options.IceCandidateImpl ?? browserIceCandidate;
    this.MediaStreamImpl = options.MediaStreamImpl ?? globalThis.MediaStream;
    this.AudioContextImpl = options.AudioContextImpl ?? globalThis.AudioContext ?? globalThis.webkitAudioContext;
    this.createAudioElement = options.createAudioElement ?? browserAudioElement;
    this.audioHost = options.audioHost ?? null;
    this.onState = options.onState;
    this.onLatency = options.onLatency;
    this.onSpeaking = options.onSpeaking;
    this.onScreenStream = options.onScreenStream;
    this.onScreenShareEnded = options.onScreenShareEnded;
    this.onLocalCameraStream = options.onLocalCameraStream;
    this.onLocalMediaTrack = options.onLocalMediaTrack;
    this.onRemoteCameraStreams = options.onRemoteCameraStreams;
    this.onCameraEnded = options.onCameraEnded;
    this.onError = options.onError;
    this.screenShareConfig = normalizeScreenShareConfig(options.screenShare);
    this.videoCallConfig = normalizeVideoCallConfig(options.videoCall);
    this.initialMuted = Boolean(options.initialMuted);
    this.latencyIntervalMs = options.latencyIntervalMs ?? DEFAULT_LATENCY_INTERVAL_MS;
    this.speakingIntervalMs = options.speakingIntervalMs ?? DEFAULT_SPEAKING_INTERVAL_MS;
    this.setIntervalImpl =
      options.setIntervalImpl?.bind(globalThis) ?? globalThis.setInterval.bind(globalThis);
    this.clearIntervalImpl =
      options.clearIntervalImpl?.bind(globalThis) ?? globalThis.clearInterval.bind(globalThis);
    this.peerConnection = null;
    this.localStream = null;
    this.audioNodes = new Map();
    this.memberVolumes = new Map();
    this.remoteTrackMembers = new Map();
    this.outboundStream = null;
    this.audioContext = null;
    this.microphoneSource = null;
    this.microphoneGainNode = null;
    this.microphoneDestination = null;
    this.microphoneGain = DEFAULT_VOLUME;
    this.microphoneGainSupported = Boolean(this.AudioContextImpl);
    this.displayStream = null;
    this.displaySender = null;
    this.screenShareViewerCount = 1;
    this.stoppingScreenShare = false;
    this.cameraStream = null;
    this.cameraSender = null;
    this.cameraPublisherCount = 1;
    this.remoteCameraStreams = new Map();
    this.stoppingCamera = false;
    this.latencyTimer = null;
    this.speakingTimer = null;
    this.lastSpeaking = false;
    this.negotiation = Promise.resolve();
  }

  // 启动本地麦克风和 SFU PeerConnection，同时把本地音频轨道暴露给 P2P 管理器。
  async start() {
    statePatch(this.onState, { device: "requesting", media: "waiting" });

    try {
      this.localStream = await this.mediaDevices.getUserMedia({ audio: true });
    } catch (error) {
      statePatch(this.onState, { device: "denied", media: "failed" });
      this.onError?.(error);
      throw error;
    }

    statePatch(this.onState, { device: "authorized", media: "negotiating" });
    this.peerConnection = new this.PeerConnectionImpl();
    this.bindPeerConnection(this.peerConnection);

    this.prepareOutboundStream();
    this.setMuted(this.initialMuted);
    this.publishLocalAudioTracks();

    for (const track of this.outboundStream.getAudioTracks()) {
      this.peerConnection.addTrack(track, this.outboundStream);
    }
    for (let index = 0; index < EXTRA_REMOTE_AUDIO_SLOTS; index += 1) {
      this.peerConnection.addTransceiver("audio", { direction: "recvonly" });
    }
    this.peerConnection.addTransceiver("video", { direction: "recvonly" });
    for (let index = 0; index < REMOTE_CAMERA_VIDEO_SLOTS; index += 1) {
      this.peerConnection.addTransceiver("video", { direction: "recvonly" });
    }

    try {
      await this.negotiate();
      this.startLatencySampling();
      this.startSpeakingSampling();
    } catch (error) {
      statePatch(this.onState, { media: "failed" });
      await this.close();
      this.onError?.(error);
      throw error;
    }
  }

  async renegotiate() {
    const nextNegotiation = this.negotiation.then(() => this.negotiate());
    this.negotiation = nextNegotiation.catch(() => {});
    return nextNegotiation;
  }

  async negotiate() {
    if (!this.peerConnection) {
      return;
    }

    statePatch(this.onState, { media: "negotiating" });
    const offer = await this.peerConnection.createOffer();
    await this.peerConnection.setLocalDescription(offer);
    const answer = await this.client.request({
      type: "webrtc_offer",
      sdp: this.peerConnection.localDescription?.sdp ?? offer.sdp,
    });
    await this.peerConnection.setRemoteDescription(
      this.SessionDescriptionImpl({
        type: "answer",
        sdp: answer.sdp,
      }),
    );
    if (this.peerConnection.connectionState === "connected") {
      statePatch(this.onState, { media: "connected" });
    }
  }

  // 切换本地麦克风轨道启用状态；P2P 和 SFU 共用同一条处理后的音频轨道。
  setMuted(muted) {
    for (const track of this.outboundStream?.getAudioTracks() ?? this.localStream?.getAudioTracks() ?? []) {
      track.enabled = !muted;
    }
    if (muted) {
      this.reportSpeaking(false);
    }
  }

  // 构造可选的麦克风增益音频图，确保 SFU 和 P2P 使用同一份增益后音频。
  prepareOutboundStream() {
    this.outboundStream = this.localStream;
    this.microphoneGainSupported = false;
    if (!this.AudioContextImpl) {
      return;
    }

    try {
      this.audioContext = new this.AudioContextImpl();
      this.microphoneSource = this.audioContext.createMediaStreamSource(this.localStream);
      this.microphoneGainNode = this.audioContext.createGain();
      this.microphoneGainNode.gain.value = clampMicrophoneGain(this.microphoneGain);
      this.microphoneDestination = this.audioContext.createMediaStreamDestination();
      this.microphoneSource.connect(this.microphoneGainNode);
      this.microphoneGainNode.connect(this.microphoneDestination);
      this.outboundStream = this.microphoneDestination.stream;
      this.microphoneGainSupported = true;
    } catch (error) {
      this.releaseAudioGraph();
      this.outboundStream = this.localStream;
      this.microphoneGainSupported = false;
      this.onError?.(error);
    }
  }

  // 更新麦克风增益；如果浏览器不支持 Web Audio，则只记录偏好供后续使用。
  setMicrophoneGain(gain) {
    this.microphoneGain = clampMicrophoneGain(gain);
    if (this.microphoneGainNode) {
      this.microphoneGainNode.gain.value = this.microphoneGain;
    }
  }

  // 更新远端成员播放音量，用于 SFU 下行音频播放节点。
  setMemberVolume(memberId, volume) {
    if (!memberId) {
      return;
    }

    const nextVolume = clampPlaybackVolume(volume);
    this.memberVolumes.set(memberId, nextVolume);
    for (const entry of this.audioNodes.values()) {
      if (entry.memberId === memberId) {
        entry.audio.volume = nextVolume;
      }
    }
  }

  canShareScreen() {
    return Boolean(this.mediaDevices?.getDisplayMedia);
  }

  canUseCamera() {
    return Boolean(this.mediaDevices?.getUserMedia);
  }

  async setVideoCallPublisherCount(publisherCount) {
    const nextPublisherCount = normalizedVideoCallPublisherCount(publisherCount);
    if (nextPublisherCount === this.cameraPublisherCount) {
      return;
    }

    this.cameraPublisherCount = nextPublisherCount;
    if (this.cameraSender) {
      await this.applyCameraBitrate(this.cameraSender);
    }
  }

  // 请求摄像头并发布为独立视频源，同时通知 P2P 管理器同步本地 camera 轨道。
  async startCamera() {
    if (!this.peerConnection) {
      throw new Error("媒体会话尚未连接。");
    }
    if (!this.canUseCamera()) {
      throw new Error("当前浏览器不支持摄像头。");
    }

    await this.stopCamera({ renegotiate: false, notify: false });
    const cameraStream = await this.mediaDevices.getUserMedia({
      video: cameraVideoConstraints(this.videoCallConfig),
      audio: false,
    });
    const [cameraTrack] = cameraStream.getVideoTracks?.() ?? [];
    if (!cameraTrack) {
      for (const track of cameraStream.getTracks?.() ?? []) {
        track.stop();
      }
      throw new Error("没有可用的摄像头视频轨道。");
    }

    cameraTrack.addEventListener?.("ended", () => {
      if (this.stoppingCamera || this.cameraStream !== cameraStream) {
        return;
      }
      this.onCameraEnded?.();
    });

    this.cameraStream = cameraStream;
    this.cameraSender = this.peerConnection.addTrack(cameraTrack, cameraStream);
    this.onLocalCameraStream?.(cameraStream);
    this.publishLocalMediaTrack("camera", cameraTrack, cameraStream);
    try {
      await this.applyCameraBitrate(this.cameraSender);
      await this.renegotiate();
      return cameraStream;
    } catch (error) {
      await this.stopCamera({ renegotiate: false });
      throw error;
    }
  }

  // 停止本地摄像头发布；释放 SFU sender 后同步移除 P2P camera 轨道。
  async stopCamera(options = {}) {
    const { renegotiate = true, notify = true } = options;
    const cameraStream = this.cameraStream;
    const cameraSender = this.cameraSender;
    if (!cameraStream && !cameraSender) {
      return;
    }

    this.stoppingCamera = true;
    for (const track of cameraStream?.getTracks?.() ?? []) {
      track.stop();
    }
    if (cameraSender && this.peerConnection?.removeTrack) {
      this.peerConnection.removeTrack(cameraSender);
    } else if (cameraSender?.replaceTrack) {
      await cameraSender.replaceTrack(null);
    }
    this.cameraStream = null;
    this.cameraSender = null;
    this.publishLocalMediaTrack("camera", null, null);
    this.stoppingCamera = false;
    if (notify) {
      this.onLocalCameraStream?.(null);
    }
    if (renegotiate && this.peerConnection) {
      await this.renegotiate();
    }
  }

  async applyCameraBitrate(sender) {
    if (!sender?.setParameters) {
      return;
    }

    try {
      const maxBitrate = videoCallBitrate(this.cameraPublisherCount, this.videoCallConfig);
      const parameters = sender.getParameters?.() ?? {};
      parameters.encodings = [{ ...(parameters.encodings?.[0] ?? {}), maxBitrate }];
      await sender.setParameters(parameters);
    } catch (error) {
      this.onError?.(error);
    }
  }

  remoteCameraStreamEntries() {
    return Array.from(this.remoteCameraStreams.values());
  }

  clearRemoteCameraStream(memberId) {
    if (!memberId || !this.remoteCameraStreams.has(memberId)) {
      return;
    }
    this.remoteCameraStreams.delete(memberId);
    this.onRemoteCameraStreams?.(this.remoteCameraStreamEntries());
  }

  rememberRemoteCameraStream(memberId, stream, track) {
    if (!memberId || !stream) {
      return;
    }
    this.remoteCameraStreams.set(memberId, { memberId, stream });
    const cleanup = () => this.clearRemoteCameraStream(memberId);
    track?.addEventListener?.("ended", cleanup, { once: true });
    track?.addEventListener?.("mute", cleanup, { once: true });
    this.onRemoteCameraStreams?.(this.remoteCameraStreamEntries());
  }

  async setScreenShareViewerCount(viewerCount) {
    const nextViewerCount = normalizedScreenShareViewerCount(viewerCount);
    if (nextViewerCount === this.screenShareViewerCount) {
      return;
    }

    this.screenShareViewerCount = nextViewerCount;
    if (this.displaySender) {
      await this.applyScreenShareBitrate(this.displaySender);
    }
  }

  // 请求屏幕共享并发布为独立视频源，同时通知 P2P 管理器同步本地 screen 轨道。
  async startScreenShare() {
    if (!this.peerConnection) {
      throw new Error("媒体会话尚未连接。");
    }
    if (!this.canShareScreen()) {
      throw new Error("当前浏览器不支持屏幕共享。");
    }

    await this.stopScreenShare({ renegotiate: false, notify: false });
    const displayStream = await this.mediaDevices.getDisplayMedia({
      video: screenShareVideoConstraints(this.screenShareConfig),
      audio: false,
    });
    const [displayTrack] = displayStream.getVideoTracks?.() ?? [];
    if (!displayTrack) {
      for (const track of displayStream.getTracks?.() ?? []) {
        track.stop();
      }
      throw new Error("没有可共享的屏幕视频轨道。");
    }

    displayTrack.addEventListener?.("ended", () => {
      if (this.stoppingScreenShare || this.displayStream !== displayStream) {
        return;
      }
      this.onScreenShareEnded?.();
    });

    this.displayStream = displayStream;
    this.displaySender = this.peerConnection.addTrack(displayTrack, displayStream);
    this.publishLocalMediaTrack("screen", displayTrack, displayStream);
    await this.applyScreenShareBitrate(this.displaySender);
    await this.renegotiate();
    return displayStream;
  }

  // 停止屏幕共享；无论是否通知 UI，都要让 P2P 连接移除 screen 轨道。
  async stopScreenShare(options = {}) {
    const { renegotiate = true, notify = true } = options;
    const displayStream = this.displayStream;
    const displaySender = this.displaySender;
    if (!displayStream && !displaySender) {
      return;
    }

    this.stoppingScreenShare = true;
    for (const track of displayStream?.getTracks?.() ?? []) {
      track.stop();
    }
    if (displaySender && this.peerConnection?.removeTrack) {
      this.peerConnection.removeTrack(displaySender);
    } else if (displaySender?.replaceTrack) {
      await displaySender.replaceTrack(null);
    }
    this.displayStream = null;
    this.displaySender = null;
    this.publishLocalMediaTrack("screen", null, null);
    this.stoppingScreenShare = false;
    if (notify) {
      this.onScreenStream?.(null, "");
    }
    if (renegotiate && this.peerConnection) {
      await this.renegotiate();
    }
  }

  async applyScreenShareBitrate(sender) {
    if (!sender?.setParameters) {
      return;
    }

    try {
      const maxBitrate = screenShareBitrate(this.screenShareViewerCount, this.screenShareConfig);
      const parameters = sender.getParameters?.() ?? {};
      parameters.encodings = [{ ...(parameters.encodings?.[0] ?? {}), maxBitrate }];
      await sender.setParameters(parameters);
    } catch (error) {
      this.onError?.(error);
    }
  }

  // 接收服务端 SFU ICE candidate，P2P candidate 由独立管理器处理。
  async addRemoteIceCandidate(candidate) {
    if (!this.peerConnection || !candidate) {
      return;
    }

    try {
      await this.peerConnection.addIceCandidate(this.IceCandidateImpl(candidate));
    } catch (error) {
      this.onError?.(error);
      throw error;
    }
  }

  // 关闭 SFU 媒体会话和本地采集资源，并通知 P2P 移除本地音视频轨道。
  async close() {
    this.stopLatencySampling();
    this.stopSpeakingSampling();
    this.reportSpeaking(false);
    this.publishLocalMediaTrack("audio", null, null);
    for (const track of this.localStream?.getTracks() ?? []) {
      track.stop();
    }
    await this.stopCamera({ renegotiate: false, notify: false });
    await this.stopScreenShare({ renegotiate: false, notify: false });
    await this.releaseAudioGraph();
    for (const entry of this.audioNodes.values()) {
      entry.audio.remove();
    }
    this.audioNodes.clear();
    this.remoteCameraStreams.clear();
    this.onRemoteCameraStreams?.(this.remoteCameraStreamEntries());
    this.onLocalCameraStream?.(null);

    if (this.peerConnection) {
      await this.peerConnection.close();
    }
  }

  async releaseAudioGraph() {
    try {
      this.microphoneSource?.disconnect?.();
      this.microphoneGainNode?.disconnect?.();
    } catch (_error) {
      // Disconnect can throw if a node is already disconnected.
    }
    const audioContext = this.audioContext;
    this.microphoneSource = null;
    this.microphoneGainNode = null;
    this.microphoneDestination = null;
    this.audioContext = null;
    if (audioContext && audioContext.state !== "closed") {
      await audioContext.close?.();
    }
  }

  // 返回当前可发布到 P2P 的本地轨道快照，便于 P2P 管理器重建后补齐状态。
  localMediaTracks() {
    return [
      ...(this.outboundStream?.getAudioTracks?.() ?? []).map((track) => ({
        source: "audio",
        track,
        stream: this.outboundStream,
      })),
      ...(this.cameraStream?.getVideoTracks?.() ?? []).map((track) => ({
        source: "camera",
        track,
        stream: this.cameraStream,
      })),
      ...(this.displayStream?.getVideoTracks?.() ?? []).map((track) => ({
        source: "screen",
        track,
        stream: this.displayStream,
      })),
    ];
  }

  // 广播本地音频轨道状态，麦克风重建或关闭时 P2P 连接可同步替换 sender。
  publishLocalAudioTracks() {
    const [track] = this.outboundStream?.getAudioTracks?.() ?? [];
    this.publishLocalMediaTrack("audio", track ?? null, track ? this.outboundStream : null);
  }

  // 将本地媒体源变化交给外部管理器，避免 P2P 读取 MediaSession 私有字段。
  publishLocalMediaTrack(source, track, stream) {
    this.onLocalMediaTrack?.({ source, track, stream });
  }

  startLatencySampling() {
    if (!this.onLatency || this.latencyTimer || !this.setIntervalImpl) {
      return;
    }

    this.latencyTimer = this.setIntervalImpl(() => {
      this.sampleLatencyStats().catch((error) => this.onError?.(error));
    }, this.latencyIntervalMs);
    this.latencyTimer?.unref?.();
  }

  stopLatencySampling() {
    if (!this.latencyTimer || !this.clearIntervalImpl) {
      this.latencyTimer = null;
      return;
    }

    this.clearIntervalImpl(this.latencyTimer);
    this.latencyTimer = null;
  }

  startSpeakingSampling() {
    if (!this.onSpeaking || this.speakingTimer || !this.setIntervalImpl) {
      return;
    }

    this.speakingTimer = this.setIntervalImpl(() => {
      this.sampleSpeakingStats().catch((error) => this.onError?.(error));
    }, this.speakingIntervalMs);
    this.speakingTimer?.unref?.();
  }

  stopSpeakingSampling() {
    if (!this.speakingTimer || !this.clearIntervalImpl) {
      this.speakingTimer = null;
      return;
    }

    this.clearIntervalImpl(this.speakingTimer);
    this.speakingTimer = null;
  }

  reportSpeaking(speaking) {
    if (!this.onSpeaking || this.lastSpeaking === speaking) {
      return;
    }

    this.lastSpeaking = speaking;
    this.onSpeaking(speaking);
  }

  async sampleLatencyStats() {
    if (!this.peerConnection?.getStats || !this.onLatency) {
      return null;
    }

    const stats = await this.peerConnection.getStats();
    const pair = selectedCandidatePair(stats);
    const serverMs = candidatePairRoundTripMs(pair);
    const members = {};

    for (const report of stats.values()) {
      if (report.type !== "inbound-rtp" || report.kind !== "audio") {
        continue;
      }

      const trackId = report.trackIdentifier ?? "";
      const memberId = this.remoteTrackMembers.get(trackId) ?? memberIdFromTrackId(trackId);
      if (!memberId || !report.jitterBufferEmittedCount) {
        continue;
      }

      const receiveMs = roundedStatMs(
        (report.jitterBufferDelay / report.jitterBufferEmittedCount) * 1000,
      );
      members[memberId] = {
        receiveMs,
      };
    }

    const latency = { serverMs, members };
    this.onLatency(latency);
    return latency;
  }

  async sampleSpeakingStats() {
    if (!this.peerConnection?.getStats || !this.onSpeaking) {
      return null;
    }
    if (![...(this.localStream?.getAudioTracks() ?? [])].some((track) => track.enabled)) {
      this.reportSpeaking(false);
      return false;
    }

    const stats = await this.peerConnection.getStats();
    let audioLevel = 0;
    for (const report of stats.values()) {
      if (
        ["media-source", "track"].includes(report.type) &&
        report.kind === "audio" &&
        Number.isFinite(report.audioLevel)
      ) {
        audioLevel = Math.max(audioLevel, report.audioLevel);
      }
    }

    const speaking = audioLevel >= SPEAKING_AUDIO_LEVEL;
    this.reportSpeaking(speaking);
    return speaking;
  }

  bindPeerConnection(peerConnection) {
    peerConnection.addEventListener("icecandidate", (event) => {
      const candidate = serviceCandidate(event.candidate);
      if (!candidate) {
        return;
      }

      try {
        this.sendSignal({
          type: "ice_candidate",
          candidate,
        });
      } catch (error) {
        this.onError?.(error);
      }
    });

    peerConnection.addEventListener("track", (event) => {
      const { memberId, source } = remoteTrackInfo(event.track?.id);
      if (memberId) {
        this.remoteTrackMembers.set(event.track.id, memberId);
      }
      if (event.track?.kind === "video") {
        if (!memberId) {
          this.onError?.(new Error("无法识别远端视频发布者。"));
          return;
        }
        const stream = videoOnlyStream(event, this.MediaStreamImpl);
        if (source === "camera") {
          this.rememberRemoteCameraStream(memberId, stream, event.track);
        } else {
          this.onScreenStream?.(stream, memberId);
        }
        return;
      }
      if (event.track?.kind === "audio") {
        this.playRemoteStream(event.streams?.[0], memberId);
        return;
      }
      if ((event.streams?.[0]?.getVideoTracks?.() ?? []).length > 0) {
        if (!memberId) {
          this.onError?.(new Error("无法识别远端视频发布者。"));
          return;
        }
        const stream = videoOnlyStream(event, this.MediaStreamImpl);
        if (source === "camera") {
          this.rememberRemoteCameraStream(memberId, stream, event.track);
        } else {
          this.onScreenStream?.(stream, memberId);
        }
        return;
      }
      this.playRemoteStream(event.streams?.[0], memberId);
    });

    peerConnection.addEventListener("connectionstatechange", () => {
      const state = peerConnection.connectionState;
      if (state === "connected") {
        statePatch(this.onState, { media: "connected" });
      } else if (["failed", "disconnected", "closed"].includes(state)) {
        statePatch(this.onState, { media: "failed" });
      }
    });
  }

  sendSignal(signal) {
    if (typeof this.client.send === "function") {
      this.client.send(signal);
      return;
    }

    void this.client.request(signal);
  }

  async playRemoteStream(stream, memberId = "") {
    if (!stream) {
      return;
    }

    const key = stream.id ?? stream;
    const existing = this.audioNodes.get(key);
    const audio = existing?.audio ?? this.createAudioElement();
    if (!existing) {
      audio.autoplay = true;
      audio.srcObject = stream;
      audio.volume = clampPlaybackVolume(this.memberVolumes.get(memberId) ?? DEFAULT_VOLUME);
      this.audioHost?.append(audio);
      this.audioNodes.set(key, { audio, memberId });
    } else if (memberId && existing.memberId !== memberId) {
      existing.memberId = memberId;
      audio.volume = clampPlaybackVolume(this.memberVolumes.get(memberId) ?? DEFAULT_VOLUME);
    }

    try {
      await audio.play?.();
      statePatch(this.onState, { downlink: "track" });
    } catch (error) {
      statePatch(this.onState, { downlink: "playback_failed" });
      this.onError?.(error);
    }
  }
}
