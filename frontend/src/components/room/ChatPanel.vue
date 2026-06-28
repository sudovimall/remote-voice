<script setup>
import { nextTick, ref, watch } from "vue";
import {
  chatAvatarText,
  chatMessageContentParts,
  chatMessageView,
} from "../../lib/chat-controls.js";

const props = defineProps({
  active: { type: Boolean, required: true },
  hideMentionPicker: { type: Function, required: true },
  mentionPickerIndex: { type: Number, required: true },
  mentionPickerMembers: { type: Array, required: true },
  messages: { type: Array, required: true },
  modelValue: { type: String, required: true },
  ownMemberId: { type: String, required: true },
  renderMentionPicker: { type: Function, required: true },
  selectMention: { type: Function, required: true },
  setMentionPickerIndex: { type: Function, required: true },
  submitMessage: { type: Function, required: true },
});

const emit = defineEmits(["update:modelValue"]);
const chatInput = ref(null);
const chatMessagesNode = ref(null);

function setInputValue(value) {
  emit("update:modelValue", value);
}

function scrollToLatest() {
  nextTick(() => {
    if (chatMessagesNode.value) {
      chatMessagesNode.value.scrollTop = chatMessagesNode.value.scrollHeight;
    }
  });
}

function viewFor(message) {
  return chatMessageView(message, props.ownMemberId);
}

function optionAvatar(member) {
  return chatAvatarText(member);
}

async function chooseMention(member) {
  const input = chatInput.value;
  const cursor = props.selectMention(
    member,
    input?.selectionStart ?? props.modelValue.length,
    input?.selectionEnd ?? props.modelValue.length,
  );
  await nextTick();
  input?.focus();
  input?.setSelectionRange(cursor, cursor);
}

async function submit() {
  const sent = await props.submitMessage();
  await nextTick();
  if (sent) {
    chatInput.value?.focus();
  }
}

function onInput(event) {
  setInputValue(event.target.value);
  nextTick(() => {
    props.renderMentionPicker(chatInput.value?.selectionStart ?? event.target.value.length);
  });
}

function onKeydown(event) {
  if (props.mentionPickerMembers.length && ["ArrowDown", "ArrowUp", "Enter", "Escape"].includes(event.key)) {
    if (event.key === "Escape") {
      event.preventDefault();
      props.hideMentionPicker();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex =
        (props.mentionPickerIndex + delta + props.mentionPickerMembers.length) %
        props.mentionPickerMembers.length;
      props.setMentionPickerIndex(nextIndex);
      return;
    }
    if (event.key === "Enter" && props.mentionPickerMembers[props.mentionPickerIndex]) {
      event.preventDefault();
      void chooseMention(props.mentionPickerMembers[props.mentionPickerIndex]);
      return;
    }
  }

  if (event.key !== "Enter" || event.shiftKey || event.isComposing) {
    return;
  }

  event.preventDefault();
  void submit();
}

watch(
  () => props.messages.length,
  () => {
    scrollToLatest();
  },
);

watch(
  () => props.active,
  (active) => {
    if (active) {
      scrollToLatest();
      nextTick(() => chatInput.value?.focus({ preventScroll: true }));
    }
  },
);
</script>

<template>
  <section id="chat-panel" class="chat-panel" aria-label="文字聊天" :hidden="!active">
    <div id="chat-messages" ref="chatMessagesNode" class="chat-messages" aria-live="polite">
      <div v-if="!messages.length" class="chat-empty">还没有消息</div>
      <article
        v-for="message in messages"
        v-else
        :key="message.id"
        class="chat-message"
        :class="{ 'chat-message-own': viewFor(message).own }"
      >
        <span class="chat-avatar">{{ viewFor(message).avatar }}</span>
        <div class="chat-bubble">
          <div class="chat-message-meta">
            <strong>{{ viewFor(message).nickname }}</strong>
            <time>{{ viewFor(message).timeLabel }}</time>
          </div>
          <p>
            <template v-for="(part, index) in chatMessageContentParts(message)" :key="index">
              <span
                v-if="part.type === 'mention'"
                class="chat-mention"
                :class="{ 'chat-mention-self': part.memberId === ownMemberId }"
              >
                {{ part.text }}
              </span>
              <template v-else>{{ part.text }}</template>
            </template>
          </p>
        </div>
      </article>
    </div>

    <form id="chat-form" class="chat-form" @submit.prevent="submit">
      <div
        id="mention-picker"
        class="mention-picker"
        role="listbox"
        aria-label="@ 成员候选"
        :hidden="!mentionPickerMembers.length"
      >
        <button
          v-for="(member, index) in mentionPickerMembers"
          :key="member.id"
          type="button"
          class="mention-option"
          :class="{ 'mention-option-active': index === mentionPickerIndex }"
          role="option"
          :aria-selected="String(index === mentionPickerIndex)"
          @mousedown.prevent
          @click="chooseMention(member)"
        >
          <span class="mention-option-avatar">{{ optionAvatar(member) }}</span>
          <span class="mention-option-name">{{ member.nickname }}</span>
        </button>
      </div>
      <textarea
        id="chat-input"
        ref="chatInput"
        maxlength="500"
        rows="3"
        aria-label="聊天消息"
        placeholder="输入文字消息"
        :value="modelValue"
        @input="onInput"
        @keydown="onKeydown"
        @blur="window.setTimeout(hideMentionPicker, 120)"
      ></textarea>
      <button id="chat-send" type="submit">发送</button>
    </form>
  </section>
</template>
