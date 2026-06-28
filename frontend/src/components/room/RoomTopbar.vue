<script setup>
defineProps({
  connectionLabel: { type: String, required: true },
  roomIdLabel: { type: String, required: true },
  voicePaneCollapsed: { type: Boolean, required: true },
});

const emit = defineEmits(["toggleVoicePane"]);
</script>

<template>
  <header class="room-topbar">
    <div class="room-heading">
      <p class="eyebrow">语音房间</p>
      <h1>房间 <span id="room-id">{{ roomIdLabel }}</span></h1>
    </div>
    <div class="room-meta">
      <span id="room-connection" class="status-pill">{{ connectionLabel }}</span>
      <button
        id="toggle-voice-pane"
        type="button"
        class="quiet-button"
        :aria-expanded="String(!voicePaneCollapsed)"
        @click="emit('toggleVoicePane')"
      >
        {{ voicePaneCollapsed ? "显示语音" : "隐藏语音" }}
      </button>
      <a class="quiet-button" href="/">离开</a>
    </div>
  </header>
</template>
