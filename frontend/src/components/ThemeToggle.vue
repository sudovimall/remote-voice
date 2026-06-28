<script setup>
import { nextTheme } from "../lib/theme.js";

const props = defineProps({
  theme: {
    type: String,
    required: true,
  },
});

const emit = defineEmits(["change"]);
</script>

<template>
  <button
    class="theme-toggle"
    type="button"
    :aria-label="theme === 'dark' ? '切换亮色模式' : '切换暗色模式'"
    :title="theme === 'dark' ? '切换亮色模式' : '切换暗色模式'"
    @click="emit('change', nextTheme(theme))"
  >
    <span aria-hidden="true">{{ theme === "dark" ? "☼" : "◐" }}</span>
  </button>
</template>
