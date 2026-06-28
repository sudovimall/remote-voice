<script setup>
import { onMounted, reactive } from "vue";
import { useLobbySession } from "../composables/useLobbySession.js";

const lobby = reactive(useLobbySession());

onMounted(() => {
  lobby.boot();
});
</script>

<template>
  <main class="lobby-shell signal-page">
    <section class="brand-rail" aria-labelledby="lobby-title">
      <div class="brand-lockup">
        <p class="eyebrow">Remote Voice</p>
        <h1 id="lobby-title">进入语音房间</h1>
        <p class="hero-copy">低延迟语音、文字聊天和屏幕共享都集中在一个实时控制台里。</p>
      </div>

      <div class="channel-board" aria-hidden="true">
        <div class="channel-board-head">
          <span>Voice</span>
          <span>SFU</span>
        </div>
        <div class="channel-line channel-line-live">
          <span></span>
          <strong></strong>
          <i></i>
        </div>
        <div class="channel-line">
          <span></span>
          <strong></strong>
          <i></i>
        </div>
        <div class="channel-line channel-line-short">
          <span></span>
          <strong></strong>
          <i></i>
        </div>
      </div>

      <div class="stage-strip" aria-live="polite">
        <span class="stage-dot" aria-hidden="true"></span>
        <span id="lobby-status">{{ lobby.statusMessage }}</span>
      </div>

      <div id="auth-controls" class="auth-controls" :hidden="!lobby.showAuthControls">
        <span>{{ lobby.authName }}</span>
        <div class="auth-actions">
          <a v-if="lobby.showAdminLink" href="/admin">管理</a>
          <button type="button" @click="lobby.logout">退出</button>
        </div>
      </div>
    </section>

    <section class="lobby-actions" aria-label="房间入口">
      <label class="field field-wide">
        <span>昵称</span>
        <input
          id="nickname"
          v-model="lobby.nickname"
          name="nickname"
          autocomplete="nickname"
          maxlength="32"
          placeholder="输入昵称"
        >
      </label>

      <div id="lobby-error" class="inline-message" role="status" :hidden="!lobby.errorMessage">
        {{ lobby.errorMessage }}
      </div>

      <form id="create-room" class="action-band" @submit.prevent="lobby.createRoom">
        <div>
          <h2>创建房间</h2>
          <p>生成新的语音空间。</p>
        </div>
        <button type="submit">创建</button>
      </form>

      <form id="join-room" class="action-band join-band" @submit.prevent="lobby.joinEnteredRoom">
        <label class="field">
          <span>房间号</span>
          <input
            id="room-id"
            v-model="lobby.roomId"
            name="room_id"
            autocomplete="off"
            autocapitalize="characters"
            maxlength="12"
            placeholder="ABC123"
          >
        </label>
        <button type="submit">加入</button>
      </form>

      <section class="lobby-room-browser" aria-labelledby="room-browser-title">
        <div class="pane-head lobby-room-head">
          <div>
            <h2 id="room-browser-title">全部房间</h2>
            <p id="room-browser-meta" class="meta-copy">{{ lobby.roomsMeta }}</p>
          </div>
          <button
            id="refresh-rooms"
            type="button"
            class="quiet-button"
            :disabled="lobby.roomsLoading"
            @click="lobby.refreshRooms"
          >
            刷新
          </button>
        </div>
        <div id="room-browser-list" class="lobby-room-list" aria-live="polite">
          <p v-if="lobby.roomListMessage">{{ lobby.roomListMessage }}</p>
          <article
            v-for="room in lobby.rooms"
            :key="room.id"
            class="lobby-room-row"
          >
            <div>
              <strong>{{ room.id }}</strong>
              <span>{{ room.memberCount }} 位成员</span>
            </div>
            <button type="button" @click="lobby.joinRoom(room.id)">加入</button>
          </article>
        </div>
      </section>
    </section>
  </main>
</template>
