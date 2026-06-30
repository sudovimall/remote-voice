import { expect, test } from "@playwright/test";

// 在浏览器上下文安装只影响 P2P 会话的测试夹具，保留 SFU MediaSession 使用真实浏览器实现。
function installP2PHarness() {
  if (window.__remoteVoiceP2PTest) {
    return;
  }

  // 生成稳定的视频流，供 fake P2P 和屏幕共享 API 在无真实设备时使用。
  function makeVideoStream(label) {
    const canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 180;
    const context = canvas.getContext("2d");
    context.fillStyle = label === "screen" ? "#1f7a8c" : "#7c3aed";
    context.fillRect(0, 0, canvas.width, canvas.height);
    context.fillStyle = "#ffffff";
    context.font = "24px sans-serif";
    context.fillText(label, 24, 96);
    return canvas.captureStream(2);
  }

  navigator.mediaDevices.getDisplayMedia = async () => makeVideoStream("screen");

  // 模拟元数据 DataChannel，覆盖摄像头和屏幕轨道来源同步所需的最小行为。
  class FakeDataChannel {
    // 创建立即可用的通道，避免测试等待真实 SCTP 连接建立。
    constructor(label) {
      this.label = label;
      this.readyState = "open";
      this.listeners = new Map();
      this.sent = [];
    }

    // 记录单个事件监听器，满足 P2PMediaSession 对 open/message 的监听需求。
    addEventListener(type, listener) {
      this.listeners.set(type, listener);
    }

    // 保存本端发送内容，便于后续排查但不参与协议转发。
    send(message) {
      this.sent.push(message);
    }

    // 主动投递远端元数据消息，让 P2P 会话能把视频轨道归类到摄像头或屏幕。
    emitMessage(message) {
      this.listeners.get("message")?.({ data: JSON.stringify(message) });
    }
  }

  // 模拟浏览器 P2P PeerConnection，只验证应用层信令、轨道同步和回退逻辑。
  class FakeP2PPeerConnection {
    // 初始化 PeerConnection 状态，测试里不创建真实 ICE/DTLS 连接。
    constructor() {
      this.listeners = new Map();
      this.senders = [];
      this.remoteTrackKeys = new Set();
      this.dataChannel = null;
      this.localDescription = null;
      this.remoteDescription = null;
      this.connectionState = "new";
      this.iceConnectionState = "new";
      this.closed = false;
    }

    // 记录事件监听器，覆盖 icecandidate、track、datachannel 和状态变更回调。
    addEventListener(type, listener) {
      this.listeners.set(type, listener);
    }

    // 创建立即 open 的元数据通道，保持和真实通道打开后的行为一致。
    createDataChannel(label) {
      this.dataChannel = new FakeDataChannel(label);
      return this.dataChannel;
    }

    // 保存本地轨道 sender，使 fake SDP 能携带当前媒体来源。
    addTrack(track, stream) {
      const sender = {
        track,
        stream,
        removed: false,
        async replaceTrack(nextTrack) {
          this.track = nextTrack;
          this.removed = !nextTrack;
        },
      };
      this.senders.push(sender);
      return sender;
    }

    // 标记 sender 已移除，后续 fake SDP 不再暴露该轨道。
    removeTrack(sender) {
      sender.removed = true;
    }

    // 生成携带本地轨道清单的 offer，服务端仍会按真实 P2P 信令转发。
    async createOffer() {
      return { type: "offer", sdp: this.fakeSdp("offer") };
    }

    // 生成携带本地轨道清单的 answer，覆盖 answer 侧视频同步路径。
    async createAnswer() {
      return { type: "answer", sdp: this.fakeSdp("answer") };
    }

    // 保存本地描述并派发一个 fake ICE candidate，用来验证后端 candidate 转发。
    async setLocalDescription(description) {
      this.localDescription = description;
      queueMicrotask(() => {
        this.listeners.get("icecandidate")?.({
          candidate: {
            candidate: "candidate:p2p-browser",
            sdpMid: "0",
            sdpMLineIndex: 0,
            toJSON() {
              return {
                candidate: "candidate:p2p-browser",
                sdpMid: "0",
                sdpMLineIndex: 0,
              };
            },
          },
        });
      });
    }

    // 应用远端描述后立即投递远端视频轨道，模拟真实 P2P 媒体到达。
    async setRemoteDescription(description) {
      this.remoteDescription = description;
      await this.dispatchRemoteTracks(description?.sdp);
    }

    // 记录收到的 ICE candidate，确保应用层可以完整走 addIceCandidate 边界。
    async addIceCandidate(candidate) {
      this.lastCandidate = candidate;
    }

    // 关闭 fake 连接，供成员离开或回退 SFU 时验证清理路径。
    close() {
      this.closed = true;
      this.connectionState = "closed";
    }

    // 把本地 sender 压缩成 JSON SDP，便于另一个页面恢复媒体来源。
    fakeSdp(descriptionType) {
      const tracks = this.senders
        .filter((sender) => sender.track && !sender.removed)
        .map((sender) => ({
          id: sender.track.id,
          kind: sender.track.kind,
          source: sender.track.__remoteVoiceP2PSource || sender.track.kind,
        }));

      return JSON.stringify({
        remoteVoiceFakeP2P: true,
        descriptionType,
        tracks,
      });
    }

    // 根据远端 fake SDP 创建对应视频轨道，并同步 DataChannel 元数据后触发 track 事件。
    async dispatchRemoteTracks(rawSdp) {
      let parsed;
      try {
        parsed = JSON.parse(rawSdp);
      } catch (_error) {
        return;
      }
      if (!parsed?.remoteVoiceFakeP2P) {
        return;
      }

      for (const remoteTrack of parsed.tracks ?? []) {
        if (!["camera", "screen"].includes(remoteTrack.source)) {
          continue;
        }
        const key = `${remoteTrack.source}:${remoteTrack.id}`;
        if (this.remoteTrackKeys.has(key)) {
          continue;
        }
        this.remoteTrackKeys.add(key);
        const stream = makeVideoStream(remoteTrack.source);
        const [track] = stream.getVideoTracks();
        this.dataChannel?.emitMessage({
          type: "media_metadata",
          tracks: [
            {
              track_id: track.id,
              source: remoteTrack.source,
            },
          ],
        });
        this.listeners.get("track")?.({
          track,
          streams: [stream],
        });
      }
    }
  }

  window.__remoteVoiceP2PTest = {
    events: [],
    sessions: [],
    PeerConnectionImpl: FakeP2PPeerConnection,
    record(event) {
      this.events.push({ ...event, at: Date.now() });
    },
    registerSession(session) {
      this.sessions.push(session);
    },
    activePeerIds() {
      return this.sessions.flatMap((session) => Array.from(session.peers?.keys?.() ?? []));
    },
    failPeer(memberId, reason = "ice_failed") {
      for (const session of this.sessions) {
        if (session.peers?.has?.(memberId)) {
          session.reportConnectionFailed(memberId, reason);
          return true;
        }
      }
      return false;
    },
  };
}

// 收集页面控制台和运行时错误，避免浏览器级测试只验证 DOM 而漏掉前端异常。
function collectBrowserErrors(page, errors) {
  page.on("console", (message) => {
    if (message.type() === "error") {
      errors.push(message.text());
    }
  });
  page.on("pageerror", (error) => {
    errors.push(error.message);
  });
}

// 登录测试管理员账号，复用浏览器测试服务器默认认证配置。
async function login(page) {
  await page.goto("/");
  if (page.url().includes("/login")) {
    await page.getByLabel("用户名").fill("admin");
    await page.getByLabel("密码").fill("password");
    await page.getByRole("button", { name: "登录" }).click();
  }
  await expect(page.getByRole("heading", { name: "进入语音房间" })).toBeVisible();
}

// 由管理员创建一次性邀请码，浏览器测试用它构造非房主账号加入持久房间。
async function createInvite(request) {
  const response = await request.post("/api/admin/invites", {
    data: { ttl_hours: 1 },
  });
  expect(response.ok()).toBe(true);
  const invite = await response.json();
  return invite.code;
}

// 在独立浏览器上下文注册普通用户，避免同一 admin 账号加入时被恢复为房主。
async function registeredUserPage(browser, baseURL, adminRequest, username) {
  const code = await createInvite(adminRequest);
  const userContext = await browser.newContext({ baseURL });
  await userContext.addInitScript(installP2PHarness);
  const registerResponse = await userContext.request.post("/api/auth/register", {
    data: {
      code,
      username,
      password: "password",
      display_name: username,
    },
  });
  expect(registerResponse.ok()).toBe(true);
  return {
    context: userContext,
    page: await userContext.newPage(),
  };
}

// 创建房间并等待房间 WebSocket 已连接，保证后续 P2P 信令可以收发。
async function createRoom(page, nickname) {
  await page.getByLabel("昵称").fill(nickname);
  await page.locator("#create-room button[type='submit']").click();
  await expect(page).toHaveURL(/\/rooms\/[A-Z0-9]+$/);
  await expect(page.locator("#room-connection")).toHaveText("已连接");
  return new URL(page.url()).pathname.split("/").pop();
}

// 通过大厅加入指定房间，覆盖真实用户入房和成员同步路径。
async function joinRoom(page, roomId, nickname) {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "进入语音房间" })).toBeVisible();
  await page.getByLabel("昵称").fill(nickname);
  await page.getByLabel("房间号").fill(roomId);
  await page.locator("#join-room button[type='submit']").click();
  await expect(page).toHaveURL(new RegExp(`/rooms/${roomId}$`));
  await expect(page.locator("#room-connection")).toHaveText("已连接");
}

// 读取浏览器内测试遥测事件，避免把测试断言耦合到生产 UI 文案。
async function p2pEvents(page) {
  return page.evaluate(() => window.__remoteVoiceP2PTest?.events ?? []);
}

// 等待单个页面出现指定 P2P 事件，用于断言成员视角内的媒体状态。
async function waitForP2PEvent(page, predicate) {
  await expect
    .poll(async () => {
      const events = await p2pEvents(page);
      return events.find(predicate) ?? null;
    })
    .not.toBeNull();
  const events = await p2pEvents(page);
  return events.find(predicate);
}

// 等待任意页面出现指定 P2P 事件，用于跨页面信令收发断言。
async function waitForAnyP2PEvent(pages, predicate) {
  await expect
    .poll(async () => {
      for (const page of pages) {
        const events = await p2pEvents(page);
        const found = events.find(predicate);
        if (found) {
          return found;
        }
      }
      return null;
    })
    .not.toBeNull();
}

// 从 session_created 事件取当前成员 ID，避免浏览器测试依赖私有 Vue 状态。
async function ownMemberId(page) {
  const event = await waitForP2PEvent(page, (entry) => entry.type === "session_created");
  return event.ownMemberId;
}

// 读取当前页面仍然保留的 P2P peer，用于验证资源清理不会留下旧成员连接。
async function activePeerIds(page) {
  return page.evaluate(() => window.__remoteVoiceP2PTest?.activePeerIds?.() ?? []);
}

// 等待指定成员仍有 P2P peer，证明刷新或三人房后其他成员对没有被误关闭。
async function waitForActivePeer(page, memberId) {
  await expect.poll(() => activePeerIds(page)).toContain(memberId);
}

// 等待指定成员的 P2P peer 已释放，覆盖离开、刷新断线和房间关闭清理路径。
async function waitForNoActivePeer(page, memberId) {
  await expect.poll(() => activePeerIds(page)).not.toContain(memberId);
}

// 验证两人 P2P 视频进入 UI，同时验证单个成员对失败后只该 pair 回退 SFU。
test("p2p media reaches browser UI and one failed pair falls back to SFU", async ({ page, context }) => {
  await context.addInitScript(installP2PHarness);
  const browserErrors = [];
  collectBrowserErrors(page, browserErrors);

  await login(page);
  const roomId = await createRoom(page, "浏览器房主");
  await expect(page.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const ownerId = await ownMemberId(page);

  const memberPage = await context.newPage();
  collectBrowserErrors(memberPage, browserErrors);
  await joinRoom(memberPage, roomId, "浏览器队友");
  await expect(memberPage.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const memberId = await ownMemberId(memberPage);

  await waitForAnyP2PEvent([page, memberPage], (entry) =>
    entry.type === "signal_sent" && entry.signal?.type === "p2p_offer",
  );
  await waitForAnyP2PEvent([page, memberPage], (entry) =>
    entry.type === "signal_sent" && entry.signal?.type === "p2p_answer",
  );
  await waitForAnyP2PEvent([page, memberPage], (entry) =>
    entry.type === "signal_sent" && entry.signal?.type === "p2p_ice_candidate",
  );

  await expect(page.locator("#toggle-camera")).toBeEnabled();
  await page.locator("#toggle-camera").click();
  await expect(page.locator("#camera-state")).toHaveText("摄像头已开启");
  await waitForP2PEvent(memberPage, (entry) =>
    entry.type === "remote_video" && entry.memberId === ownerId && entry.source === "camera",
  );
  await expect
    .poll(() => memberPage.locator("#video-grid-panel video").count())
    .toBeGreaterThanOrEqual(1);
  await expect
    .poll(() => memberPage.evaluate(() => Boolean(document.querySelector("#video-grid-panel video")?.srcObject)))
    .toBe(true);

  await page.locator("#screen-tab").click();
  await expect(page.locator("#start-screen-share")).toBeEnabled();
  await page.locator("#start-screen-share").click();
  await memberPage.locator("#screen-tab").click();
  await waitForP2PEvent(memberPage, (entry) =>
    entry.type === "screen_stream_applied" &&
    entry.memberId === ownerId &&
    entry.hasStream === true,
  );
  await expect
    .poll(() => memberPage.evaluate(() => Boolean(document.querySelector("#screen-video")?.srcObject)))
    .toBe(true);

  const thirdPage = await context.newPage();
  collectBrowserErrors(thirdPage, browserErrors);
  await joinRoom(thirdPage, roomId, "浏览器三号");
  await expect(thirdPage.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const thirdId = await ownMemberId(thirdPage);
  await waitForP2PEvent(page, (entry) => entry.type === "peer_created" && entry.memberId === thirdId);

  const forced = await page.evaluate((targetMemberId) => {
    return window.__remoteVoiceP2PTest.failPeer(targetMemberId);
  }, memberId);
  expect(forced).toBe(true);

  await waitForP2PEvent(page, (entry) =>
    entry.type === "route_updated" && entry.memberId === memberId && entry.route === "sfu",
  );
  await waitForP2PEvent(memberPage, (entry) =>
    entry.type === "route_updated" && entry.memberId === ownerId && entry.route === "sfu",
  );
  await expect
    .poll(() => page.evaluate(() => window.__remoteVoiceP2PTest.activePeerIds()))
    .toContain(thirdId);
  await expect
    .poll(() => thirdPage.evaluate(() => window.__remoteVoiceP2PTest.activePeerIds()))
    .toContain(ownerId);

  expect(browserErrors).toEqual([]);
});

// 验证刷新、普通成员离开和房主关闭房间时都会释放对应 P2P 资源。
test("p2p peers are cleaned up across refresh, member leave, and owner close", async ({
  page,
  context,
  browser,
  baseURL,
}) => {
  await context.addInitScript(installP2PHarness);
  const browserErrors = [];
  collectBrowserErrors(page, browserErrors);

  await login(page);
  const roomId = await createRoom(page, "清理房主");
  await expect(page.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const ownerId = await ownMemberId(page);

  const member = await registeredUserPage(browser, baseURL, context.request, "cleanup-member");
  const memberPage = member.page;
  collectBrowserErrors(memberPage, browserErrors);
  await joinRoom(memberPage, roomId, "清理成员");
  await expect(memberPage.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const firstMemberId = await ownMemberId(memberPage);
  await waitForActivePeer(page, firstMemberId);
  await waitForActivePeer(memberPage, ownerId);

  await memberPage.reload();
  await expect(memberPage.locator("#room-connection")).toHaveText("已连接");
  await expect(memberPage.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const refreshedMemberId = await ownMemberId(memberPage);
  await waitForP2PEvent(page, (entry) => entry.type === "peer_closed" && entry.memberId === firstMemberId);
  await waitForNoActivePeer(page, firstMemberId);
  await waitForActivePeer(page, refreshedMemberId);
  await waitForActivePeer(memberPage, ownerId);

  await memberPage.locator("#leave-room").click();
  await expect(memberPage).toHaveURL(/\/$/);
  await expect(memberPage.getByRole("heading", { name: "进入语音房间" })).toBeVisible();
  await waitForP2PEvent(page, (entry) => entry.type === "peer_closed" && entry.memberId === refreshedMemberId);
  await waitForNoActivePeer(page, refreshedMemberId);

  const observer = await registeredUserPage(browser, baseURL, context.request, "cleanup-observer");
  const observerPage = observer.page;
  collectBrowserErrors(observerPage, browserErrors);
  await joinRoom(observerPage, roomId, "清理观察者");
  await expect(observerPage.locator("#media-state")).toHaveText("已连接", { timeout: 15_000 });
  const observerId = await ownMemberId(observerPage);
  await waitForActivePeer(page, observerId);
  await waitForActivePeer(observerPage, ownerId);

  await page.locator("#leave-room").click();
  await expect(page).toHaveURL(/\/$/);
  await expect(observerPage.locator("#room-connection")).toHaveText("房间已关闭");
  await waitForNoActivePeer(observerPage, ownerId);

  expect(browserErrors).toEqual([]);
  await member.context.close();
  await observer.context.close();
});
