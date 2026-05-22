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

export class MediaSession {
  constructor(client, options = {}) {
    this.client = client;
    this.mediaDevices = options.mediaDevices ?? navigator.mediaDevices;
    this.PeerConnectionImpl = options.PeerConnectionImpl ?? RTCPeerConnection;
    this.SessionDescriptionImpl =
      options.SessionDescriptionImpl ?? browserSessionDescription;
    this.IceCandidateImpl = options.IceCandidateImpl ?? browserIceCandidate;
    this.createAudioElement = options.createAudioElement ?? browserAudioElement;
    this.audioHost = options.audioHost ?? null;
    this.onState = options.onState;
    this.onError = options.onError;
    this.peerConnection = null;
    this.localStream = null;
    this.audioNodes = new Map();
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

    for (const track of this.localStream.getAudioTracks()) {
      this.peerConnection.addTrack(track, this.localStream);
    }

    await this.negotiate();
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
    for (const track of this.localStream?.getAudioTracks() ?? []) {
      track.enabled = !muted;
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
    for (const track of this.localStream?.getTracks() ?? []) {
      track.stop();
    }
    for (const audio of this.audioNodes.values()) {
      audio.remove();
    }
    this.audioNodes.clear();

    if (this.peerConnection) {
      await this.peerConnection.close();
    }
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
      this.playRemoteStream(event.streams?.[0]);
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

  async playRemoteStream(stream) {
    if (!stream) {
      return;
    }

    const key = stream.id ?? stream;
    const existing = this.audioNodes.get(key);
    const audio = existing ?? this.createAudioElement();
    if (!existing) {
      audio.autoplay = true;
      audio.srcObject = stream;
      this.audioHost?.append(audio);
      this.audioNodes.set(key, audio);
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
