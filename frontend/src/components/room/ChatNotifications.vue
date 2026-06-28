<script setup>
defineProps({
  mentionReminder: { type: Object, required: true },
  toast: { type: Object, required: true },
});
</script>

<template>
  <div
    id="mention-reminder"
    class="mention-reminder"
    role="status"
    aria-live="polite"
    :hidden="!mentionReminder.visible"
  >
    <strong id="mention-reminder-title">{{ mentionReminder.title }}</strong>
    <span id="mention-reminder-text">{{ mentionReminder.text }}</span>
  </div>

  <div id="chat-toast-container" class="chat-toast-container" aria-live="polite" aria-atomic="true">
    <article v-if="toast.visible" class="chat-toast">
      <strong>{{ toast.title }}</strong>
      <span>{{ toast.text }}</span>
    </article>
  </div>
</template>
