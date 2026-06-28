<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

const props = defineProps({
  active: { type: Boolean, required: true },
  canShare: { type: Boolean, required: true },
  canStop: { type: Boolean, required: true },
  mediaReady: { type: Boolean, required: true },
  screenPopoutTitle: { type: String, required: true },
  screenShareTitle: { type: String, required: true },
  stream: { default: null },
});

const emit = defineEmits(["start", "stop"]);

const SCREEN_ASPECT_16_9 = 16 / 9;
const SCREEN_ASPECT_16_10 = 16 / 10;

const screenVideo = ref(null);
const screenPopoutVideo = ref(null);
const screenPanel = ref(null);
const screenVideoFrame = ref(null);
const screenPopoutFrame = ref(null);
const popoutOpen = ref(false);
const frameStyle = ref({});

const sharing = computed(() => Boolean(props.screenShareTitle && props.screenShareTitle !== "当前没有屏幕共享"));

function preferredScreenAspectRatio() {
  const video = screenVideo.value;
  const width = video?.videoWidth ?? 0;
  const height = video?.videoHeight ?? 0;
  if (width > 0 && height > 0) {
    const ratio = width / height;
    return Math.abs(ratio - SCREEN_ASPECT_16_10) < Math.abs(ratio - SCREEN_ASPECT_16_9)
      ? SCREEN_ASPECT_16_10
      : SCREEN_ASPECT_16_9;
  }

  return SCREEN_ASPECT_16_9;
}

function resizeScreenVideoFrame() {
  if (!props.active || !screenPanel.value || !screenVideoFrame.value) {
    return;
  }

  const ratio = preferredScreenAspectRatio();
  const panelRect = screenPanel.value.getBoundingClientRect();
  const headRect = screenPanel.value.querySelector(".screen-panel-head")?.getBoundingClientRect();
  const styles = window.getComputedStyle(screenPanel.value);
  const gap = Number.parseFloat(styles.rowGap || styles.gap || "0") || 0;
  const availableWidth = Math.max(0, screenPanel.value.clientWidth);
  const availableHeight = Math.max(0, panelRect.height - (headRect?.height ?? 0) - gap);

  if (!availableWidth || !availableHeight) {
    return;
  }

  let frameWidth = Math.min(availableWidth, availableHeight * ratio);
  let frameHeight = frameWidth / ratio;
  if (frameHeight > availableHeight) {
    frameHeight = availableHeight;
    frameWidth = frameHeight * ratio;
  }

  frameStyle.value = {
    "--screen-frame-ratio": ratio === SCREEN_ASPECT_16_10 ? "16 / 10" : "16 / 9",
    "--screen-frame-width": `${Math.floor(frameWidth)}px`,
    "--screen-frame-height": `${Math.floor(frameHeight)}px`,
  };
}

function attachStream(stream) {
  if (screenVideo.value && screenVideo.value.srcObject !== stream) {
    screenVideo.value.srcObject = stream;
  }
  if (screenPopoutVideo.value && screenPopoutVideo.value.srcObject !== stream) {
    screenPopoutVideo.value.srcObject = stream;
  }
  nextTick(resizeScreenVideoFrame);
}

function requestFullscreen(target) {
  target?.requestFullscreen?.().catch(() => {});
}

function openPopout() {
  if (sharing.value && props.stream) {
    popoutOpen.value = true;
  }
}

function fullscreenMain() {
  requestFullscreen(screenVideoFrame.value);
}

watch(
  () => props.stream,
  (stream) => {
    attachStream(stream);
    if (!stream) {
      popoutOpen.value = false;
    }
  },
);

watch(
  () => props.active,
  (active) => {
    if (active) {
      nextTick(resizeScreenVideoFrame);
    }
  },
);

watch(sharing, (isSharing) => {
  if (!isSharing) {
    popoutOpen.value = false;
  }
});

onMounted(() => {
  attachStream(props.stream);
  window.addEventListener("resize", resizeScreenVideoFrame);
  screenVideo.value?.addEventListener("loadedmetadata", resizeScreenVideoFrame);
  screenVideo.value?.addEventListener("resize", resizeScreenVideoFrame);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", resizeScreenVideoFrame);
  screenVideo.value?.removeEventListener("loadedmetadata", resizeScreenVideoFrame);
  screenVideo.value?.removeEventListener("resize", resizeScreenVideoFrame);
});

defineExpose({
  fullscreenMain,
  openPopout,
  resizeScreenVideoFrame,
});
</script>

<template>
  <section id="screen-panel" ref="screenPanel" class="screen-panel" aria-label="屏幕共享" :hidden="!active">
    <div class="screen-panel-head" hidden>
      <div>
        <strong id="screen-share-title">{{ screenShareTitle }}</strong>
      </div>
    </div>
    <div id="screen-video-frame" ref="screenVideoFrame" class="screen-video-frame" :style="frameStyle">
      <video
        id="screen-video"
        ref="screenVideo"
        playsinline
        autoplay
        :class="{ 'screen-video-active': Boolean(stream) }"
      ></video>
      <span
        id="screen-video-placeholder"
        :class="{ 'screen-video-placeholder-hidden': Boolean(stream) }"
      >
        等待共享画面
      </span>
    </div>
  </section>

  <div id="screen-popout" class="screen-popout" :hidden="!popoutOpen">
    <div class="screen-popout-surface">
      <div class="screen-popout-head">
        <strong id="screen-popout-title">{{ screenPopoutTitle }}</strong>
        <div>
          <button
            id="popout-fullscreen-screen-share"
            type="button"
            class="quiet-button"
            @click="requestFullscreen(screenPopoutFrame)"
          >
            全屏
          </button>
          <button id="close-screen-popout" type="button" class="quiet-button" @click="popoutOpen = false">
            关闭
          </button>
        </div>
      </div>
      <div id="screen-popout-frame" ref="screenPopoutFrame" class="screen-video-frame screen-popout-frame">
        <video id="screen-popout-video" ref="screenPopoutVideo" playsinline autoplay></video>
      </div>
    </div>
  </div>

</template>
