import assert from "node:assert/strict";
import test from "node:test";

import { P2PMediaSession } from "../../frontend/src/lib/p2p-media-session.js";

function flush() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function track(id, kind) {
  return {
    id,
    kind,
    listeners: {},
    addEventListener(type, listener) {
      this.listeners[type] = listener;
    },
    emit(type) {
      this.listeners[type]?.();
    },
  };
}

function stream(id, tracks = []) {
  return {
    id,
    getAudioTracks() {
      return tracks.filter((entry) => entry.kind === "audio");
    },
    getVideoTracks() {
      return tracks.filter((entry) => entry.kind === "video");
    },
    getTracks() {
      return tracks;
    },
  };
}

class FakeDataChannel {
  constructor(label) {
    this.label = label;
    this.readyState = "open";
    this.listeners = {};
    this.sent = [];
  }

  addEventListener(type, listener) {
    this.listeners[type] = listener;
  }

  send(message) {
    this.sent.push(message);
  }

  emitMessage(message) {
    this.listeners.message?.({ data: JSON.stringify(message) });
  }
}

class FakePeerConnection {
  constructor() {
    this.listeners = {};
    this.addedTracks = [];
    this.removedSenders = [];
    this.senders = [];
    this.localDescriptions = [];
    this.remoteDescriptions = [];
    this.candidates = [];
    this.dataChannels = [];
    this.offerCount = 0;
    this.answerCount = 0;
    this.closed = false;
    this.connectionState = "new";
    this.iceConnectionState = "new";
    FakePeerConnection.instances.push(this);
  }

  addEventListener(type, listener) {
    this.listeners[type] = listener;
  }

  createDataChannel(label) {
    const channel = new FakeDataChannel(label);
    this.dataChannels.push(channel);
    return channel;
  }

  addTrack(addedTrack, addedStream) {
    const sender = {
      track: addedTrack,
      stream: addedStream,
      async replaceTrack(nextTrack) {
        this.track = nextTrack;
      },
    };
    this.addedTracks.push([addedTrack, addedStream]);
    this.senders.push(sender);
    return sender;
  }

  removeTrack(sender) {
    this.removedSenders.push(sender);
  }

  async createOffer() {
    this.offerCount += 1;
    return { type: "offer", sdp: `offer-${this.offerCount}` };
  }

  async createAnswer() {
    this.answerCount += 1;
    return { type: "answer", sdp: `answer-${this.answerCount}` };
  }

  async setLocalDescription(description) {
    this.localDescription = description;
    this.localDescriptions.push(description);
  }

  async setRemoteDescription(description) {
    this.remoteDescription = description;
    this.remoteDescriptions.push(description);
  }

  async addIceCandidate(candidate) {
    this.candidates.push(candidate);
  }

  close() {
    this.closed = true;
  }

  emitIce(candidate) {
    this.listeners.icecandidate?.({ candidate });
  }

  emitTrack(event) {
    this.listeners.track?.(event);
  }

  emitConnectionState(state) {
    this.connectionState = state;
    this.listeners.connectionstatechange?.({});
  }

  emitIceConnectionState(state) {
    this.iceConnectionState = state;
    this.listeners.iceconnectionstatechange?.({});
  }
}

FakePeerConnection.instances = [];

class FakeMediaStream {
  constructor(tracks = []) {
    this.id = `generated-${tracks.map((entry) => entry.id).join("-")}`;
    this.tracks = tracks;
  }

  getVideoTracks() {
    return this.tracks.filter((entry) => entry.kind === "video");
  }

  getAudioTracks() {
    return this.tracks.filter((entry) => entry.kind === "audio");
  }
}

function p2pHarness(options = {}) {
  FakePeerConnection.instances = [];
  const sent = [];
  const errors = [];
  const screenStreams = [];
  const cameraStreams = [];
  const audioElements = [];
  class FakeAudio {
    constructor() {
      this.autoplay = false;
      this.srcObject = null;
      this.volume = 1;
      this.removed = false;
      audioElements.push(this);
    }

    async play() {}

    remove() {
      this.removed = true;
    }
  }

  const session = new P2PMediaSession(
    {
      send(signal) {
        sent.push(signal);
      },
    },
    options.ownMemberId ?? "m_a",
    {
      PeerConnectionImpl: FakePeerConnection,
      SessionDescriptionImpl: (description) => description,
      IceCandidateImpl: (candidate) => candidate,
      MediaStreamImpl: FakeMediaStream,
      createAudioElement: () => new FakeAudio(),
      onScreenStream(streamValue, memberId) {
        screenStreams.push({ stream: streamValue, memberId });
      },
      onRemoteCameraStreams(entries) {
        cameraStreams.push(entries);
      },
      onError(error) {
        errors.push(error);
      },
    },
  );

  return {
    audioElements,
    cameraStreams,
    errors,
    peerConnections: FakePeerConnection.instances,
    screenStreams,
    sent,
    session,
  };
}

test("p2p session creates an offer for another online member", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  const audioTrack = track("audio-local", "audio");
  const audioStream = stream("audio-stream", [audioTrack]);
  harness.session.setLocalTrack("audio", audioTrack, audioStream);

  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
  ]);
  await flush();

  assert.equal(harness.peerConnections.length, 1);
  assert.deepEqual(harness.peerConnections[0].addedTracks[0], [
    audioTrack,
    audioStream,
  ]);
  assert.equal(harness.sent[0].type, "p2p_offer");
  assert.equal(harness.sent[0].target_member_id, "m_b");
  assert.match(harness.sent[0].request_id, /^p2p_offer-/);
});

test("p2p session never creates a connection to the local member", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });

  harness.session.syncMembers([{ id: "m_a", connected: true }]);
  await flush();

  assert.equal(harness.peerConnections.length, 0);
  assert.equal(harness.sent.length, 0);
});

test("p2p offer creates an answer for the sender", async () => {
  const harness = p2pHarness({ ownMemberId: "m_b" });

  await harness.session.handleOffer("m_a", "offer-from-a");

  assert.deepEqual(harness.peerConnections[0].remoteDescriptions, [
    { type: "offer", sdp: "offer-from-a" },
  ]);
  assert.equal(harness.sent[0].type, "p2p_answer");
  assert.equal(harness.sent[0].target_member_id, "m_a");
  assert.equal(harness.sent[0].sdp, "answer-1");
});

test("p2p answer and ICE are applied to the matching member connection", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
    { id: "m_c", connected: true },
  ]);
  await flush();

  await harness.session.handleAnswer("m_b", "answer-from-b");
  await harness.session.handleIceCandidate("m_c", { candidate: "candidate-c" });

  assert.deepEqual(harness.peerConnections[0].remoteDescriptions.at(-1), {
    type: "answer",
    sdp: "answer-from-b",
  });
  assert.deepEqual(harness.peerConnections[1].candidates, [{ candidate: "candidate-c" }]);
});

test("p2p local ICE is sent only to the target member", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
  ]);
  await flush();

  harness.peerConnections[0].emitIce({
    toJSON() {
      return { candidate: "candidate-local" };
    },
  });

  assert.equal(harness.sent.at(-1).type, "p2p_ice_candidate");
  assert.equal(harness.sent.at(-1).target_member_id, "m_b");
  assert.deepEqual(harness.sent.at(-1).candidate, { candidate: "candidate-local" });
  assert.match(harness.sent.at(-1).request_id, /^p2p_ice_candidate-/);
});

test("p2p failed state reports fallback for one member only", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
    { id: "m_c", connected: true },
  ]);
  await flush();

  harness.peerConnections[0].emitIceConnectionState("failed");

  assert.equal(harness.sent.at(-1).type, "p2p_connection_failed");
  assert.equal(harness.sent.at(-1).target_member_id, "m_b");
  assert.equal(harness.sent.at(-1).reason, "ice_failed");
  assert.equal(harness.peerConnections[0].closed, true);
  assert.equal(harness.peerConnections[1].closed, false);
});

test("p2p route update closes only the affected member pair", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
    { id: "m_c", connected: true },
  ]);
  await flush();

  harness.session.applyMediaRouteUpdated({
    type: "media_route_updated",
    member_ids: ["m_a", "m_b"],
    route: "sfu",
    reason: "p2p_failed",
  });

  assert.equal(harness.peerConnections[0].closed, true);
  assert.equal(harness.peerConnections[1].closed, false);
});

test("p2p route fallback ignores late offer and ICE for the affected member", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
  ]);
  await flush();
  const peerConnection = harness.peerConnections[0];

  harness.session.applyMediaRouteUpdated({
    type: "media_route_updated",
    member_ids: ["m_a", "m_b"],
    route: "sfu",
    reason: "p2p_failed",
  });
  await harness.session.handleOffer("m_b", "late-offer");
  await harness.session.handleIceCandidate("m_b", { candidate: "late-candidate" });

  assert.equal(harness.peerConnections.length, 1);
  assert.equal(peerConnection.closed, true);
  assert.equal(peerConnection.remoteDescriptions.length, 0);
  assert.equal(peerConnection.candidates.length, 0);
  assert.equal(harness.sent.some((signal) => signal.type === "p2p_answer"), false);
});

test("p2p not-listening state mutes remote audio without losing member volume", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  const remoteTrack = track("audio-remote", "audio");
  const remoteStream = stream("remote-audio-stream", [remoteTrack]);

  harness.session.setMemberVolume("m_b", 0.7);
  harness.session.setMemberListening("m_b", false);
  await harness.session.playRemoteStream(remoteStream, "m_b");

  assert.equal(harness.audioElements.length, 1);
  assert.equal(harness.audioElements[0].volume, 0);

  harness.session.setMemberListening("m_b", true);
  assert.equal(harness.audioElements[0].volume, 0.7);
});

test("p2p local camera and screen tracks are added to existing peers", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
  ]);
  await flush();
  const cameraTrack = track("camera-local", "video");
  const screenTrack = track("screen-local", "video");

  harness.session.setLocalTrack("camera", cameraTrack, stream("camera-stream", [cameraTrack]));
  harness.session.setLocalTrack("screen", screenTrack, stream("screen-stream", [screenTrack]));
  await flush();

  assert.equal(harness.peerConnections[0].addedTracks.some(([added]) => added === cameraTrack), true);
  assert.equal(harness.peerConnections[0].addedTracks.some(([added]) => added === screenTrack), true);

  harness.session.setLocalTrack("camera", null, null);
  await flush();

  assert.equal(harness.peerConnections[0].removedSenders.length >= 1, true);
});

test("p2p remote metadata separates camera and screen video tracks", async () => {
  const harness = p2pHarness({ ownMemberId: "m_a" });
  harness.session.syncMembers([
    { id: "m_a", connected: true },
    { id: "m_b", connected: true },
  ]);
  await flush();
  const peerConnection = harness.peerConnections[0];
  const cameraTrack = track("remote-camera-track", "video");
  const screenTrack = track("remote-screen-track", "video");

  peerConnection.dataChannels[0].emitMessage({
    type: "media_metadata",
    tracks: [
      { track_id: "remote-camera-track", source: "camera" },
      { track_id: "remote-screen-track", source: "screen" },
    ],
  });
  peerConnection.emitTrack({
    track: cameraTrack,
    streams: [stream("remote-camera-stream", [cameraTrack])],
  });
  peerConnection.emitTrack({
    track: screenTrack,
    streams: [stream("remote-screen-stream", [screenTrack])],
  });

  assert.equal(harness.cameraStreams.at(-1)[0].memberId, "m_b");
  assert.deepEqual(harness.cameraStreams.at(-1)[0].stream.getVideoTracks(), [cameraTrack]);
  assert.equal(harness.screenStreams.at(-1).memberId, "m_b");
  assert.deepEqual(harness.screenStreams.at(-1).stream.getVideoTracks(), [screenTrack]);
});
