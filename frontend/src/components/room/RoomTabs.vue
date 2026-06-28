<script setup>
defineProps({
  activePanel: { type: String, required: true },
  unreadBadge: { type: String, default: "" },
});

const emit = defineEmits(["select"]);

const tabs = [
  { id: "members-tab", panel: "members", label: "成员" },
  { id: "chat-tab", panel: "chat", label: "聊天" },
  { id: "screen-tab", panel: "screen", label: "共享" },
];
</script>

<template>
  <div class="panel-tabs" role="tablist" aria-label="侧栏视图">
    <button
      v-for="tab in tabs"
      :id="tab.id"
      :key="tab.panel"
      class="panel-tab"
      :class="{ 'panel-tab-active': activePanel === tab.panel }"
      type="button"
      :data-panel="tab.panel"
      :aria-selected="String(activePanel === tab.panel)"
      @click="emit('select', tab.panel)"
    >
      {{ tab.label }}
    </button>
    <span id="chat-unread" class="chat-unread" :hidden="!unreadBadge">{{ unreadBadge }}</span>
  </div>
</template>
