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
const DEFAULT_LATENCY_INTERVAL_MS = 1500;
const DEFAULT_SPEAKING_INTERVAL_MS = 250;
const SPEAKING_AUDIO_LEVEL = 0.035;
const DEFAULT_VOLUME = 1;

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
    this.AudioContextImpl = options.AudioContextImpl ?? globalThis.AudioContext ?? globalThis.webkitAudioContext;
    this.createAudioElement = options.createAudioElement ?? browserAudioElement;
    this.audioHost = options.audioHost ?? null;
    this.onState = options.onState;
    this.onLatency = options.onLatency;
    this.onSpeaking = options.onSpeaking;
    this.onError = options.onError;
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
    this.latencyTimer = null;
    this.speakingTimer = null;
    this.lastSpeaking = false;
    this.negotiation = Promise.resolve();
  }

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

    for (const track of this.outboundStream.getAudioTracks()) {
      this.peerConnection.addTrack(track, this.outboundStream);
    }
    for (let index = 0; index < EXTRA_REMOTE_AUDIO_SLOTS; index += 1) {
      this.peerConnection.addTransceiver("audio", { direction: "recvonly" });
    }

    try {
      await this.negotiate();
      this.startLatencySampling();
      this.startSpeakingSampling();
    } catch (error) {
      statePatch(this.onState, { media: "failed" });
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

  setMuted(muted) {
    for (const track of this.outboundStream?.getAudioTracks() ?? this.localStream?.getAudioTracks() ?? []) {
      track.enabled = !muted;
    }
    if (muted) {
      this.reportSpeaking(false);
    }
  }

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

  setMicrophoneGain(gain) {
    this.microphoneGain = clampMicrophoneGain(gain);
    if (this.microphoneGainNode) {
      this.microphoneGainNode.gain.value = this.microphoneGain;
    }
  }

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

  async close() {
    this.stopLatencySampling();
    this.stopSpeakingSampling();
    this.reportSpeaking(false);
    for (const track of this.localStream?.getTracks() ?? []) {
      track.stop();
    }
    await this.releaseAudioGraph();
    for (const entry of this.audioNodes.values()) {
      entry.audio.remove();
    }
    this.audioNodes.clear();

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
      const memberId = memberIdFromTrackId(event.track?.id);
      if (memberId) {
        this.remoteTrackMembers.set(event.track.id, memberId);
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
