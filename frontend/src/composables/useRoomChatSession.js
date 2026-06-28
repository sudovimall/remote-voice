import { nextTick, ref } from "vue";
import {
  chatUnreadBadgeText,
  insertMentionText,
  mentionCandidates,
  mentionsForSend,
  messageMentionsCurrentMember,
  nextChatUnreadCount,
  sendChatMessageSignal,
} from "../lib/chat-controls.js";

const MENTION_REMINDER_MS = 10000;
const CHAT_TOAST_MS = 5000;

export function useRoomChatSession({
  activeSidePanel,
  clientRef,
  currentRoom,
  onError,
  ownMemberId,
}) {
  const unreadChatCount = ref(0);
  const unreadBadgeLabel = ref("");
  const chatMessages = ref([]);
  const chatInput = ref("");
  const selectedMentions = ref([]);
  const mentionPickerMembers = ref([]);
  const mentionPickerIndex = ref(0);
  const mentionReminder = ref({ visible: false, title: "", text: "" });
  const chatToast = ref({ visible: false, title: "", text: "" });

  let mentionReminderTimer = null;
  let chatToastTimer = null;

  function renderUnreadBadge() {
    unreadBadgeLabel.value = chatUnreadBadgeText(unreadChatCount.value);
  }

  function clearMentionReminder() {
    if (mentionReminderTimer) {
      window.clearTimeout(mentionReminderTimer);
      mentionReminderTimer = null;
    }
    mentionReminder.value = { visible: false, title: "", text: "" };
  }

  function showMentionReminder(message) {
    if (message?.member_id === ownMemberId.value || !messageMentionsCurrentMember(message, ownMemberId.value)) {
      return;
    }

    mentionReminder.value = {
      visible: true,
      title: `${message.nickname || "成员"} @ 了你`,
      text: message.content || "",
    };
    if (mentionReminderTimer) {
      window.clearTimeout(mentionReminderTimer);
    }
    mentionReminderTimer = window.setTimeout(clearMentionReminder, MENTION_REMINDER_MS);
  }

  function clearChatToast() {
    if (chatToastTimer) {
      window.clearTimeout(chatToastTimer);
      chatToastTimer = null;
    }
    chatToast.value = { visible: false, title: "", text: "" };
  }

  function showChatToast(message) {
    if (message?.member_id === ownMemberId.value) {
      return;
    }

    chatToast.value = {
      visible: true,
      title: `${message.nickname || "成员"} 发来消息`,
      text: message.content || "",
    };
    if (chatToastTimer) {
      window.clearTimeout(chatToastTimer);
    }
    chatToastTimer = window.setTimeout(clearChatToast, CHAT_TOAST_MS);
  }

  function rememberChatMessages(messages = []) {
    chatMessages.value = messages;
    selectedMentions.value = [];
    hideMentionPicker();
  }

  function handleChatMessage(message) {
    if (!message) {
      return;
    }
    chatMessages.value = [...chatMessages.value, message];
    unreadChatCount.value = nextChatUnreadCount(
      unreadChatCount.value,
      activeSidePanel.value,
      message,
      ownMemberId.value,
    );
    showMentionReminder(message);
    showChatToast(message);
    renderUnreadBadge();
  }

  function activeMentionQuery(cursor = chatInput.value.length) {
    const prefix = chatInput.value.slice(0, cursor);
    const atIndex = prefix.lastIndexOf("@");
    if (atIndex < 0) {
      return null;
    }
    const query = prefix.slice(atIndex + 1);
    if (/\s/.test(query)) {
      return null;
    }

    return query;
  }

  function hideMentionPicker() {
    mentionPickerMembers.value = [];
    mentionPickerIndex.value = 0;
  }

  function renderMentionPicker(cursor = chatInput.value.length) {
    const query = activeMentionQuery(cursor);
    if (query === null || !currentRoom.value) {
      hideMentionPicker();
      return;
    }

    const candidates = mentionCandidates(currentRoom.value, ownMemberId.value).filter((member) =>
      (member.nickname ?? "").toLowerCase().includes(query.toLowerCase()),
    );
    mentionPickerMembers.value = candidates;
    mentionPickerIndex.value = Math.min(mentionPickerIndex.value, Math.max(candidates.length - 1, 0));
    if (!candidates.length) {
      hideMentionPicker();
    }
  }

  function selectMention(member, selectionStart = chatInput.value.length, selectionEnd = chatInput.value.length) {
    const inserted = insertMentionText(chatInput.value, selectionStart, selectionEnd, member);
    chatInput.value = inserted.value;
    selectedMentions.value = [...selectedMentions.value, inserted.mention];
    hideMentionPicker();
    return inserted.cursor;
  }

  function setMentionPickerIndex(index) {
    if (!mentionPickerMembers.value.length) {
      mentionPickerIndex.value = 0;
      return;
    }
    mentionPickerIndex.value =
      (index + mentionPickerMembers.value.length) % mentionPickerMembers.value.length;
  }

  function activateChatPanel() {
    unreadChatCount.value = 0;
    renderUnreadBadge();
    nextTick(() => {
      clearMentionReminder();
    });
  }

  async function submitChatMessage() {
    let signal;
    const mentions = mentionsForSend(chatInput.value, selectedMentions.value);
    try {
      signal = sendChatMessageSignal(chatInput.value, undefined, mentions);
    } catch (error) {
      onError(error.message || "聊天消息无效。");
      return false;
    }

    chatInput.value = "";
    selectedMentions.value = [];
    hideMentionPicker();
    try {
      if (!clientRef.value) {
        throw new Error("房间信令尚未连接。");
      }
      await clientRef.value.sendChatMessage(signal.content, signal.request_id, signal.mentions ?? []);
      return true;
    } catch (error) {
      chatInput.value = signal.content;
      selectedMentions.value = signal.mentions ?? [];
      onError(error.message || "聊天消息发送失败。");
      return false;
    }
  }

  function disposeChatSession() {
    clearMentionReminder();
    clearChatToast();
  }

  return {
    activateChatPanel,
    chatInput,
    chatMessages,
    chatToast,
    clearChatToast,
    clearMentionReminder,
    disposeChatSession,
    handleChatMessage,
    hideMentionPicker,
    mentionPickerIndex,
    mentionPickerMembers,
    mentionReminder,
    rememberChatMessages,
    renderMentionPicker,
    selectMention,
    setMentionPickerIndex,
    showMentionReminder,
    submitChatMessage,
    unreadBadgeLabel,
  };
}
