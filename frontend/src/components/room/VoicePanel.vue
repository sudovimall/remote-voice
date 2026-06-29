<script setup>
defineProps({
  deviceStateLabel: { type: String, required: true },
  downlinkStateLabel: { type: String, required: true },
  mediaReady: { type: Boolean, required: true },
  mediaStateLabel: { type: String, required: true },
  micStateLabel: { type: String, required: true },
  cameraBusy: { type: Boolean, required: true },
  cameraStateLabel: { type: String, required: true },
  cameraToggleLabel: { type: String, required: true },
  canUseCamera: { type: Boolean, required: true },
  microphoneGainLevel: { type: Number, required: true },
  microphoneGainPercent: { type: String, required: true },
  microphoneGainSupported: { type: Boolean, required: true },
  muteSelfLabel: { type: String, required: true },
  permissionNote: { type: String, required: true },
});

const emit = defineEmits(["leave", "setMicrophoneGain", "toggleCamera", "toggleMute"]);
</script>

<template>
  <aside id="voice-pane" class="voice-pane" aria-labelledby="voice-title">
    <div class="pane-head">
      <h2 id="voice-title">本地语音</h2>
      <span class="meta-copy">单房间会话</span>
    </div>
    <label class="volume-control microphone-gain-control">
      <span>输入音量</span>
      <input
        id="microphone-gain"
        type="range"
        min="0"
        max="2"
        step="0.05"
        :value="microphoneGainLevel"
        :disabled="!microphoneGainSupported"
        :title="microphoneGainSupported ? '调整别人听到的麦克风音量' : '当前浏览器不支持输入音量调节'"
        @input="emit('setMicrophoneGain', $event.target.value)"
      >
      <strong id="microphone-gain-value">{{ microphoneGainPercent }}</strong>
    </label>

    <button id="mic-state" class="mic-button" type="button" disabled>
      <span aria-hidden="true"></span>
      {{ micStateLabel }}
    </button>

    <div class="signal-stack">
      <p>
        <span>设备状态</span>
        <strong id="device-state">{{ deviceStateLabel }}</strong>
      </p>
      <p>
        <span>媒体状态</span>
        <strong id="media-state">{{ mediaStateLabel }}</strong>
      </p>
      <p>
        <span>下行音频</span>
        <strong id="downlink-state">{{ downlinkStateLabel }}</strong>
      </p>
    </div>

    <div id="permission-note" class="permission-note">{{ permissionNote }}</div>

    <div class="camera-control">
      <button
        id="toggle-camera"
        type="button"
        class="quiet-button quiet-button-wide"
        :disabled="!mediaReady || !canUseCamera || cameraBusy"
        :title="canUseCamera ? '切换摄像头视频' : '当前浏览器不支持摄像头'"
        @click="emit('toggleCamera')"
      >
        {{ cameraToggleLabel }}
      </button>
      <span id="camera-state">{{ cameraStateLabel }}</span>
    </div>

    <div class="voice-actions">
      <button
        id="mute-self"
        type="button"
        class="quiet-button quiet-button-wide"
        :disabled="!mediaReady"
        @click="emit('toggleMute')"
      >
        {{ muteSelfLabel }}
      </button>
      <button id="leave-room" type="button" class="quiet-button quiet-button-wide" @click="emit('leave')">
        断开
      </button>
    </div>
  </aside>
</template>
