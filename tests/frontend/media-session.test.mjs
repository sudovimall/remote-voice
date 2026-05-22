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
  class FakeAudio {
    constructor() {
      this.autoplay = false;
      this.srcObject = null;
      this.removed = false;
      audioNodes.push(this);
    }

    async play() {}

    remove() {
      this.removed = true;
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
      FakePeerConnection.instances.push(this);
    }

    addEventListener(type, listener) {
      this[`on_${type}`] = listener;
    }

    addTrack(addedTrack, addedStream) {
      this.addedTracks.push([addedTrack, addedStream]);
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

    emitTrack(remoteStream) {
      this.on_track?.({ streams: [remoteStream] });
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
  const session = new MediaSession(client, {
    mediaDevices: {
      async getUserMedia(constraints) {
        assert.deepEqual(constraints, { audio: true });
        return stream;
      },
    },
    PeerConnectionImpl: FakePeerConnection,
    SessionDescriptionImpl: (description) => description,
    IceCandidateImpl: (candidate) => candidate,
    createAudioElement: () => new FakeAudio(),
    onState(state) {
      states.push(state);
    },
    onError(error) {
      errors.push(error);
    },
  });

  return {
    audioNodes,
    client,
    errors,
    peerConnections: FakePeerConnection.instances,
    sent,
    session,
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
  assert.equal(harness.track.enabled, false);
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
