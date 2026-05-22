export function selfMutedSignal(selfMuted) {
  return {
    type: "set_self_muted",
    self_muted: selfMuted,
  };
}

export function memberCanSpeakSignal(memberId, canSpeak) {
  return {
    type: "set_member_can_speak",
    member_id: memberId,
    can_speak: canSpeak,
  };
}

export function canManageMember(room, ownMemberId, member) {
  return room?.owner_member_id === ownMemberId && member?.id !== ownMemberId;
}

export function memberPermissionLabel(member) {
  return member?.can_speak ? "禁言" : "允许发言";
}
