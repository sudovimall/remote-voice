<script setup>
import VideoGridPanel from "./VideoGridPanel.vue";

defineProps({
  active: { type: Boolean, required: true },
  cameraBusy: { type: Boolean, required: true },
  cameraStateLabel: { type: String, required: true },
  cameraToggleLabel: { type: String, required: true },
  canUseCamera: { type: Boolean, required: true },
  localCameraStream: { type: Object, default: null },
  mediaReady: { type: Boolean, required: true },
  members: { type: Array, required: true },
  ownMemberId: { type: String, required: true },
  remoteCameraStreams: { type: Array, required: true },
  speakingMemberIds: { type: Object, required: true },
  videoCallPublishers: { type: Object, default: () => ({}) },
});

const emit = defineEmits(["toggleCamera"]);
</script>

<template>
  <section id="video-call-panel" class="video-call-panel" aria-label="视频通话" :hidden="!active">
    <div class="video-toolbar">
      <button
        id="toggle-camera"
        type="button"
        class="quiet-button"
        :disabled="!mediaReady || !canUseCamera || cameraBusy"
        :title="canUseCamera ? '切换摄像头视频' : '当前浏览器不支持摄像头'"
        @click="emit('toggleCamera')"
      >
        {{ cameraToggleLabel }}
      </button>
      <span id="camera-state" class="camera-state">{{ cameraStateLabel }}</span>
    </div>

    <VideoGridPanel
      :local-camera-stream="localCameraStream"
      :members="members"
      :own-member-id="ownMemberId"
      :remote-camera-streams="remoteCameraStreams"
      :speaking-member-ids="speakingMemberIds"
      :video-call-publishers="videoCallPublishers"
    />
  </section>
</template>
