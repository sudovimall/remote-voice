import { computed, ref } from "vue";
import {
  authDisplayName,
  fetchAuthState,
  shouldShowAdminLink,
} from "../lib/auth-ui.js";
import { fetchRoomSummaries } from "../lib/lobby-rooms.js";
import {
  lobbyRoomId,
  loadNickname,
  saveNickname,
  saveRoomEntryIntent,
} from "../lib/room-entry.js";

function defaultWindow() {
  return globalThis.window;
}

function storageFrom(name, deps, win) {
  return deps[name] ?? win?.[name];
}

function defaultFetch(deps, win) {
  if (deps.fetchImpl) {
    return deps.fetchImpl;
  }
  return win.fetch.bind(win);
}

function defaultNavigate(deps, win) {
  return deps.navigate ?? ((path) => win.location.assign(path));
}

export function useLobbySession(deps = {}) {
  const win = deps.window ?? defaultWindow();
  const localStorage = storageFrom("localStorage", deps, win);
  const sessionStorage = storageFrom("sessionStorage", deps, win);
  const location = deps.location ?? win.location;
  const fetchImpl = defaultFetch(deps, win);
  const navigate = defaultNavigate(deps, win);

  const nickname = ref(loadNickname(localStorage));
  const roomId = ref(lobbyRoomId(location.search));
  const errorMessage = ref("");
  const statusMessage = ref("准备进入");
  const rooms = ref([]);
  const roomsLoading = ref(false);
  const roomsMeta = ref("正在读取");
  const roomListMessage = ref("");
  const authState = ref({ enabled: false, user: null });

  const authName = computed(() => authDisplayName(authState.value.user));
  const showAuthControls = computed(() => Boolean(authState.value.enabled && authState.value.user));
  const showAdminLink = computed(() => shouldShowAdminLink(authState.value.user));

  function showError(message) {
    errorMessage.value = message;
  }

  function showPending(message) {
    errorMessage.value = "";
    statusMessage.value = message;
  }

  function submittedNickname() {
    const value = nickname.value.trim();
    if (!value) {
      showError("先输入昵称。");
      return "";
    }

    try {
      nickname.value = saveNickname(localStorage, value);
    } catch (_error) {
      showError("无法保存昵称，请检查浏览器本地存储。");
    }

    return nickname.value;
  }

  function enterRoom(intent, path, pendingMessage) {
    try {
      saveRoomEntryIntent(sessionStorage, intent);
      showPending(pendingMessage);
      navigate(path);
    } catch (entryError) {
      showError(entryError.message || "无法准备房间入口。");
    }
  }

  function joinRoom(nextRoomId) {
    const nicknameValue = submittedNickname();
    if (!nicknameValue) {
      return;
    }

    const normalizedRoomId = nextRoomId.trim().toUpperCase();
    if (!normalizedRoomId) {
      showError("输入房间号后再加入。");
      return;
    }

    roomId.value = normalizedRoomId;
    enterRoom(
      {
        mode: "join",
        roomId: normalizedRoomId,
        nickname: nicknameValue,
      },
      `/rooms/${encodeURIComponent(normalizedRoomId)}`,
      `正在连接房间 ${normalizedRoomId}。`,
    );
  }

  function createRoom() {
    const nicknameValue = submittedNickname();
    if (!nicknameValue) {
      return;
    }

    enterRoom(
      {
        mode: "create",
        nickname: nicknameValue,
      },
      "/rooms/new",
      "正在建立房间连接。",
    );
  }

  function joinEnteredRoom() {
    joinRoom(roomId.value);
  }

  async function refreshRooms() {
    roomsLoading.value = true;
    roomsMeta.value = "正在刷新";
    roomListMessage.value = "";
    try {
      rooms.value = await fetchRoomSummaries(fetchImpl);
      roomsMeta.value = `${rooms.value.length} 个房间`;
      roomListMessage.value = rooms.value.length ? "" : "当前没有房间。";
    } catch (roomsError) {
      rooms.value = [];
      roomsMeta.value = "刷新失败";
      roomListMessage.value = roomsError.message || "房间列表不可用。";
    } finally {
      roomsLoading.value = false;
    }
  }

  async function logout() {
    await fetchImpl("/api/auth/logout", { method: "POST" });
    navigate("/login?next=%2F");
  }

  async function loadAuthState() {
    try {
      authState.value = await fetchAuthState(fetchImpl);
      if (authState.value.user && !nickname.value) {
        nickname.value = authDisplayName(authState.value.user);
      }
    } catch (_authError) {
      authState.value = { enabled: false, user: null };
    }
  }

  function boot() {
    if (win?.document?.body?.dataset) {
      win.document.body.dataset.page = "voice-lobby";
    }
    void loadAuthState();
    void refreshRooms();
  }

  return {
    authName,
    authState,
    boot,
    createRoom,
    errorMessage,
    joinEnteredRoom,
    joinRoom,
    loadAuthState,
    logout,
    nickname,
    refreshRooms,
    roomId,
    roomListMessage,
    rooms,
    roomsLoading,
    roomsMeta,
    showAdminLink,
    showAuthControls,
    statusMessage,
  };
}
