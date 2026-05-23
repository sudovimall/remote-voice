# 聊天 @ 提及设计

日期：2026-05-23

## 目标

在现有房间文字聊天里增加结构化 @ 提及。用户可以在聊天输入框中输入 `@` 选择房间成员，发送后消息携带被提及成员的稳定 `member_id`。当前用户被其他成员 @ 时，如果聊天面板未打开，前端显示一个无需确认的提醒浮层，10 秒后自动消失。

## 范围

本次实现：

- 聊天输入框的 @ 成员候选和选择。
- `send_chat_message` 协议增加结构化 `mentions`。
- `ChatMessage` 保存结构化 mentions。
- 服务端校验、去重并保存 mentions。
- 聊天消息正文里的 @ 高亮。
- 被别人 @ 且未打开聊天面板时显示 10 秒前端提醒。

本次不做：

- 私聊。
- 持久化聊天历史。
- 全局通知中心。
- 浏览器系统通知权限。
- @ 全体成员。
- 昵称编辑或历史消息 mention 重新解析。

## 用户交互

用户在聊天输入框中输入 `@` 后，前端在输入区附近显示当前房间成员候选，不包含自己。候选项展示头像首字和昵称，顺序复用成员列表排序：房主优先，然后按昵称排序。

用户选择候选后，输入框插入 `@昵称 `，并在本地记录这段 mention 对应的 `member_id` 和昵称快照。用户可以继续编辑正文。发送前前端会根据当前输入内容重新校验本地 mentions，只提交仍能在正文中找到对应 `@昵称` 片段的记录。用户删除或改掉该片段后，mention 不再提交。

如果用户只输入普通 `@文字` 且没有通过候选选择成员，它只是普通文本，不会生成结构化 mention。

## 协议设计

客户端 `send_chat_message` 增加可选字段：

```json
{
  "type": "send_chat_message",
  "request_id": "chat-1",
  "content": "@阿木 晚上打哪张图？",
  "mentions": [
    {
      "member_id": "m_xxx",
      "nickname": "阿木"
    }
  ]
}
```

`mentions` 缺省时按空数组处理。字段里的 `nickname` 是前端选择时的显示快照，服务端最终以当前房间成员昵称为准，避免客户端伪造显示名。

服务端 `chat_message_sent`、`chat_message` 和 `joined_room.chat_messages` 中的 `ChatMessage` 增加同样的结构：

```json
{
  "id": "c_abc123",
  "room_id": "ABC123",
  "member_id": "m_owner",
  "nickname": "房主",
  "content": "@阿木 晚上打哪张图？",
  "sent_at_epoch_millis": 1779465600000,
  "mentions": [
    {
      "member_id": "m_xxx",
      "nickname": "阿木"
    }
  ]
}
```

## 后端设计

新增模型：

```rust
pub struct ChatMention {
    pub member_id: String,
    pub nickname: String,
}
```

`ChatMessage` 新增：

```rust
pub mentions: Vec<ChatMention>
```

`RoomStore::send_chat_message` 接收 mentions。校验规则：

- mention 成员必须属于当前房间。
- 不保存 @ 自己。
- 按 `member_id` 去重，保留第一次出现的顺序。
- 最多保存当前房间最大成员数范围内的 mentions；由于房间当前最大人数默认 8，这个限制足够覆盖实际场景。
- `nickname` 使用服务端房间成员当前昵称覆盖客户端传入值。

聊天历史仍保存在房间内存中，随 `ChatMessage` 一起进入历史列表。成员离开后，历史消息仍通过 mention 的昵称快照展示当时被 @ 的名字。

## 前端设计

`static/chat-controls.mjs` 增加纯函数：

- `mentionCandidates(room, ownMemberId)`：返回可 @ 的成员列表。
- `insertMentionText(inputValue, selectionStart, selectionEnd, member)`：生成插入后的文本和光标位置。
- `mentionsForSend(content, selectedMentions)`：发送前过滤、去重并返回结构化 mentions。
- `messageMentionsCurrentMember(message, ownMemberId)`：判断当前用户是否被 @。

`static/room.js` 负责 UI 编排：

- 维护 `selectedMentions`，记录用户通过候选选中的成员。
- 监听聊天输入框键盘输入，遇到 `@` 时显示候选浮层。
- 候选点击或键盘确认后插入 `@昵称 `。
- 提交聊天时把 `content` 和 `mentions` 一起传给 `RoomConnection.sendChatMessage`。
- 渲染消息正文时，将结构化 mentions 对应的 `@昵称` 片段拆成高亮节点，其他文本仍使用 `textContent`，不引入 HTML 注入风险。

候选浮层作为聊天输入区的一部分，不使用浏览器 `prompt`、`alert` 或确认框。

## 提醒规则

当前用户收到一条聊天消息时，如果满足以下条件，显示 @ 提醒浮层：

- 消息不是当前用户自己发送的。
- `message.mentions` 包含当前用户 `member_id`。
- 当前侧栏未打开聊天面板。

提醒浮层显示发送者昵称和消息正文摘要，持续 10 秒后自动消失。若 10 秒内收到新的 @ 我消息，提醒内容更新并重置计时。用户切换到聊天面板时立即清除提醒。聊天面板已打开时不弹提醒，只在消息列表中高亮 @。

提醒是前端临时状态，不进入聊天历史，不需要用户点击确认，也不请求浏览器系统通知权限。

## 错误处理

前端发送前会过滤已删除的 mention 片段。服务端仍作为真源校验 mentions，遇到未知成员或不合法结构时返回现有 `invalid_message` 错误。

如果服务端拒绝发送，前端恢复输入框内容，并保持现有错误展示方式。失败消息不插入本地消息列表，避免和服务端确认消息重复。

## 测试策略

后端：

- `send_chat_message` 缺省 mentions 时兼容旧客户端。
- 有效 mentions 随 `ChatMessage` 保存和序列化。
- mention 未知成员时拒绝。
- mention 自己时不保存。
- 重复 mention 按 `member_id` 去重。
- 服务端使用房间成员昵称覆盖客户端传入昵称。
- WebSocket `send_chat_message` 接收 mentions 并在 `chat_message_sent` / `chat_message` 中返回。

前端：

- @ 候选不包含当前用户，排序符合成员列表规则。
- 选择候选后插入 `@昵称 ` 并生成结构化 mention。
- 删除 mention 文本后发送不再带该 mention。
- `RoomConnection.sendChatMessage(content, requestId, mentions)` 构造正确协议。
- 消息渲染高亮结构化 mentions，普通 `@文字` 不误判为被 @。
- 别人 @ 当前用户且聊天面板未打开时显示提醒并在 10 秒后消失。
- 打开聊天面板时清除提醒。
- 聊天面板已打开时不弹提醒。

手动检查：

- 输入 `@` 后候选浮层位置不遮挡发送按钮。
- 鼠标选择和键盘选择都能插入 mention。
- 窄屏布局下候选浮层、聊天消息和提醒浮层不重叠。
- 连续收到多条 @ 我消息时提醒内容更新且计时重置。

## 后续扩展

结构化 mentions 为后续能力保留接口：只看提到我的消息、@ 全体成员、浏览器系统通知、消息搜索和持久化聊天历史。当前设计不提前实现这些能力。
