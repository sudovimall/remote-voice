<script setup>
import {
  canManageMember,
  canToggleMemberListening,
  memberLatencyView,
  memberListeningLabel,
  memberPermissionLabel,
} from "../../lib/room-controls.js";
import { volumePercent } from "../../lib/audio-volume.js";

const props = defineProps({
  currentRoom: { type: Object, default: null },
  getMemberVolume: { type: Function, required: true },
  latencySnapshot: { type: Object, required: true },
  members: { type: Array, required: true },
  notListeningMemberIds: { type: Object, required: true },
  ownMemberId: { type: String, required: true },
  speakingMemberIds: { type: Object, required: true },
});

const emit = defineEmits(["setMemberVolume", "toggleListening", "togglePermission"]);

function avatarText(member) {
  return Array.from(member.nickname || "?")[0] ?? "?";
}

function speakingLabel(member) {
  if (!member.can_speak) {
    return "已禁言";
  }
  if (member.self_muted) {
    return "已静音";
  }

  return "可发言";
}

function memberStateLabel(member) {
  if (member.id === props.ownMemberId) {
    return "当前成员";
  }
  if (!member.connected) {
    return "待连接";
  }

  return "已连接";
}

function roleLabel(member) {
  return member.id === props.currentRoom?.owner_member_id ? "房主" : "成员";
}
</script>

<template>
  <div id="member-list" class="member-list" aria-label="成员列表">
    <article v-if="!members.length" class="member-row member-row-ghost">
      <div class="member-identity">
        <span class="member-avatar member-avatar-empty">+</span>
        <div>
          <strong>等待成员状态</strong>
          <span>{{ currentRoom ? "等待成员加入。" : "返回大厅重新进入。" }}</span>
        </div>
      </div>
    </article>

    <article
      v-for="member in members"
      v-else
      :key="member.id"
      class="member-row"
      :class="{ 'member-row-owner': member.id === currentRoom?.owner_member_id }"
    >
      <div class="member-identity">
        <span
          class="member-avatar"
          :class="{ 'member-avatar-muted': member.id !== currentRoom?.owner_member_id }"
        >
          {{ avatarText(member) }}
        </span>
        <div>
          <div class="member-name-line">
            <strong>{{ member.nickname }}</strong>
            <span
              class="member-speaking-indicator"
              :class="{
                'member-speaking-indicator-active':
                  speakingMemberIds.has(member.id) && member.can_speak && !member.self_muted,
              }"
              title="发言中"
              aria-label="发言中"
            ></span>
          </div>
          <span class="member-state">{{ memberStateLabel(member) }}</span>
        </div>
      </div>

      <div class="member-signals">
        <span
          class="role-chip"
          :class="{ 'role-chip-muted': member.id !== currentRoom?.owner_member_id }"
        >
          {{ roleLabel(member) }}
        </span>
        <span
          class="signal-chip"
          :class="{ 'signal-chip-ready': member.can_speak && !member.self_muted }"
        >
          {{ speakingLabel(member) }}
        </span>
        <button
          type="button"
          class="member-toggle"
          :disabled="!canManageMember(currentRoom, ownMemberId, member)"
          @click="emit('togglePermission', member)"
        >
          {{ canManageMember(currentRoom, ownMemberId, member) ? memberPermissionLabel(member) : "权限" }}
        </button>
        <button
          v-if="canToggleMemberListening(ownMemberId, member)"
          type="button"
          class="member-toggle member-listening-toggle"
          @click="emit('toggleListening', member)"
        >
          {{ memberListeningLabel(notListeningMemberIds.has(member.id)) }}
        </button>
        <label v-if="member.id !== ownMemberId" class="volume-control member-volume-control">
          <span>音量</span>
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            :value="getMemberVolume(member.id)"
            :aria-label="`${member.nickname} 播放音量`"
            @input="emit('setMemberVolume', member.id, $event.target.value)"
          >
          <strong class="volume-value">{{ volumePercent(getMemberVolume(member.id)) }}</strong>
        </label>
        <span
          :class="memberLatencyView(member.id, ownMemberId, latencySnapshot).className"
          :title="memberLatencyView(member.id, ownMemberId, latencySnapshot).title"
          :aria-label="memberLatencyView(member.id, ownMemberId, latencySnapshot).title"
        >
          {{ memberLatencyView(member.id, ownMemberId, latencySnapshot).label }}
        </span>
      </div>
    </article>
  </div>
</template>
