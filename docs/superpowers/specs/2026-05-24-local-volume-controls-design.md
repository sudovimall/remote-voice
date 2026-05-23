# 本地音量控制设计

日期：2026-05-24

## 目标

为房间语音增加当前浏览器本地的音量控制：

- 当前用户可以调整每个其他成员的播放音量。
- 当前用户可以调整自己的麦克风输入增益，也就是别人听到自己的音量。
- 设置在刷新页面和断线重连后保留。

## 范围

本次实现：

- 每个其他成员的本地播放音量滑块。
- 当前用户的麦克风输入增益滑块。
- 音量偏好保存到 `localStorage`。
- 远端音频 track 新建或重连后自动应用保存的成员音量。
- 麦克风重新采集或重连后自动应用保存的输入增益。
- 浏览器不支持 Web Audio 时，远端播放音量仍可用，麦克风输入增益降级为禁用状态。

本次不做：

- 服务端保存音量偏好。
- 音量设置同步给其他用户。
- 房间级默认音量。
- 自动音量归一化、压缩器、降噪或回声处理参数调整。
- 音量波形、电平表或语音活动 UI 重设计。

## 用户体验

### 每个成员的播放音量

成员列表中，除当前用户外，每个成员增加一个紧凑音量滑块：

- 范围：`0%` 到 `200%`。
- 默认：`100%`。
- 步进：`5%`。
- 标签显示当前百分比，例如 `100%`。

`0%` 只表示当前浏览器本地静音该成员，不等同于现有“不听”。现有“不听”仍表示让服务端停止给当前用户转发该成员音频。两个控制可以同时存在：

- 想节省接收和转发：用“不听”。
- 只是临时调小或静音：用音量滑块。

### 麦克风输入增益

本地语音面板增加“输入音量”滑块：

- 范围：`0%` 到 `200%`。
- 默认：`100%`。
- 步进：`5%`。
- 标签显示当前百分比。

静音按钮继续控制是否发送麦克风音轨。输入音量只改变音频增益，不改变静音状态。

## 数据与持久化

音量设置只保存在当前浏览器 `localStorage`。

建议 key：

```text
remote_voice_member_volume:v1:<room_id>:<member_id>
remote_voice_microphone_gain:v1
```

播放音量按房间和成员保存，因为同一成员 ID 只在房间内有意义。麦克风输入增益按浏览器全局保存，因为它是当前设备偏好，不依赖房间。

存储值使用 0 到 2 的数字字符串：

- `0` 表示 `0%`
- `1` 表示 `100%`
- `2` 表示 `200%`

读取时做边界裁剪。非法、缺失或不可解析值回退到 `1`。

## 前端架构

### 新增音量 helper

新增 `static/audio-volume.mjs`，负责纯函数和存储访问：

- `clampVolume(value) -> number`
- `volumePercent(value) -> string`
- `memberVolumeKey(roomId, memberId) -> string`
- `loadMemberVolume(storage, roomId, memberId) -> number`
- `saveMemberVolume(storage, roomId, memberId, value) -> void`
- `loadMicrophoneGain(storage) -> number`
- `saveMicrophoneGain(storage, value) -> void`

这样 `room.js` 只负责 UI 编排，`media-session.mjs` 只负责媒体应用。

### MediaSession 远端播放音量

`MediaSession` 增加：

- `setMemberVolume(memberId, volume)`
- `memberVolumes: Map<string, number>`

当远端 track 到达时，继续从 track id 解析 `memberId`。创建或复用 `<audio>` 后，按成员 ID 设置：

```js
audio.volume = clampVolume(memberVolumes.get(memberId) ?? 1);
```

如果音量在音频节点已存在后改变，立即更新当前对应成员的所有 audio 节点。

为了支持一个成员未来可能有多个音轨，`audioNodes` 需要记录音频节点和成员 ID 的关系，而不是只存裸 audio 元素。

### MediaSession 麦克风输入增益

`MediaSession` 增加：

- `setMicrophoneGain(gain)`
- `microphoneGain`
- `microphoneGainSupported`

启动媒体时优先使用 Web Audio：

1. `getUserMedia({ audio: true })` 获取原始麦克风流。
2. 创建 `AudioContext`。
3. `createMediaStreamSource(localStream)`。
4. `createGain()` 并设置 `gain.value`。
5. `createMediaStreamDestination()`。
6. 连接 `source -> gain -> destination`。
7. 将 `destination.stream.getAudioTracks()` 加到 `PeerConnection`。

如果 Web Audio 初始化失败或浏览器不支持所需 API：

- 保持现有行为，直接发送原始麦克风 track。
- `microphoneGainSupported = false`。
- UI 禁用输入音量滑块并显示不可用状态。

关闭媒体时释放 Web Audio 资源：

- 停止原始麦克风流 track。
- 断开节点。
- 关闭 `AudioContext`。

静音逻辑应作用在实际发送的 track 上。如果使用 Web Audio，就是 destination stream 的 track；如果降级，就是原始 local stream 的 track。

## Room UI 编排

`room.js` 维护：

- `memberVolumes: Map<string, number>`
- `microphoneGain: number`

进入房间或渲染成员列表时，从 `localStorage` 读取每个成员音量并传给 `MediaSession`。成员音量滑块变化时：

1. 裁剪值到 `0..2`。
2. 保存到 `localStorage`。
3. 更新 `memberVolumes`。
4. 调用 `mediaSession?.setMemberVolume(memberId, value)`。
5. 重新渲染成员行中的百分比。

本地输入音量滑块变化时：

1. 裁剪值到 `0..2`。
2. 保存到 `localStorage`。
3. 调用 `mediaSession?.setMicrophoneGain(value)`。
4. 更新百分比标签。

`startMedia()` 创建 `MediaSession` 后，立即应用已保存的 `microphoneGain` 和当前成员音量偏好。重连后 `startMedia()` 会重新执行，所以保存值自然恢复。

## 视觉与布局

成员行继续保持紧凑工具界面，不新增卡片嵌套。

建议把音量滑块放在成员状态/控制区域内，样式为一组紧凑控件：

```text
音量 [----|-----] 100%
```

本地语音面板中，输入音量放在设备状态和权限提示附近，作为本地语音控制的一部分。

在窄屏下，滑块可以换行，但不能挤压昵称、发言状态或权限按钮。百分比文本使用固定宽度，避免拖动时布局跳动。

## 错误处理与降级

- `localStorage` 不可用时，使用内存默认值，当前会话仍可调节。
- 读取到非法存储值时回退 `100%`。
- Web Audio 不可用时，禁用输入增益滑块；不阻止加入房间和语音连接。
- 设置远端成员音量时，如果音频节点尚未创建，只保存偏好，等 track 到达后应用。

## 测试策略

前端 helper：

- 音量值裁剪到 `0..2`。
- 百分比格式正确。
- `localStorage` 缺失、非法值和正常值读取正确。
- member volume key 包含 room id 和 member id。

`MediaSession`：

- 远端 track 到达后按 memberId 应用保存音量。
- 音量变化会更新已存在 audio 节点。
- 关闭会移除 audio 节点并释放资源。
- 支持 Web Audio 时，通过 gain destination track 发送音频。
- `setMicrophoneGain()` 更新 GainNode 的 `gain.value`。
- Web Audio 不支持时仍能使用原始麦克风流启动。

`room-controls` / `room-layout`：

- 成员行包含其他成员音量滑块，不给当前用户显示远端播放音量。
- 本地语音面板包含输入音量滑块和百分比。
- 窄屏布局不依赖嵌套卡片，不覆盖已有按钮。

集成/手动检查：

- A 调低 B 的音量，只影响 A 浏览器。
- A 把 B 音量设为 `0%` 后仍能用“不听/接收”切换。
- 刷新或断线重连后成员音量恢复。
- 调整输入音量后，其他成员听到的当前用户音量变化。
- 刷新或断线重连后输入音量恢复。

## 开放边界

`200%` 增益可能造成失真，这是用户可控的本地设置。后续如果需要更好的听感，可以增加压缩器或限制最大值，但本次不做，以免引入额外音频处理复杂度。
