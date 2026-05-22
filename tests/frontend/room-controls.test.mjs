import assert from "node:assert/strict";
import test from "node:test";

import {
  canManageMember,
  canToggleMemberListening,
  memberCanSpeakSignal,
  memberListeningLabel,
  memberListeningSignal,
  memberPermissionLabel,
  selfMutedSignal,
} from "../../static/room-controls.mjs";

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
