<script setup>
import { computed, onMounted, ref } from "vue";
import LobbyView from "./components/LobbyView.vue";
import RoomView from "./components/RoomView.vue";
import ThemeToggle from "./components/ThemeToggle.vue";
import { applyTheme, storedTheme } from "./lib/theme.js";

const theme = ref(storedTheme(window.localStorage));
const routePath = ref(window.location.pathname);
const isRoom = computed(() => routePath.value.startsWith("/rooms/"));

function setTheme(value) {
  theme.value = applyTheme(document.documentElement, window.localStorage, value);
}

onMounted(() => {
  setTheme(theme.value);
});
</script>

<template>
  <ThemeToggle :theme="theme" @change="setTheme" />
  <RoomView v-if="isRoom" />
  <LobbyView v-else />
</template>
