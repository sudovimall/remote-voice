# 屏幕共享设计

日期：2026-05-24

## 目标

为语音房间增加屏幕共享能力：

- 同一个房间同一时间只能有一个成员共享屏幕。
- 共享者只共享画面，不共享系统音频。
- 语音沟通继续使用现有麦克风语音链路。
- 共享画面在“成员 / 聊天 / 共享”三个面板之间切换查看。
- 共享画面支持页面内弹窗观看和浏览器全屏观看。
- 房主可以强制停止当前屏幕共享。

## 范围

本次实现：

- 前端增加“共享”面板，与“成员”“聊天”同级切换。
- 共享面板支持开始共享、停止共享、查看当前共享者、弹窗观看、全屏观看。
- 页面内共享弹窗，不打开新的浏览器窗口。
- 浏览器使用 `getDisplayMedia({ video: true, audio: false })` 采集屏幕画面。
- 服务端保存房间当前屏幕共享占用状态，并强制保证唯一共享者。
- 服务端允许房主停止当前共享者的屏幕共享。
- WebRTC 媒体层支持转发视频 track，但视频发布只允许当前屏幕共享者。
- 共享者停止、断线、离开或房间关闭时释放屏幕共享占用。
- 码率按共享者捕获视频 track 当前分辨率自动估算，不固定 720p 或 1080p。

本次不做：

- 系统音频共享。
- 多人同时共享。
- 屏幕录制、截图、标注、远程控制。
- 独立浏览器窗口弹出。
- 服务端转码、录制、分辨率重采样。
- 共享画面持久化或历史回放。
- TURN/HTTPS 部署策略调整。

## 用户体验

### 面板切换

当前成员/聊天切换扩展为三项：

```text
成员 / 聊天 / 共享
```

切换规则：

- 默认仍进入成员面板。
- 点击“聊天”显示现有聊天面板。
- 点击“共享”显示共享面板。
- 收到新的屏幕共享开始事件时，不强制切换当前用户面板，但“共享”入口应有明显状态。
- 如果当前用户正在共享，离开共享面板不会停止共享。

共享面板空状态：

- 没人共享时显示“当前没有屏幕共享”。
- 可以共享的成员看到“开始共享屏幕”按钮。
- 浏览器不支持 `getDisplayMedia` 时，按钮禁用并显示“当前浏览器不支持屏幕共享”。

共享面板观看状态：

- 显示共享者昵称。
- 显示远端共享视频。
- 显示“弹窗”“全屏”观看按钮。
- 当前共享者看到“停止共享”按钮。
- 房主看到“停止共享”按钮，即使共享者不是自己。
- 普通成员不能停止别人的共享。

### 弹窗与全屏

“弹窗”是页面内浮层：

- 不调用 `window.open()`，避免浏览器拦截和跨窗口媒体状态复杂化。
- 弹窗与共享面板复用同一个远端视频流状态。
- 关闭弹窗只关闭观看浮层，不停止共享。
- 弹窗内保留关闭和全屏按钮。

“全屏”调用浏览器 Fullscreen API：

- 对共享视频容器调用 `requestFullscreen()`。
- 如果当前浏览器不支持或调用失败，显示房间内错误提示。
- 退出全屏不停止共享。

## 权限与房间状态

房间需要新增屏幕共享状态：

```rust
ScreenShareState {
    member_id: MemberId,
    nickname: String,
}
```

房间层负责唯一占用：

- `start_screen_share(member_id)`：
  - 成员必须在房间内。
  - 成员必须在线。
  - 如果已有其他成员共享，拒绝。
  - 如果当前成员已经共享，返回当前状态，保持幂等。
- `stop_screen_share(requester_member_id)`：
  - 当前共享者本人可以停止。
  - 房主可以停止任意当前共享者。
  - 普通成员停止别人共享时拒绝。
  - 没有人共享时保持幂等。
- 成员离开、断线过期、房间关闭时，如果该成员正在共享，自动清理状态。

房间快照需要包含当前屏幕共享状态，供新加入和重连成员恢复 UI。

## 信令设计

新增客户端信令：

```json
{ "type": "start_screen_share", "request_id": "..." }
{ "type": "stop_screen_share", "request_id": "..." }
```

新增服务端信令：

```json
{
  "type": "screen_share_started",
  "member_id": "...",
  "nickname": "..."
}
```

```json
{
  "type": "screen_share_stopped",
  "member_id": "..."
}
```

错误处理：

- 已有其他成员共享时，发送 `error`，文案为“当前已有成员正在共享屏幕。”。
- 普通成员试图停止别人共享时，发送 `error`，文案为“只有共享者或房主可以停止屏幕共享。”。
- 信令携带无效字段时沿用现有 invalid message 处理。

媒体协商：

- 开始共享成功后，前端才采集屏幕并添加 video track。
- 添加 video track 后触发现有 WebRTC offer/answer 重新协商。
- 停止共享时前端停止本地 display track，并重新协商移除视频发送。
- 服务端收到视频 track 后，只接受当前房间屏幕共享者的 track。
- 服务端有新视频下行 track 时，通过现有 `renegotiation_needed` 通知其他成员重新协商。

## 媒体架构

### 前端 MediaSession

现有 `MediaSession` 负责音频采集、音频发送、远端音频播放和重新协商。屏幕共享继续放在 `MediaSession` 内，但与音频状态分离。

新增状态：

- `displayStream`
- `displaySender`
- `remoteVideoNodes`
- `screenShareMemberId`
- `screenShareVideoTrack`

新增方法：

- `canShareScreen()`
- `startScreenShare()`
- `stopScreenShare()`
- `setScreenShareState(state)`
- `requestShareFullscreen(container)`

采集规则：

```js
navigator.mediaDevices.getDisplayMedia({
  video: true,
  audio: false,
});
```

停止规则：

- 用户点击停止共享时，停止 display stream 的所有 track。
- 浏览器共享条里的“停止共享”会触发 display video track 的 `ended` 事件，前端应发送 `stop_screen_share`。
- 页面断开或媒体会话关闭时停止 display track。

码率规则：

- 读取 display video track 的 `getSettings()`。
- 使用 `width * height * fps` 估算 `RTCRtpSender.setParameters().encodings[0].maxBitrate`。
- 分辨率越高，估算码率越高；不主动降分辨率。
- 如果浏览器不支持 `setParameters()` 或设置失败，不阻止共享，只记录错误并继续使用浏览器默认码率。

建议初版估算：

```text
像素数 <= 921600 约 2.5 Mbps
像素数 <= 2073600 约 5 Mbps
更高分辨率约 8 Mbps
```

### 后端媒体层

现有媒体层只接受音频：

```rust
if track.kind() != RTPCodecType::Audio {
    return;
}
```

需要扩展为：

- 音频 track 保持现有逻辑。
- 视频 track 只在发布成员是当前屏幕共享者时接受。
- 非共享者发布视频 track 时忽略并清理。
- 为视频创建独立 fanout track，转发给同房间其他成员。
- 下行 slot 需要区分 audio 和 video，避免视频占用音频槽位。

视频下行策略：

- 房间只有一个视频发布者，所以每个订阅者只需要一个屏幕共享视频下行槽。
- 音频仍保留现有多个成员音频下行槽。
- 新加入成员如果已有共享者，answer 中应包含视频下行能力并接收当前共享。

## 前端 UI 结构

`room.html`：

- 成员/聊天切换按钮扩展为三状态控制。
- 新增 `section#screen-panel`。
- 新增 `div#screen-popout` 页面内浮层。
- 新增远端视频容器和共享控制按钮。

`room.js`：

- 维护 `activeSidePanel`，取值扩展为 `members | chat | screen`。
- 维护 `screenShareState`。
- 处理 start/stop screen share 信令。
- 处理共享面板渲染和弹窗状态。
- 调用 `MediaSession` 开始/停止本地屏幕采集。

`styles.css`：

- 三项切换保持紧凑工具风格。
- 共享视频容器使用固定比例和黑色背景，避免视频未到达时布局跳动。
- 弹窗浮层使用页面内 fixed 布局，移动端占满可视宽度。
- 全屏容器内视频使用 `object-fit: contain`。

## 数据流

开始共享：

1. 用户在共享面板点击“开始共享屏幕”。
2. 前端发送 `start_screen_share`。
3. 服务端校验唯一占用并广播 `screen_share_started`。
4. 当前用户收到成功后调用 `getDisplayMedia({ video: true, audio: false })`。
5. 前端添加 video track 并重新协商。
6. 后端接收共享者 video track，创建 fanout track。
7. 其他成员收到 `renegotiation_needed` 后重新协商并接收视频。
8. 其他成员共享面板显示远端视频。

停止共享：

1. 共享者或房主点击“停止共享屏幕”。
2. 前端发送 `stop_screen_share`。
3. 服务端校验权限并广播 `screen_share_stopped`。
4. 共享者前端停止 display track 并重新协商。
5. 后端移除视频 fanout track，订阅者视频节点清空。

共享者通过浏览器共享条停止：

1. display video track 触发 `ended`。
2. 前端发送 `stop_screen_share`。
3. 后续流程与手动停止一致。

## 错误处理与降级

- `getDisplayMedia` 不存在：禁用开始共享按钮。
- 用户取消共享选择：不占用房间共享状态；如果服务端已占用，前端应立即发送停止共享释放占用。
- 已有成员共享：按钮点击后显示服务端错误。
- 共享 track `ended`：自动停止共享并释放占用。
- 重新协商失败：停止本地 display track，并显示错误提示。
- 远端视频未到达：共享面板保持“正在连接共享画面”状态。
- 房主强停：共享者收到停止事件后停止本地 display track。

## 测试策略

### Rust 单元与集成测试

房间状态：

- 同房间只能一个成员开始共享。
- 同一成员重复开始共享保持幂等。
- 共享者本人可以停止共享。
- 房主可以停止别人共享。
- 普通成员不能停止别人共享。
- 共享者离开或断线过期后释放共享占用。
- 房间快照包含共享状态。

WebSocket 信令：

- `start_screen_share` 成功后广播 `screen_share_started`。
- 第二个成员 `start_screen_share` 收到错误。
- 房主 `stop_screen_share` 会广播 `screen_share_stopped`。
- 普通成员停止别人共享收到错误。
- 新加入成员的 `joined_room` 包含当前共享状态。

媒体层：

- 非共享者发布视频 track 被忽略。
- 当前共享者发布视频 track 会创建视频 fanout。
- 新听众加入后能订阅已有视频共享 track。
- 停止共享后视频 fanout 被清理。
- 音频转发行为保持现有测试通过。

### 前端测试

`MediaSession`：

- `startScreenShare()` 调用 `getDisplayMedia({ video: true, audio: false })`。
- 添加 display video track 后触发重新协商。
- display track `ended` 后调用停止回调。
- `stopScreenShare()` 停止 display stream track。
- 码率估算按 track settings 设置 sender parameters。
- setParameters 失败时不抛出到 UI 主流程。

`room.js` / controls：

- 三面板状态可以在成员、聊天、共享间切换。
- 没人共享时显示开始按钮。
- 已有共享时显示共享者和观看按钮。
- 当前共享者和房主显示停止按钮。
- 普通成员不显示停止别人共享按钮。
- 弹窗打开和关闭不改变共享状态。
- 全屏按钮调用共享容器的 `requestFullscreen()`。

布局：

- 共享面板有稳定视频比例。
- 移动端共享浮层不覆盖关闭按钮。
- 成员、聊天、共享入口文本不挤压或重叠。

### 手动验证

- A 开始共享，B 能在共享面板看到画面。
- B 共享时，A 再点开始共享会收到“当前已有成员正在共享屏幕。”。
- A 是房主时，可以停止 B 的共享。
- C 作为普通成员不能停止 B 的共享。
- B 点击浏览器共享条停止后，所有成员 UI 回到无共享状态。
- 共享中 B 刷新或断线，房间释放共享占用。
- 弹窗关闭和退出全屏都不停止共享。
- 语音麦克风在共享期间保持可用。
