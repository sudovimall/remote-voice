<script setup>
import { computed } from "vue";

const props = defineProps({
  localCameraStream: { type: Object, default: null },
  members: { type: Array, required: true },
  ownMemberId: { type: String, required: true },
  remoteCameraStreams: { type: Array, required: true },
  speakingMemberIds: { type: Object, required: true },
  videoCallPublishers: { type: Object, default: () => ({}) },
});

const vStream = {
  // 把 MediaStream 直接绑定到 video.srcObject，避免 Vue 把它当普通 DOM 属性序列化。
  mounted(element, binding) {
    element.srcObject = binding.value ?? null;
  },
  // 当远端重新协商替换 stream 时同步到同一个 video 元素，保持 tile 尺寸稳定。
  updated(element, binding) {
    if (element.srcObject !== binding.value) {
      element.srcObject = binding.value ?? null;
    }
  },
};

const remoteStreamsByMember = computed(() =>
  Object.fromEntries(props.remoteCameraStreams.map((entry) => [entry.memberId, entry.stream])),
);

// 合并房间成员、发布状态和媒体流，宫格始终以成员为单位渲染占位。
const tiles = computed(() =>
  props.members.map((member) => {
    const isOwnMember = member.id === props.ownMemberId;
    const stream = isOwnMember ? props.localCameraStream : remoteStreamsByMember.value[member.id];
    const publishing = Boolean(props.videoCallPublishers?.[member.id]);
    return {
      member,
      isOwnMember,
      publishing,
      speaking: props.speakingMemberIds?.has?.(member.id) ?? false,
      status: member.connected
        ? publishing
          ? stream
            ? "视频已连接"
            : "等待视频流"
          : "摄像头未开启"
        : "离线",
      stream,
    };
  }),
);
</script>

<template>
  <section id="video-grid-panel" class="video-grid-panel" aria-label="视频通话宫格">
    <article
      v-for="tile in tiles"
      :key="tile.member.id"
      class="video-tile"
      :class="{
        'video-tile-active': tile.stream,
        'video-tile-speaking': tile.speaking,
      }"
    >
      <video
        v-if="tile.stream"
        v-stream="tile.stream"
        autoplay
        playsinline
        :muted="tile.isOwnMember"
      />
      <div v-else class="video-tile-placeholder">
        <span class="video-avatar">{{ tile.member.nickname?.slice(0, 1) || "?" }}</span>
      </div>
      <footer class="video-tile-meta">
        <strong>{{ tile.member.nickname }}</strong>
        <span>{{ tile.isOwnMember ? "我" : tile.status }}</span>
      </footer>
    </article>
  </section>
</template>
