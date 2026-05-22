# 后端逻辑说明

本文档梳理当前 Rust 后端的运行路径、核心状态和房间语音控制逻辑。协议字段、WebRTC、ICE、SDP、RTP、SFU 等固定术语保留英文。

## 启动流程

入口在 `src/main.rs`：

1. `init_log()` 初始化 `tracing_subscriber`，默认日志级别为 `info`，也可通过环境变量覆盖。
2. `init_config()` 从 `application.yaml` 读取配置；如果文件不存在，使用内置默认配置。
3. `app::serve(config)` 根据配置创建共享状态、HTTP 路由并启动 Axum 服务。

启动时后端会输出中文日志：

- `后端配置已加载：[...]`
- `HTTP 服务已启动，监听地址：...`

配置展示文本使用中文字段名，便于直接从日志确认监听端口、房间人数上限、断线保留时间、媒体 UDP 端口范围和对外媒体 IP。

## 共享状态

`AppState` 是后端各层共享的运行时状态：

- `rooms: RoomStore`：内存房间状态和领域规则。
- `signals: SignalHub`：已连接 WebSocket 成员的房间内广播通道。
- `media: MediaController`：后端 WebRTC PeerConnection 和 RTP 转发。
- `disconnect_grace_period`：成员断线后可恢复身份的宽限时间。

状态对象使用 `Arc` 包装，供 Axum handler、WebSocket 任务和媒体事件任务共享。

## HTTP 路由

HTTP 路由在 `src/transport/http/mod.rs` 汇总：

- `GET /`：返回大厅页面。
- `GET /rooms/{room_id}`：返回房间页面。
- `GET /assets/{asset}`：返回编译期内嵌的静态资源。
- `GET /health`：健康检查，返回 `ok`。
- `GET /api/rooms`：返回房间摘要列表。
- `GET /api/rooms/{room_id}`：返回房间完整快照。
- `POST /api/rooms/{room_id}/members/{member_id}/speaking`：HTTP 兼容路径，用房主身份修改成员发言权限。
- `GET /ws`：升级为 WebSocket 信令连接。

当前创建房间、加入房间和恢复身份都走 WebSocket；HTTP 不再负责创建或加入房间。

## 房间领域

`RoomStore` 在内存中保存所有房间。主要结构：

- `Room`：房间 ID、房主成员 ID、成员表、创建时间、最后活跃时间。
- `Member`：成员 ID、昵称、角色、发言权限、本地静音状态、连接状态、恢复凭据和私有不听名单。
- `RoomJoin`：创建、加入或恢复房间时返回的房间快照、成员信息和恢复凭据。

关键规则：

- 创建房间的成员是房主。
- 新成员默认可发言、未静音、已连接。
- 房间人数达到 `room.max_members` 后拒绝加入。
- 只有房主可以修改其他成员的 `can_speak`。
- 成员不能屏蔽自己的语音。
- 成员离开或断线超时被移除时，其他成员私有不听名单里的该成员引用会被清理。
- 房主显式离开或房主断线超时后，整个房间关闭。

成员恢复依赖 `member_id + resume_token`。恢复成功后，原成员会重新标记为已连接，私有不听名单也会保留。

## WebSocket 信令

信令入口是 `src/transport/http/signaling.rs`。每个 WebSocket 连接在 `handle_socket` 内部维护当前连接绑定的 `room_id` 和 `member_id`。

客户端消息：

- `create_room`：创建房间并绑定当前 WebSocket。
- `join_room`：加入已有房间并绑定当前 WebSocket。
- `resume_room`：用恢复凭据重新绑定原成员。
- `leave_room`：显式离开房间。
- `set_self_muted`：更新自己的本地静音状态。
- `set_member_can_speak`：房主修改成员发言权限。
- `set_member_listening`：当前听众停止或恢复接收某个发布者的语音。
- `webrtc_offer`：浏览器发给后端 PeerConnection 的 SDP offer。
- `webrtc_answer`：当前后端不主动发起 offer，因此会拒绝。
- `ice_candidate`：浏览器发给后端 PeerConnection 的远端 ICE candidate。

服务端消息：

- `joined_room`：加入、创建或恢复成功；返回房间快照、当前成员 ID、恢复凭据和当前成员私有不听名单。
- `member_joined`：其他成员加入。
- `member_left`：普通成员离开或超时移除。
- `room_closed`：房主离开或超时导致房间关闭。
- `member_updated`：成员连接状态、静音或发言权限变化。
- `member_listening_updated`：只回给当前听众，包含当前听众私有不听名单。
- `webrtc_answer`：后端对浏览器 offer 生成的 answer。
- `renegotiation_needed`：有新的服务端下行音轨，客户端需要重新 offer。
- `ice_candidate`：后端本地 ICE candidate，只回给当前协商连接。
- `error`：请求失败，包含稳定错误码和中文错误消息。

`ClientSignal` 使用 `deny_unknown_fields`。这会拒绝旧的成员间 P2P 信令字段，避免客户端绕过后端 SFU 媒体层。

## 连接生命周期

创建、加入和恢复成功后：

1. `RoomStore` 更新领域状态。
2. `SignalHub` 为该成员注册房间事件通道。
3. 服务端发送 `joined_room` 给当前连接。
4. 对其他成员广播 `member_joined` 或 `member_updated`。

WebSocket 断开时：

- 如果是显式 `leave_room`，立即执行离开逻辑。
- 如果是非显式断开，关闭媒体会话，注销信令通道，将成员标记为离线，并广播 `member_updated`。
- 后端启动一个延迟清理任务；宽限期内成员可以用恢复凭据回到原身份。
- 宽限期后仍未恢复：普通成员被移出房间，房主则关闭房间。

## 媒体层和 SFU 转发

媒体层在 `src/media/mod.rs`，核心类型是 `MediaController`。

每个成员只有一条到后端的 `RTCPeerConnection`。这个连接同时承载：

- 浏览器上行麦克风音轨。
- 后端给该成员的多个下行音轨槽位。

主要状态：

- `sessions`：按 `(room_id, member_id)` 保存媒体会话。
- `member_can_speak`：发言权限缓存；即使媒体会话尚未建立，也可提前同步权限。
- `member_not_listening`：当前听众不接收哪些发布者的私有策略。
- `event_sender`：媒体事件广播，用于通知信令层发起重新协商。

媒体 offer 处理流程：

1. WebSocket 收到 `webrtc_offer` 后调用 `MediaController::handle_offer`。
2. 如果该成员已有 PeerConnection，复用旧连接做重新协商。
3. 如果是新连接，创建 PeerConnection，预创建固定数量的下行音频 sender 槽位。
4. 后端设置 `on_track` 回调，收到浏览器上行音频后登记 `InboundTrack`。
5. 为同房间其他成员挂载该发布者的 fanout track。
6. 返回 SDP answer，并把后端本地 ICE candidate 通过 WebSocket 流式返回。

RTP 转发逻辑：

- 后端不解码 Opus，也不混音。
- 上行 RTP 包从 `TrackRemote` 读出后写入对应的 `TrackLocalStaticRTP`。
- `can_speak = false` 时，服务端在 RTP 边界丢弃该成员上行包。
- 听众屏蔽某个发布者时，后端从该听众的下行表移除该发布者音轨，并把对应 sender 替换回空槽位 track。
- 听众恢复接收时，后端查找发布者已存在的上行 fanout track 并重新挂载。
- 新听众加入或重新 offer 后，会按当前私有不听策略接入已有发布者音轨。

## 错误模型

后端统一使用 `Error`：

- HTTP 响应包含稳定 `code` 和中文 `message`。
- WebSocket `error` 消息复用同一组错误码和中文消息。
- 常见错误码包括 `room_not_found`、`room_full`、`not_room_owner`、`member_not_found`、`invalid_resume_token`、`invalid_message`、`internal_error`。

协议字段和错误码保持英文，便于前端稳定判断；用户可读消息保持中文。

## 当前边界

- 房间状态只保存在内存中，进程重启会丢失房间。
- 一个成员同一时间只能有一个活跃 WebSocket 绑定。
- 后端不做音频混音、录音、持久化账号、房间密码或多节点同步。
- 当前媒体实现依赖浏览器和后端之间的 WebRTC；普通 UDP 音频包不会进入 WebSocket。
- 默认每个成员预留 7 个下行音频槽位，超过槽位会返回内部错误。
