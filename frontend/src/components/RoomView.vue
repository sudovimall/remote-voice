<script setup>
import { reactive, ref } from "vue";
import { useRoomSession } from "../composables/useRoomSession.js";
import ChatNotifications from "./room/ChatNotifications.vue";
import ChatPanel from "./room/ChatPanel.vue";
import MembersPanel from "./room/MembersPanel.vue";
import RoomTabs from "./room/RoomTabs.vue";
import RoomTopbar from "./room/RoomTopbar.vue";
import ScreenSharePanel from "./room/ScreenSharePanel.vue";
import VideoGridPanel from "./room/VideoGridPanel.vue";
import VoicePanel from "./room/VoicePanel.vue";

const room = reactive(useRoomSession());
const screenPanel = ref(null);

// 打开独立屏幕共享窗口，方便用户在主房间内继续聊天或查看成员。
function openScreenPopout() {
  screenPanel.value?.openPopout();
}

// 进入浏览器全屏展示共享画面，保持演示和远程协作时的可读性。
function fullscreenScreenShare() {
  screenPanel.value?.fullscreenMain();
}
</script>

<template>
  <main
    class="room-shell signal-page"
    :class="{ 'voice-pane-collapsed': room.voicePaneCollapsed }"
  >
    <RoomTopbar
      :connection-label="room.connectionLabel"
      :room-id-label="room.roomIdLabel"
      :voice-pane-collapsed="room.voicePaneCollapsed"
      @toggle-voice-pane="room.setVoicePaneCollapsed(!room.voicePaneCollapsed)"
    />

    <div id="room-error" class="room-alert" role="status" :hidden="!room.errorMessage">
      {{ room.errorMessage }}
    </div>

    <section class="room-grid">
      <section
        id="side-panel"
        class="members-pane"
        aria-labelledby="members-title"
        :data-active-panel="room.activeSidePanel"
      >
        <div class="pane-head">
          <div>
            <h2 id="members-title">{{ room.panelTitle }}</h2>
            <span id="members-meta" class="meta-copy">{{ room.membersMeta }}</span>
          </div>
          <div class="pane-controls">
            <div id="screen-toolbar" class="screen-toolbar" :hidden="room.activeSidePanel !== 'screen'">
              <button
                id="start-screen-share"
                type="button"
                class="quiet-button"
                :hidden="Boolean(room.currentScreenShare)"
                :disabled="!room.mediaReady || !room.canShareScreen"
                :title="room.canShareScreen ? '开始共享屏幕' : '当前浏览器不支持屏幕共享'"
                @click="room.startScreenShare"
              >
                开始共享屏幕
              </button>
              <button
                id="open-screen-popout"
                type="button"
                class="quiet-button"
                :disabled="!room.currentScreenShare || !room.activeScreenStream"
                @click="openScreenPopout"
              >
                弹窗
              </button>
              <button
                id="fullscreen-screen-share"
                type="button"
                class="quiet-button"
                :disabled="!room.currentScreenShare || !room.activeScreenStream"
                @click="fullscreenScreenShare"
              >
                全屏
              </button>
              <button
                id="stop-screen-share"
                type="button"
                class="quiet-button"
                :hidden="!room.canStopScreenShare"
                @click="room.stopScreenShare"
              >
                停止共享
              </button>
            </div>
            <RoomTabs
              :active-panel="room.activeSidePanel"
              :unread-badge="room.unreadBadgeLabel"
              @select="room.setActiveSidePanel"
            />
          </div>
        </div>
      </section>

      <section class="stage-pane" aria-label="房间主区域">
        <div class="stage-content">
          <ChatNotifications :mention-reminder="room.mentionReminder" :toast="room.chatToast" />
          <div v-show="room.activeSidePanel === 'members'" class="room-overview-panel">
            <VideoGridPanel
              :local-camera-stream="room.localCameraStream"
              :members="room.members"
              :own-member-id="room.ownMemberId"
              :remote-camera-streams="room.remoteCameraStreams"
              :speaking-member-ids="room.speakingMemberIds"
              :video-call-publishers="room.currentRoom?.video_call_publishers ?? {}"
            />
            <MembersPanel
              :current-room="room.currentRoom"
              :get-member-volume="room.memberVolume"
              :latency-snapshot="room.latencySnapshot"
              :members="room.members"
              :not-listening-member-ids="room.notListeningMemberIds"
              :own-member-id="room.ownMemberId"
              :speaking-member-ids="room.speakingMemberIds"
              @set-member-volume="room.setMemberVolume"
              @toggle-listening="room.toggleMemberListening"
              @toggle-permission="room.toggleMemberPermission"
            />
          </div>
          <ChatPanel
            v-model="room.chatInput"
            :active="room.activeSidePanel === 'chat'"
            :hide-mention-picker="room.hideMentionPicker"
            :mention-picker-index="room.mentionPickerIndex"
            :mention-picker-members="room.mentionPickerMembers"
            :messages="room.chatMessages"
            :own-member-id="room.ownMemberId"
            :render-mention-picker="room.renderMentionPicker"
            :select-mention="room.selectMention"
            :set-mention-picker-index="room.setMentionPickerIndex"
            :submit-message="room.submitChatMessage"
          />
          <ScreenSharePanel
            ref="screenPanel"
            :active="room.activeSidePanel === 'screen'"
            :can-share="room.canShareScreen"
            :can-stop="room.canStopScreenShare"
            :media-ready="room.mediaReady"
            :screen-popout-title="room.screenPopoutTitle"
            :screen-share-title="room.screenShareTitle"
            :stream="room.activeScreenStream"
          />
        </div>
      </section>

      <VoicePanel
        v-show="!room.voicePaneCollapsed"
        :device-state-label="room.deviceStateLabel"
        :downlink-state-label="room.downlinkStateLabel"
        :media-ready="room.mediaReady"
        :media-state-label="room.mediaStateLabel"
        :mic-state-label="room.micStateLabel"
        :camera-busy="room.cameraBusy"
        :camera-state-label="room.cameraStateLabel"
        :camera-toggle-label="room.cameraToggleLabel"
        :can-use-camera="room.canUseCamera"
        :microphone-gain-level="room.microphoneGainLevel"
        :microphone-gain-percent="room.microphoneGainPercent"
        :microphone-gain-supported="room.microphoneGainSupported"
        :mute-self-label="room.muteSelfLabel"
        :permission-note="room.permissionNote"
        @leave="room.leaveRoom"
        @set-microphone-gain="room.setMicrophoneGain"
        @toggle-camera="room.toggleCamera"
        @toggle-mute="room.toggleSelfMuted"
      />
    </section>

    <div id="remote-audio" hidden aria-hidden="true"></div>
  </main>
</template>
