import assert from "node:assert/strict";
import test from "node:test";

import { MediaSession } from "../../static/media-session.mjs";

function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function mediaHarness(options = {}) {
  const sent = [];
  const latencies = [];
  const speakingStates = [];
  const track = {
    enabled: true,
    stopped: false,
    stop() {
      this.stopped = true;
    },
  };
  const stream = {
    getAudioTracks() {
      return [track];
    },
    getTracks() {
      return [track];
    },
  };
  const audioNodes = [];
  const gainNodes = [];
  const destinationTrack = {
    enabled: true,
    stopped: false,
    stop() {
      this.stopped = true;
    },
  };
  const destinationStream = {
    getAudioTracks() {
      return [destinationTrack];
    },
    getTracks() {
      return [destinationTrack];
    },
  };
  const displayTrack = {
    kind: "video",
    stopped: false,
    listeners: {},
    getSettings() {
      return options.displaySettings ?? { width: 1920, height: 1080, frameRate: 30 };
    },
    addEventListener(type, listener) {
      this.listeners[type] = listener;
    },
    stop() {
      this.stopped = true;
    },
    emitEnded() {
      this.listeners.ended?.();
    },
  };
  const displayStream = {
    id: "display-stream",
    getAudioTracks() {
      return [];
    },
    getVideoTracks() {
      return [displayTrack];
    },
    getTracks() {
      return [displayTrack];
    },
  };
  class FakeAudioContext {
    constructor() {
      this.closed = false;
      FakeAudioContext.instances.push(this);
    }

    createMediaStreamSource(sourceStream) {
      return {
        stream: sourceStream,
        connectedTo: null,
        connect(target) {
          this.connectedTo = target;
          return target;
        },
        disconnect() {
          this.connectedTo = null;
        },
      };
    }

    createGain() {
      const node = {
        gain: { value: 1 },
        connectedTo: null,
        connect(target) {
          this.connectedTo = target;
          return target;
        },
        disconnect() {
          this.connectedTo = null;
        },
      };
      gainNodes.push(node);
      return node;
    }

    createMediaStreamDestination() {
      return { stream: destinationStream };
    }

    async close() {
      this.closed = true;
    }
  }
  FakeAudioContext.instances = [];
  class FakeAudio {
    constructor() {
      this.autoplay = false;
      this.srcObject = null;
      this.volume = 1;
      this.removed = false;
      audioNodes.push(this);
    }

    async play() {}

    remove() {
      this.removed = true;
    }
  }
  class FakeMediaStream {
    constructor(tracks = []) {
      this.id = "generated-screen-stream";
      this.tracks = tracks;
    }

    getVideoTracks() {
      return this.tracks.filter((streamTrack) => streamTrack.kind === "video");
    }

    getAudioTracks() {
      return this.tracks.filter((streamTrack) => streamTrack.kind === "audio");
    }
  }
  class FakePeerConnection {
    constructor() {
      this.addedTracks = [];
      this.transceivers = [];
      this.candidates = [];
      this.localDescriptions = [];
      this.remoteDescriptions = [];
      this.offerCount = 0;
      this.closed = false;
      this.connectionState = "new";
      this.senders = [];
      this.removedSender = null;
      FakePeerConnection.instances.push(this);
    }

    addEventListener(type, listener) {
      this[`on_${type}`] = listener;
    }

    addTrack(addedTrack, addedStream) {
      this.addedTracks.push([addedTrack, addedStream]);
      const sender = {
        track: addedTrack,
        parameters: {},
        async setParameters(parameters) {
          this.parameters = parameters;
        },
        getParameters() {
          return this.parameters;
        },
        async replaceTrack(nextTrack) {
          this.track = nextTrack;
        },
      };
      this.senders.push(sender);
      return sender;
    }

    removeTrack(sender) {
      this.removedSender = sender;
    }

    addTransceiver(kind, options) {
      this.transceivers.push([kind, options]);
    }

    async addIceCandidate(candidate) {
      this.candidates.push(candidate);
    }

    async close() {
      this.closed = true;
    }

    async createOffer() {
      this.offerCount += 1;
      await options.offerGate?.promise;
      return { type: "offer", sdp: `offer-${this.offerCount}` };
    }

    async setLocalDescription(description) {
      this.localDescription = description;
      this.localDescriptions.push(description);
    }

    async setRemoteDescription(description) {
      this.remoteDescriptions.push(description);
    }

    emitIce(candidate) {
      this.on_icecandidate?.({ candidate });
    }

    async getStats() {
      return options.stats ?? new Map();
    }

    emitTrack(remoteStream, remoteTrack = { id: "remote-track" }) {
      this.on_track?.({
        streams: remoteStream === undefined ? [] : [remoteStream],
        track: remoteTrack,
      });
    }

    emitState(state) {
      this.connectionState = state;
      this.on_connectionstatechange?.({});
    }
  }
  FakePeerConnection.instances = [];

  const client = {
    async request(signal) {
      sent.push(signal);
      if (options.requestError) {
        throw options.requestError;
      }
      if (signal.type === "webrtc_offer") {
        return {
          type: "webrtc_answer",
          sdp: `answer-for-${signal.sdp}`,
        };
      }

      return {};
    },
  };
  const states = [];
  const errors = [];
  const screenStreams = [];
  const screenStops = [];
  let displayConstraints = null;
  const session = new MediaSession(client, {
    screenShare: options.screenShare,
    mediaDevices: {
      async getUserMedia(constraints) {
        assert.deepEqual(constraints, { audio: true });
        return stream;
      },
      async getDisplayMedia(constraints) {
        displayConstraints = constraints;
        assert.deepEqual(constraints, options.expectedDisplayConstraints ?? {
          video: {
            width: { max: 1280 },
            height: { max: 720 },
            frameRate: { max: 12 },
          },
          audio: false,
        });
        return displayStream;
      },
    },
    PeerConnectionImpl: FakePeerConnection,
    SessionDescriptionImpl: (description) => description,
    IceCandidateImpl: (candidate) => candidate,
    MediaStreamImpl: FakeMediaStream,
    createAudioElement: () => new FakeAudio(),
    AudioContextImpl: options.AudioContextImpl === undefined ? FakeAudioContext : options.AudioContextImpl,
    setIntervalImpl: options.setIntervalImpl,
    clearIntervalImpl: options.clearIntervalImpl,
    onState(state) {
      states.push(state);
    },
    onLatency(latency) {
      latencies.push(latency);
    },
    onSpeaking(speaking) {
      speakingStates.push(speaking);
    },
    onError(error) {
      errors.push(error);
    },
    onScreenStream(stream, memberId) {
      screenStreams.push({ stream, memberId });
    },
    onScreenShareEnded() {
      screenStops.push("ended");
    },
  });

  return {
    audioNodes,
    destinationTrack,
    displayStream,
    displayTrack,
    get displayConstraints() {
      return displayConstraints;
    },
    client,
    errors,
    gainNodes,
    audioContexts: FakeAudioContext.instances,
    latencies,
    peerConnections: FakePeerConnection.instances,
    sent,
    session,
    screenStreams,
    screenStops,
    speakingStates,
    states,
    track,
  };
}

test("media session starts microphone and applies an answer", async () => {
  const harness = mediaHarness();

  await harness.session.start();

  const peerConnection = harness.peerConnections[0];
  assert.equal(peerConnection.addedTracks.length, 1);
  assert.deepEqual(harness.sent[0], {
    type: "webrtc_offer",
    sdp: "offer-1",
  });
  assert.deepEqual(peerConnection.remoteDescriptions, [
    {
      type: "answer",
      sdp: "answer-for-offer-1",
    },
  ]);
  assert.equal(harness.states.some((state) => state.device === "authorized"), true);
});

test("media session does not report microphone denied when negotiation fails after permission", async () => {
  const harness = mediaHarness({
    requestError: new Error("offer failed"),
  });

  await assert.rejects(() => harness.session.start(), /offer failed/);

  assert.equal(harness.states.some((state) => state.device === "authorized"), true);
  assert.equal(harness.states.some((state) => state.device === "denied"), false);
  assert.deepEqual(harness.states.at(-1), { media: "failed" });
});

test("media session reserves remote audio slots for multi-member rooms", async () => {
  const harness = mediaHarness();

  await harness.session.start();

  assert.deepEqual(harness.peerConnections[0].transceivers, [
    ["audio", { direction: "recvonly" }],
    ["audio", { direction: "recvonly" }],
    ["audio", { direction: "recvonly" }],
    ["audio", { direction: "recvonly" }],
    ["audio", { direction: "recvonly" }],
    ["audio", { direction: "recvonly" }],
    ["video", { direction: "recvonly" }],
  ]);
});

test("media session forwards local and remote ICE", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  peerConnection.emitIce({
    toJSON() {
      return { candidate: "candidate:browser" };
    },
  });
  await harness.session.addRemoteIceCandidate({ candidate: "candidate:server" });

  assert.deepEqual(harness.sent.at(-1), {
    type: "ice_candidate",
    candidate: { candidate: "candidate:browser" },
  });
  assert.deepEqual(peerConnection.candidates, [{ candidate: "candidate:server" }]);
});

test("media session serializes renegotiation and toggles local mute", async () => {
  const offerGate = deferred();
  const harness = mediaHarness({ offerGate });
  const start = harness.session.start();
  offerGate.resolve();
  await start;
  const peerConnection = harness.peerConnections[0];

  const first = harness.session.renegotiate();
  const second = harness.session.renegotiate();
  await Promise.all([first, second]);

  harness.session.setMuted(true);
  assert.equal(peerConnection.offerCount, 3);
  assert.equal(peerConnection.addedTracks[0][0].enabled, false);
});

test("renegotiation restores connected state when the peer connection stays connected", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];
  peerConnection.connectionState = "connected";

  await harness.session.renegotiate();

  assert.deepEqual(harness.states.at(-1), { media: "connected" });
});

test("media session plays tracks and releases resources on close", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  peerConnection.emitTrack({ id: "remote-1" });
  assert.equal(harness.audioNodes.length, 1);
  assert.equal(harness.audioNodes[0].autoplay, true);

  await harness.session.close();

  assert.equal(harness.track.stopped, true);
  assert.equal(peerConnection.closed, true);
  assert.equal(harness.audioNodes[0].removed, true);
});

test("media session plays audio track even when its stream also has a video slot", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  peerConnection.emitTrack(
    {
      id: "remote-mixed-stream",
      getVideoTracks() {
        return [{ kind: "video" }];
      },
    },
    { id: "m_member:audio-track", kind: "audio" },
  );
  await Promise.resolve();

  assert.equal(harness.audioNodes.length, 1);
  assert.equal(harness.screenStreams.length, 0);
  assert.equal(harness.states.some((state) => state.downlink === "track"), true);
});

test("media session applies and updates per-member playback volume", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  harness.session.setMemberVolume("m_member", 0.35);
  peerConnection.emitTrack({ id: "remote-1" }, { id: "m_member:audio-track" });
  assert.equal(harness.audioNodes[0].volume, 0.35);

  harness.session.setMemberVolume("m_member", 1.5);
  assert.equal(harness.audioNodes[0].volume, 1);
});

test("media session sends microphone through Web Audio gain and updates gain", async () => {
  const harness = mediaHarness();
  harness.session.setMicrophoneGain(1.4);

  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  assert.equal(peerConnection.addedTracks[0][0], harness.destinationTrack);
  assert.equal(harness.gainNodes[0].gain.value, 1.4);

  harness.session.setMicrophoneGain(0.25);
  assert.equal(harness.gainNodes[0].gain.value, 0.25);

  harness.session.setMuted(true);
  assert.equal(harness.destinationTrack.enabled, false);
});

test("media session falls back to original microphone when Web Audio is unavailable", async () => {
  const harness = mediaHarness({ AudioContextImpl: null });

  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  assert.equal(peerConnection.addedTracks[0][0], harness.track);
  assert.equal(harness.session.microphoneGainSupported, false);
});

test("media session samples server latency and remote member total latency", async () => {
  const harness = mediaHarness({
    stats: new Map([
      [
        "transport-1",
        {
          type: "transport",
          selectedCandidatePairId: "candidate-pair-1",
        },
      ],
      [
        "candidate-pair-1",
        {
          type: "candidate-pair",
          state: "succeeded",
          currentRoundTripTime: 0.0184,
        },
      ],
      [
        "inbound-rtp-1",
        {
          type: "inbound-rtp",
          kind: "audio",
          trackIdentifier: "m_member:audio-track",
          jitterBufferDelay: 0.078,
          jitterBufferEmittedCount: 3,
        },
      ],
    ]),
  });
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];
  peerConnection.emitTrack({ id: "remote-stream" }, { id: "m_member:audio-track" });

  await harness.session.sampleLatencyStats();

  assert.deepEqual(harness.latencies, [
    {
      serverMs: 18.4,
      members: {
        m_member: {
          receiveMs: 26,
        },
      },
    },
  ]);
});

test("media session falls back to usable candidate pair round trip stats", async () => {
  const harness = mediaHarness({
    stats: new Map([
      [
        "candidate-pair-1",
        {
          type: "candidate-pair",
          state: "succeeded",
          totalRoundTripTime: 0.12,
          responsesReceived: 4,
        },
      ],
    ]),
  });
  await harness.session.start();

  await harness.session.sampleLatencyStats();

  assert.equal(harness.latencies.at(-1).serverMs, 30);
});

test("media session reports speaking only from microphone audio level and clears on mute", async () => {
  const harness = mediaHarness({
    stats: new Map([
      [
        "media-source-1",
        {
          type: "media-source",
          kind: "audio",
          audioLevel: 0.08,
        },
      ],
    ]),
  });
  await harness.session.start();

  await harness.session.sampleSpeakingStats();
  harness.session.setMuted(true);

  assert.deepEqual(harness.speakingStates, [true, false]);
});

test("media session starts bandwidth-limited screen share without system audio", async () => {
  const harness = mediaHarness({
    displaySettings: { width: 1920, height: 1080, frameRate: 30 },
  });
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  await harness.session.startScreenShare();

  assert.equal(peerConnection.addedTracks.at(-1)[0], harness.displayTrack);
  assert.equal(peerConnection.addedTracks.at(-1)[1], harness.displayStream);
  assert.equal(peerConnection.offerCount, 2);
  assert.deepEqual(peerConnection.senders.at(-1).parameters, {
    encodings: [{ maxBitrate: 2_000_000 }],
  });
});

test("media session updates screen share bitrate from viewer count", async () => {
  const harness = mediaHarness();
  await harness.session.start();

  await harness.session.setScreenShareViewerCount(2);
  await harness.session.startScreenShare();
  const sender = harness.peerConnections[0].senders.at(-1);

  assert.deepEqual(sender.parameters, {
    encodings: [{ maxBitrate: 1_200_000 }],
  });

  await harness.session.setScreenShareViewerCount(3);

  assert.deepEqual(sender.parameters, {
    encodings: [{ maxBitrate: 800_000 }],
  });
});

test("media session uses backend screen share config", async () => {
  const harness = mediaHarness({
    screenShare: {
      max_width: 1024,
      max_height: 576,
      max_frame_rate: 10,
      bitrate_rules: [
        { max_viewers: 1, max_bitrate_bps: 1_500_000 },
        { max_viewers: 4, max_bitrate_bps: 600_000 },
      ],
    },
    expectedDisplayConstraints: {
      video: {
        width: { max: 1024 },
        height: { max: 576 },
        frameRate: { max: 10 },
      },
      audio: false,
    },
  });
  await harness.session.start();

  await harness.session.setScreenShareViewerCount(3);
  await harness.session.startScreenShare();

  assert.deepEqual(harness.displayConstraints, {
    video: {
      width: { max: 1024 },
      height: { max: 576 },
      frameRate: { max: 10 },
    },
    audio: false,
  });
  assert.deepEqual(harness.peerConnections[0].senders.at(-1).parameters, {
    encodings: [{ maxBitrate: 600_000 }],
  });
});

test("media session stops display tracks and renegotiates", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];

  await harness.session.startScreenShare();
  const sender = peerConnection.senders.at(-1);
  await harness.session.stopScreenShare();

  assert.equal(harness.displayTrack.stopped, true);
  assert.equal(peerConnection.removedSender, sender);
  assert.equal(peerConnection.offerCount, 3);
});

test("display track ended reports screen share stopped", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  await harness.session.startScreenShare();

  harness.displayTrack.emitEnded();

  assert.deepEqual(harness.screenStops, ["ended"]);
});

test("remote video track is reported without creating audio playback", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];
  const videoTrack = { id: "m_member:screen-track", kind: "video" };
  const audioTrack = { id: "m_member:audio-track", kind: "audio" };
  const screenStream = {
    id: "screen-stream",
    getVideoTracks() {
      return [videoTrack];
    },
    getAudioTracks() {
      return [audioTrack];
    },
  };

  peerConnection.emitTrack(screenStream, videoTrack);

  assert.deepEqual(harness.screenStreams[0].stream.getVideoTracks(), [videoTrack]);
  assert.deepEqual(harness.screenStreams[0].stream.getAudioTracks(), []);
  assert.equal(harness.screenStreams[0].memberId, "m_member");
  assert.equal(harness.audioNodes.length, 0);
});

test("remote video track without stream creates a stream for screen sharing", async () => {
  const harness = mediaHarness();
  await harness.session.start();
  const peerConnection = harness.peerConnections[0];
  const remoteTrack = { id: "m_member:screen-track", kind: "video" };

  peerConnection.emitTrack(undefined, remoteTrack);

  assert.deepEqual(harness.screenStreams[0].stream.getVideoTracks(), [remoteTrack]);
  assert.equal(harness.screenStreams[0].memberId, "m_member");
  assert.equal(harness.audioNodes.length, 0);
});

test("media session calls timer functions with the global context", async () => {
  const timers = [];
  const harness = mediaHarness({
    setIntervalImpl(callback, intervalMs) {
      assert.equal(this, globalThis);
      timers.push({ callback, intervalMs });
      return timers.length;
    },
    clearIntervalImpl(timer) {
      assert.equal(this, globalThis);
      timers.push({ cleared: timer });
    },
  });

  await harness.session.start();
  await harness.session.close();

  assert.equal(timers.some((timer) => timer.intervalMs === 1500), true);
  assert.equal(timers.some((timer) => timer.intervalMs === 250), true);
  assert.equal(timers.some((timer) => timer.cleared === 1), true);
  assert.equal(timers.some((timer) => timer.cleared === 2), true);
});
