import assert from "node:assert/strict";
import test from "node:test";

import {
  canManageMember,
  canToggleMemberListening,
  memberCanSpeakSignal,
  memberLatencySignal,
  memberLatencyClass,
  memberListeningLabel,
  memberListeningSignal,
  memberLatencyView,
  memberPermissionLabel,
  memberSpeakingSignal,
  selfMutedSignal,
} from "../../frontend/src/lib/room-controls.js";

test("room control signals update self mute and speak permission", () => {
  assert.deepEqual(selfMutedSignal(true), {
    type: "set_self_muted",
    self_muted: true,
  });
  assert.deepEqual(memberCanSpeakSignal("m_member", false), {
    type: "set_member_can_speak",
    member_id: "m_member",
    can_speak: false,
  });
  assert.deepEqual(memberSpeakingSignal(true), {
    type: "set_member_speaking",
    speaking: true,
  });
  assert.deepEqual(memberLatencySignal(28.4), {
    type: "set_member_latency",
    server_ms: 28.4,
  });
});

test("only the owner manages another room member", () => {
  const room = {
    owner_member_id: "m_owner",
  };

  assert.equal(
    canManageMember(room, "m_owner", { id: "m_member", can_speak: true }),
    true,
  );
  assert.equal(
    canManageMember(room, "m_owner", { id: "m_owner", can_speak: true }),
    false,
  );
  assert.equal(
    canManageMember(room, "m_member", { id: "m_owner", can_speak: true }),
    false,
  );
});

test("permission control label describes its next action", () => {
  assert.equal(memberPermissionLabel({ can_speak: true }), "禁言");
  assert.equal(memberPermissionLabel({ can_speak: false }), "允许发言");
});

test("member listening controls describe current private receive choice", () => {
  assert.deepEqual(memberListeningSignal("m_member", false), {
    type: "set_member_listening",
    member_id: "m_member",
    listening: false,
  });
  assert.equal(canToggleMemberListening("m_owner", { id: "m_member" }), true);
  assert.equal(canToggleMemberListening("m_owner", { id: "m_owner" }), false);
  assert.equal(memberListeningLabel(false), "不听");
  assert.equal(memberListeningLabel(true), "接收");
});

test("member latency view shows server latency for self and total latency for peers", () => {
  const latency = {
    serverMs: 18.4,
    members: {
      m_member: {
        serverMs: 34.2,
        receiveMs: 26.2,
      },
    },
  };

  assert.deepEqual(memberLatencyView("m_owner", "m_owner", latency), {
    label: "18 ms",
    title: "服务器延迟 18 ms",
    className: "member-latency member-latency-good",
  });
  assert.deepEqual(memberLatencyView("m_member", "m_owner", latency), {
    label: "79 ms",
    title: "该用户服务器延迟 34 ms；当前用户服务器延迟 18 ms；接收缓冲 26 ms；总延迟 79 ms",
    className: "member-latency member-latency-warn",
  });
});

test("member latency view does not reuse current user's latency for unknown peers", () => {
  assert.deepEqual(
    memberLatencyView("m_member", "m_owner", {
      serverMs: 18.4,
      members: {},
    }),
    {
      label: "-- ms",
      title: "该用户服务器延迟 -- ms；当前用户服务器延迟 18 ms；总延迟 暂无该用户延迟统计",
      className: "member-latency member-latency-unknown",
    },
  );
});

test("member latency class uses quality thresholds", () => {
  assert.equal(memberLatencyClass(12), "member-latency member-latency-good");
  assert.equal(memberLatencyClass(58), "member-latency member-latency-warn");
  assert.equal(memberLatencyClass(120), "member-latency member-latency-bad");
  assert.equal(memberLatencyClass(null), "member-latency member-latency-unknown");
});
