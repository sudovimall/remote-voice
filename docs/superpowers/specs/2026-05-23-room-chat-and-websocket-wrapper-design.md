# 房间文字聊天与 WebSocket 封装设计

日期：2026-05-23

## 目标

为现有语音房间增加文字聊天：每个房间在内存中保存最近 N 条聊天历史，默认 100 条且可配置。前端在成员面板右上角使用三角形切换按钮在“成员”和“聊天”之间切换；按钮显示未读红点数量。实现过程中顺手把前端 WebSocket 房间协议封装到房间语义客户端里，减少 `room.js` 直接处理裸信令的数量。

## 范围

本次实现：

- 房间内存聊天历史。
- `room.chat_history_limit` 配置，默认 `100`。
- WebSocket 聊天协议。
- 前端房间协议封装模块。
- 聊天窗口、消息列表、输入框、未读数量、头像、昵称和时间展示。

本次不做：

- 持久化聊天历史。
- 私聊、图片、表情、文件、Markdown 或富文本。
- 管理员删除消息。
- 敏感词、垃圾消息防刷和账号级认证。

## 后端设计

`RoomStore` 继续作为房间内存状态的真源。每个 `Room` 新增聊天历史字段，但不通过普通 `Room` 快照公开给所有成员，避免每次成员状态更新都携带聊天历史。

新增模型：

```rust
pub struct ChatMessage {
    pub id: String,
    pub room_id: String,
    pub member_id: String,
    pub nickname: String,
    pub content: String,
    pub sent_at_epoch_millis: u64,
}

pub struct ChatMessageList {
    pub messages: Vec<ChatMessage>,
}
```

`Member` 的昵称会在发送消息时复制进 `ChatMessage`。这样成员离线或离开后，历史消息仍能显示当时的昵称和头像首字。

`RoomStore` 新增：

- `chat_history_limit: usize`
- `send_chat_message(room_id, member_id, content) -> Result<ChatMessage>`
- `chat_history(room_id) -> Result<Vec<ChatMessage>>`

校验规则：

- 必须是房间内成员。
- `content.trim()` 后不能为空。
- 最大长度为 500 个 Unicode scalar 字符。
- 保存 trim 后内容。
- 历史超过 `chat_history_limit` 时删除最旧消息。

消息 ID 使用不可猜随机后缀，格式类似 `c_<random>`。时间使用 Unix epoch 毫秒，便于前端本地格式化。

## 配置设计

`application.yaml` 增加：

```yaml
room:
  max_members: 8
  disconnect_grace_seconds: 30
  chat_history_limit: 100
```

`Settings::Display` 的中文配置日志也展示聊天历史条数，例如：

```text
聊天历史条数 = 100
```

测试覆盖默认值和自定义值。

## WebSocket 协议

新增客户端消息：

```json
{
  "type": "send_chat_message",
  "request_id": "chat-1",
  "content": "晚上打哪张图？"
}
```

新增服务端消息：

```json
{
  "type": "chat_message_sent",
  "request_id": "chat-1",
  "message": {
    "id": "c_abc123",
    "room_id": "ABC123",
    "member_id": "m_owner",
    "nickname": "房主",
    "content": "晚上打哪张图？",
    "sent_at_epoch_millis": 1779465600000
  }
}
```

```json
{
  "type": "chat_message",
  "message": {
    "id": "c_abc123",
    "room_id": "ABC123",
    "member_id": "m_owner",
    "nickname": "房主",
    "content": "晚上打哪张图？",
    "sent_at_epoch_millis": 1779465600000
  }
}
```

`joined_room` 新增：

```json
{
  "chat_messages": []
}
```

语义：

- 发送者收到 `chat_message_sent`，用于确认服务端采用的 ID、时间和清洗后的内容。
- 同房间其他在线成员收到 `chat_message`。
- `joined_room.chat_messages` 返回最近 N 条历史；创建房间时为空，加入或恢复时返回当前房间历史。
- 历史消息不计入未读。

## 前端 WebSocket 封装

保留 `static/signaling-client.mjs` 作为底层通用客户端，负责连接、JSON、`request_id` pending 和底层错误。

新增 `static/room-connection.mjs`，作为房间语义客户端：

- `connect()`
- `enter(intent)`
- `leave()`
- `setSelfMuted(selfMuted)`
- `setMemberCanSpeak(memberId, canSpeak)`
- `setMemberListening(memberId, listening)`
- `sendChatMessage(content)`
- `sendWebrtcOffer(sdp)`
- `sendIceCandidate(candidate)`
- `onRoomChanged(listener)`
- `onChatMessage(listener)`
- `onListeningState(listener)`
- `onMediaSignal(listener)`
- `onError(listener)`
- `onClose(listener)`

`room.js` 只保留 UI 编排和媒体协调。它不再直接分发大部分裸 `signal.type`，而是通过 `RoomConnection` 订阅房间事件、聊天事件、媒体事件和错误事件。

## 聊天 UI

沿用现有两栏房间页面，不新增第三栏。左侧成员面板的标题区增加右上角三角形切换按钮：

- 成员视图时按钮文字为 `聊天`。
- 聊天视图时按钮文字为 `成员`。
- 有未读消息时，按钮上显示红色圆点数字。
- 三角形样式使用 CSS 实现，不使用图片资源。

未读规则：

- 当前在成员视图时，收到其他成员新消息，未读数加一。
- 当前在聊天视图时，新消息不增加未读并自动滚到底部。
- 自己发送成功的消息不计未读。
- `joined_room.chat_messages` 中的历史消息不计未读。
- 切换到聊天视图时未读数清零。

聊天窗口：

- 消息列表在上方，输入区固定在底部。
- 每条消息显示头像、昵称、时间和正文。
- 头像复用当前成员头像逻辑：昵称首字。
- 时间当天显示 `HH:mm`，跨天显示 `MM-DD HH:mm`。
- 自己消息靠右或使用浅绿色强调底色；其他成员消息靠左。
- 空聊天状态显示 `还没有消息`。
- 输入框最大 500 字符。
- Enter 发送，Shift+Enter 换行。
- 发送失败使用现有 `room-error` 显示错误。

视觉风格保持当前项目的安静工具界面：紧凑消息流、小方形头像、8px 圆角消息块、清晰分隔，不做社交软件式夸张气泡。

## 错误处理

后端新增错误仍使用现有 `invalid_message`：

- 未加入房间发送聊天。
- 空消息。
- 超过 500 字符。
- 成员不存在。

前端发送失败时不插入本地临时消息，等服务端确认后再追加。这样不会出现失败消息和服务端消息重复的问题。

## 测试策略

后端：

- 配置默认 `chat_history_limit = 100`。
- 自定义历史条数可解析并进入配置日志展示。
- 房间领域发送聊天、清洗空白、拒绝空消息、拒绝超长消息、历史截断、昵称快照。
- WebSocket 发送聊天后发送者收到 `chat_message_sent`，其他成员收到 `chat_message`。
- 加入或恢复房间时 `joined_room.chat_messages` 返回历史。

前端：

- `RoomConnection` 分发房间事件、聊天事件、媒体事件。
- `RoomConnection.sendChatMessage()` 构造正确协议并返回确认消息。
- 聊天 helper 格式化时间、裁剪输入、判断未读。
- `room-controls` 和现有状态测试保持通过。

手动检查：

- 成员面板右上角三角按钮可切换成员/聊天。
- 收到消息时未读数字出现，进入聊天后清零。
- 消息展示头像、昵称、时间和正文。
- 窄屏布局不重叠。

## 开放边界

当前聊天历史是房间内存状态，服务重启后丢失。后续如果需要持久化，可以把 `ChatMessage` 和 `RoomStore` 的历史读写替换为数据库仓储；WebSocket 协议和前端 `RoomConnection` 不需要大改。
