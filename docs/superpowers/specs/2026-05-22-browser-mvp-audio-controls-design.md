# Browser MVP Audio Controls 设计文档

日期：2026-05-22

## 目标

把 Remote Voice 推进到 MVP 初步可用状态：

- 浏览器进入房间后请求麦克风权限。
- 浏览器通过 WebRTC 与 Rust SFU 建立音频会话。
- 房间成员可以本地静音并同步成员状态。
- 房主可以控制成员是否允许发言。
- 页面能展示媒体、成员和权限的基本错误状态，支持联调验证。

本规格建立在现有 WebSocket 房间入口和后端 SFU 媒体能力之上，不把部署文档、守护进程配置和生产反向代理说明拉入本轮。

## 范围

### 本阶段做

- 抽出浏览器媒体会话模块，负责麦克风、PeerConnection、offer/answer、ICE、远端音轨和重新协商。
- 房间页在 `joined_room` 后启动媒体会话。
- 本地静音同时控制浏览器上行音轨和 WebSocket 成员状态。
- 房主成员列表提供发言权限控制。
- 成员列表继续以服务端房间快照为准，响应 `member_updated`。
- 房间关闭或页面离开时关闭媒体会话并停止本地音轨。
- 增加可自动化的媒体状态和房间控制测试。
- 使用假麦克风 Playwright 流程联调创建、加入、WebRTC 连接、静音和禁言状态。

### 本阶段不做

- 不实现账号系统和持久化成员身份。
- 不实现刷新后的成员恢复或断线自动重连。
- 不实现视频、屏幕共享、聊天、录音、房间密码。
- 不实现生产部署文档、HTTPS 反向代理示例或 `systemd` 配置。
- 不在浏览器侧实现音频混音或复杂音量控制。

## 方案选择

### 采用：房间 UI 与媒体会话模块分离

保留当前 `SignalingClient` 作为 WebSocket 传输层，新增浏览器媒体会话模块负责 WebRTC：

- `room.js` 继续负责 DOM、房间快照、错误展示、静音按钮和房主权限按钮。
- `media-session.mjs` 负责 `getUserMedia`、`RTCPeerConnection`、offer/answer、ICE、远端音轨播放和重新协商。
- 房间页面通过清晰回调读取媒体状态，不直接散落 PeerConnection 细节。

这个分层把 WebSocket 房间状态和 WebRTC 媒体协商分开，便于测试控制消息和定位 ICE、SDP、自动播放等联调问题。

### 未采用：继续把媒体逻辑堆入 `room.js`

这个方案少一个模块，但会把 DOM 渲染、房间权限和浏览器 WebRTC 状态写在同一文件。后续 ICE、重新协商和 UI 变更会互相干扰。

### 未采用：只打通音频再补控制

本地静音和房主禁言已经有后端领域与 WebSocket 协议支持。MVP 要达到可用状态时，浏览器不应只留下音频传输而缺少基本控制面。

## 浏览器媒体会话

### 启动流程

房间页收到成功 `joined_room` 后启动媒体会话：

1. 调用 `navigator.mediaDevices.getUserMedia({ audio: true })` 请求麦克风。
2. 创建 `RTCPeerConnection`。
3. 把本地 audio track 加入连接。
4. 创建 offer 并设置本地描述。
5. 通过 `SignalingClient.request()` 发送 `webrtc_offer`。
6. 收到 `webrtc_answer` 后设置远端描述。
7. 继续通过 WebSocket 发送和接收 ICE candidate。

媒体会话只使用浏览器 WebRTC 媒体通道传输音频。WebSocket 只承载协商和房间控制 JSON。

### ICE

- 浏览器产生本地 candidate 时发送 `ice_candidate`。
- 服务端 `ice_candidate` 事件进入媒体会话并调用 `addIceCandidate`。
- candidate 添加失败要进入媒体错误展示，不静默吞掉。

### 下行音频

- `RTCPeerConnection.ontrack` 接收后端下发的远端 audio track。
- 媒体会话为远端 stream 创建或复用带 `autoplay` 的 `<audio>` 节点。
- 页面不提供播放器 UI；音频节点只承担播放。
- 自动播放失败时保留连接，向房间 UI 上报可见错误。

### 重新协商

当房间页收到 `renegotiation_needed`：

- 转给媒体会话触发新的 offer。
- 重新协商按串行队列执行，不能并发创建多个 offer。
- 新的 answer 继续走现有 WebSocket `webrtc_offer` 请求响应路径。

这用于后端在其他成员出现新上行 track 后给听众挂新的下行 track。

## 房间控制

### 本地静音

媒体启动成功后，当前成员可切换静音：

- 浏览器本地 audio track `enabled` 跟随静音按钮切换。
- 房间页发送 `set_self_muted`。
- 成员列表等服务端 `member_updated` 快照回来后更新静音状态。

静音是客户端主动关闭自己的上行轨道；后端房间状态用于其他成员看到静音标记。

### 房主发言权限

成员列表按当前房间快照判断操作权限：

- 当前成员是房主时，其他成员显示可操作的“禁言”或“允许发言”控制。
- 房主不对自己显示发言权限操作。
- 普通成员只看到发言状态，不获得可用权限按钮。

房主点击后发送：

- `set_member_can_speak`
  - `member_id`
  - `can_speak`

服务端仍是权限真源：

- `RoomStore` 判断操作者是否房主。
- 媒体层根据 `can_speak` 丢弃被禁言成员上行 RTP。
- 浏览器只在收到 `member_updated` 快照后刷新成员状态。

### 自己被禁言

如果当前成员 `can_speak = false`：

- 成员列表显示禁言。
- 本地语音区显示房主已禁言。
- 本地麦克风权限和 PeerConnection 可以保持存在；真正音频转发以后端权限判断为准。

## UI 状态

房间本地语音区增加稳定状态位：

- 设备状态：
  - 未请求权限。
  - 请求中。
  - 已授权。
  - 权限被拒绝。
- 媒体状态：
  - 等待连接。
  - 协商中。
  - 已连接。
  - 连接失败。
- 下行音频：
  - 等待其他成员音轨。
  - 已收到远端音轨。
  - 播放异常。

控制状态：

- 静音按钮在本地媒体就绪后可用。
- 断开按钮继续离开房间并清理媒体。
- 房主权限按钮按成员和角色启用，按钮文案反映下一步操作。

## 错误处理

### 麦克风权限失败

- 房间加入保持有效。
- 页面显示权限失败。
- 不继续发起 WebRTC offer。
- 用户仍可看到成员列表和房间关闭事件。

### 协商失败

- offer、answer 或 ICE 处理失败时展示媒体失败状态。
- WebSocket 房间连接可继续存在，方便用户看到成员状态和错误。

### 自动播放失败

- 保留已建立媒体连接。
- 下行音频状态显示播放异常。
- 不把自动播放错误伪装成房间断开。

### 房间关闭和离开

- 收到 `room_closed` 时关闭媒体会话，停止本地 track，移除远端音频节点。
- 页面 `pagehide` 和显式离开也执行媒体清理。

## 模块边界

- `static/signaling-client.mjs`
  - 继续承载 WebSocket 请求响应和广播事件分发。
- `static/media-session.mjs`
  - 新增媒体会话、ICE、重新协商、远端音轨播放。
- `static/room-controls.mjs`
  - 新增 DOM 无关的房间控制消息与按钮权限判断。
- `static/room.js`
  - 组合房间 UI、`SignalingClient`、`MediaSession` 和 `room-controls`。
- `static/room.html`
  - 暴露设备、媒体、下行状态节点和可操作静音/断开控件。

## 测试

### Node 模块测试

覆盖：

- 媒体会话发送 `webrtc_offer`、`ice_candidate` 所需的信令边界。
- `renegotiation_needed` 触发的重新协商串行控制。
- `set_self_muted` 和 `set_member_can_speak` 控制消息构造。
- 房主权限按钮启用逻辑。

媒体模块测试使用可控的 PeerConnection、mediaDevices 和 audio 元素替身，不依赖真实浏览器设备。

### Rust 测试

保留现有覆盖：

- WebSocket offer/answer。
- ICE candidate。
- 重新协商事件。
- 房主发言权限。
- 禁言后 RTP 不转发。

如果前端联调暴露协议缺口，先补失败测试再修服务端。

### Playwright 联调

使用假麦克风自动检查：

1. 房主创建房间。
2. 第二成员加入。
3. 两页都显示媒体连接已建立。
4. 静音按钮修改当前成员静音状态。
5. 房主禁言第二成员，双方成员列表显示禁言状态。
6. 桌面和窄屏视口没有控制重叠，控制台无相关错误。

### 手工音频验收

自动化检查只能证明协商和状态流。真实可听性需要再做一次手工确认：

- 两个浏览器或设备进入同一房间。
- 一端说话，另一端能听到。
- 房主禁言后听众不再听到该成员。
- 重新允许发言后音频恢复。

## 完成标准

这轮完成后，Remote Voice MVP 满足：

- 用户能输入昵称创建或加入房间。
- 房主和成员通过 WebSocket 看见实时成员状态。
- 浏览器通过 WebRTC 与后端 SFU 建立音频链路。
- 用户可以本地静音。
- 房主可以禁言和恢复成员发言权限。
- 自动化测试与 Playwright 联调覆盖主要控制流。
