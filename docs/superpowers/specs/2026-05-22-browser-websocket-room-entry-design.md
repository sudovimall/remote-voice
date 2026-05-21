# Browser WebSocket Room Entry 设计文档

日期：2026-05-22

## 目标

把浏览器创建房间和加入房间流程统一切到 WebSocket，确保房间成员的创建、连接绑定和成员广播都走同一条实时通道。

同时为浏览器端抽出可复用的 WebSocket 信令客户端，后续麦克风静音、房主权限、WebRTC 协商和其他房间事件继续复用这一层。

## 范围

### 本阶段做

- WebSocket 协议新增创建房间消息。
- 浏览器创建房间和加入房间都由房间页 WebSocket 执行。
- 大厅页持久化昵称到 `localStorage` 并准备一次性进入意图。
- 房间页读取进入意图，打开 WebSocket，发创建或加入消息。
- 创建成功后把 `/rooms/new` 替换成真实房间 URL。
- 抽出浏览器 WebSocket 信令客户端模块，集中处理连接、请求 ID、JSON 消息和消息分发。
- 保持房间成员广播继续由 WebSocket 房间事件驱动。

### 本阶段不做

- 不实现刷新后的成员恢复和自动重连。
- 不把 `member_id` 作为长期浏览器身份持久化。
- 不改房主断开时关闭房间的 MVP 规则。
- 不把浏览器切成单页应用。
- 不提前接入新的麦克风、WebRTC 或权限 UI 功能。

## 方案选择

### 采用：房间页持有 WebSocket

大厅页只收集昵称和目标动作：

- 创建房间时进入 `/rooms/new`。
- 加入房间时进入 `/rooms/:room_id`。

房间页打开 WebSocket 后执行真正的 `create_room` 或 `join_room`。创建成功后，房间页用真实房间 ID 替换地址栏中的 `/rooms/new`。

当前后端把成员生命周期绑定到 WebSocket：成员 socket 关闭会离开房间，房主 socket 关闭会关闭房间。创建动作如果在大厅页 WebSocket 上执行，跳转房间页时会立刻断开刚创建的房主连接，需要额外设计连接迁移或断线宽限期。把创建和加入都放到房间页，可以沿用现有生命周期规则，不引入额外恢复语义。

### 未采用：大厅页先 WebSocket 创建或加入

这个方案能让大厅页直接显示创建或加入错误，但页面跳转会关闭大厅 WebSocket。要保持成员身份和房主房间不丢失，需要新增跨 socket 恢复流程，超出当前目标。

### 未采用：浏览器单页切换大厅和房间

这个方案能复用同一个 WebSocket 连接完成创建和房间内工作，但会改变已确定的大厅页和房间页边界，前端改动面更大。

## 协议设计

### 客户端消息

新增：

- `create_room`
  - `request_id`
  - `nickname`

保留：

- `join_room`
  - `request_id`
  - `room_id`
  - `nickname`
  - `member_id` 可选字段继续只服务已有成员绑定流程，当前浏览器进入流程不依赖它。

后续房间内控制和媒体协商消息仍复用现有 WebSocket 协议。

### 服务端消息

`create_room` 和新成员 `join_room` 成功后都返回：

- `joined_room`
  - `request_id`
  - `room`
  - `member_id`

已有成员广播继续保持：

- 新成员加入时向在线成员发送 `member_joined`。
- 成员离开时发送 `member_left`。
- 成员属性更新时发送 `member_updated`。
- 房主离开后发送 `room_closed`。

这样房间页可以把创建和加入成功都收敛成同一个状态入口，在线房主也能通过同一 WebSocket 收到成员加入快照。

## 前端边界

### 大厅页

大厅页负责：

- 回填昵称输入。
- 校验昵称和加入房间号。
- 保存昵称到本地持久存储。
- 写入一次性进入意图。
- 导航到目标房间页。

大厅页不负责：

- 创建成员身份。
- 建立房间内信令连接。
- 保存 WebSocket 创建或加入响应。

### 房间页

房间页负责：

- 从 URL 和一次性进入意图确定本次动作。
- 打开房间 WebSocket。
- 发 `create_room` 或 `join_room`。
- 收到 `joined_room` 后渲染房间快照。
- 创建成功后更新 URL。
- 继续消费成员和房间广播事件。

如果 URL 缺少房间号，或者房间页没有对应进入意图，本阶段显示本地错误并引导用户回大厅重新进入。

## 浏览器存储

### 昵称

昵称放入 `localStorage`，键名固定为：

- `remote-voice.nickname`

保存值是用户提交时 trim 后的昵称字符串。大厅页加载时读取它作为昵称输入默认值。

### 进入意图

创建或加入动作放入 `sessionStorage`，键名固定为：

- `remote-voice.room-entry-intent`

创建意图：

```json
{
  "mode": "create",
  "nickname": "房主"
}
```

加入意图：

```json
{
  "mode": "join",
  "roomId": "ABC123",
  "nickname": "队友"
}
```

房间页只接受和当前 URL 相符的加入意图。收到 `joined_room` 后清理该意图，避免刷新时重复创建或重复加入。

当前阶段不把 `member_id` 写入 `localStorage`。如果后续要支持刷新和断线重连，需要单独定义成员恢复凭据和服务端恢复规则。

## 浏览器 WebSocket 客户端

新增可复用的信令客户端模块，职责包括：

- 生成请求 ID。
- 根据当前页面协议选择 `ws:` 或 `wss:` URL。
- 负责 WebSocket 建连和发 JSON 消息。
- 按 `request_id` 把直接响应路由回发起方。
- 把 `member_joined`、`member_left`、`member_updated`、`room_closed`、媒体协商事件等广播事件交给订阅者。
- 把 socket 连接错误、关闭和无法解析的消息交给 UI 层处理。

房间 UI 继续负责房间状态渲染，不把 DOM 逻辑放进信令客户端。

## HTTP 边界

删除以下 HTTP 写入口，避免房间成员写入同时存在 HTTP 和 WebSocket 两条入口：

- `POST /api/rooms`
- `POST /api/rooms/:room_id/join`

`GET /api/rooms/:room_id` 继续保留为查询接口。

## 错误处理

大厅页只显示本地输入和进入准备错误：

- 昵称为空。
- 加入房间号为空或格式整理失败。
- 本地存储不可用时仍可继续跳转，但显示稳定错误提示。

房间页显示房间动作和信令错误：

- WebSocket 连接失败或关闭。
- `create_room` 或 `join_room` 返回 `error`。
- 收到无法解析的信令消息。
- 房间地址或进入意图不匹配。

创建或加入失败时，房间页不伪造成功房间状态。已经进入房间后收到 `room_closed` 时，沿用房间关闭状态并停止继续渲染在线房间状态。

## 测试

### Rust

WebSocket 集成测试覆盖：

- `create_room` 返回 `joined_room`，响应带房间和当前成员 ID。
- 房主 WebSocket 创建房间后，另一个成员通过 `join_room` 加入，房主收到 `member_joined`。
- 现有新成员加入、成员绑定和房间事件测试继续通过。

HTTP 路由测试只保留仍受支持的查询和页面资源行为。

### 前端模块

Node 模块测试覆盖：

- 昵称 `localStorage` 的加载和保存。
- 创建与加入进入意图的序列化、读取和 URL 匹配。
- `create_room` 与 `join_room` 消息构造。
- 信令客户端请求响应配对和广播事件分发的核心逻辑。

### 浏览器检查

人工检查覆盖：

- 大厅昵称重新打开后能从 `localStorage` 回填。
- 创建房间从大厅进入 `/rooms/new` 后成功连接，并把地址替换为真实房间号。
- 第二个房间页加入同一房间后，房主页面成员列表更新。
- 加入不存在房间时显示房间页错误，不展示假成员状态。

## 后续工作

- 定义刷新和断线重连语义。
- 把本地静音、房主权限和 WebRTC 协商迁入统一信令客户端。
- 视前端复杂度决定是否继续保持原生模块或引入更完整的客户端状态管理。
