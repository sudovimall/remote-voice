# 后端逻辑说明
更新时间：2026-07-04 04:34:04 CST
更新概述：同步认证、持久房间、P2P/SFU 媒体路由、屏幕共享、摄像头视频和定向重新协商逻辑。

本文档梳理当前 Rust 后端的运行路径、核心状态、房间控制、音视频媒体和共享桌面逻辑。协议字段、WebRTC、ICE、SDP、RTP、SFU 等固定术语保留英文。

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
- `authenticated_rooms` 相关服务：认证开启时维护 SQLite 中的用户、session、邀请码和持久房间。
- `disconnect_grace_period`：成员断线后可恢复身份的宽限时间。

状态对象使用 `Arc` 包装，供 Axum handler、WebSocket 任务、service 编排和媒体事件任务共享。

## HTTP 路由

HTTP 路由在 `src/transport/http/mod.rs` 汇总：

- `GET /`：返回大厅页面。
- `GET /rooms/{room_id}`：返回房间页面。
- `GET /assets/{asset}`：返回编译期内嵌的静态资源。
- `GET /health`：健康检查，返回 `ok`。
- `GET /api/rooms`：返回房间摘要列表。
- `GET /api/rooms/{room_id}`：返回房间完整快照。
- `GET /api/client-config`：返回前端需要的屏幕共享和视频通话码率配置。
- `POST /api/rooms/{room_id}/members/{member_id}/speaking`：HTTP 兼容路径，用房主身份修改成员发言权限。
- 认证 API：登录、登出、邀请码注册、管理端房间关闭等路径在 `transport/http/auth.rs` 中处理。
- `GET /ws`：升级为 WebSocket 信令连接。

当前创建房间、加入房间和恢复身份都走 WebSocket；HTTP 不再负责创建或加入房间。

## 房间领域

`RoomStore` 在内存中保存所有房间。主要结构：

- `Room`：房间 ID、房主成员 ID、成员表、屏幕共享、摄像头发布者、聊天历史、媒体路由、创建时间和最后活跃时间。
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
- 同一房间同一时间只能有一个屏幕共享者，屏幕共享者本人或房主可以停止共享。
- 摄像头发布状态按成员记录，成员停止发布、离开或断线过期时会释放对应状态。
- P2P 失败时只把对应成员对路由标记为 SFU，其他成员对继续按默认 P2P 路由处理。

成员恢复依赖 `member_id + resume_token`。恢复成功后，原成员会重新标记为已连接，私有不听名单也会保留。
认证开启时，运行时成员会绑定创建、加入或恢复时的登录用户 ID；已绑定成员只能由同一账号恢复。

## Service 编排

`src/service/` 把 WebSocket/HTTP 入口中的跨模块业务规则拆开：

- `RoomLifecycleService`：创建、加入、恢复、回滚加入和关闭运行时房间。
- `AuthenticatedRoomService`：认证模式下持久房间的创建、加入决策、活跃时间刷新和关闭状态。
- `MemberControlService`：房主权限、成员发言权限、本地静音和不听策略。
- `MediaRouteService`：屏幕共享、摄像头发布、屏幕观看、WebRTC offer/ICE、P2P 信令和 SFU 回退。
- `ChatService`：聊天消息和 mentions 校验。

transport 层负责解析协议和发送消息，具体状态变更尽量委托 service 和 domain 层。

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
- `start_screen_share` / `stop_screen_share`：占用或释放房间屏幕共享状态。
- `set_screen_viewing`：当前成员是否正在观看屏幕共享，用于控制屏幕视频下行和观看人数。
- `start_video_call` / `stop_video_call`：占用或释放当前成员摄像头发布状态。
- `webrtc_offer`：浏览器发给后端 PeerConnection 的 SDP offer。
- `webrtc_answer`：当前后端不主动发起 offer，因此会拒绝。
- `ice_candidate`：浏览器发给后端 PeerConnection 的远端 ICE candidate。
- `p2p_offer` / `p2p_answer` / `p2p_ice_candidate`：成员间 P2P 信令，由服务端校验成员对和路由后定向转发。
- `p2p_connection_failed`：客户端报告某成员对直连失败，服务端把该成员对回退 SFU。

服务端消息：

- `joined_room`：加入、创建或恢复成功；返回房间快照、当前成员 ID、恢复凭据和当前成员私有不听名单。
- `member_joined`：其他成员加入。
- `member_left`：普通成员离开或超时移除。
- `room_closed`：房主离开或超时导致房间关闭。
- `member_updated`：成员连接状态、静音或发言权限变化。
- `member_listening_updated`：只回给当前听众，包含当前听众私有不听名单。
- `screen_share_started` / `screen_share_stopped`：屏幕共享状态广播。
- `screen_share_viewer_count_updated`：屏幕共享观看人数广播。
- `video_call_started` / `video_call_stopped`：摄像头发布状态广播。
- `video_call_publisher_count_updated`：摄像头发布人数广播，用于前端调整码率。
- `webrtc_answer`：后端对浏览器 offer 生成的 answer。
- `renegotiation_needed`：当前成员实际新增服务端下行音视频轨道时，客户端需要重新 offer。
- `ice_candidate`：后端本地 ICE candidate，只回给当前协商连接。
- `p2p_offer` / `p2p_answer` / `p2p_ice_candidate`：服务端替换真实发送者后定向转发的 P2P 信令。
- `media_route_updated`：成员对 P2P 失败后回退 SFU 的路由广播。
- `error`：请求失败，包含稳定错误码和中文错误消息。

`ClientSignal` 使用 `deny_unknown_fields`。P2P 信令也必须经过服务端成员、房间、在线状态和媒体路由校验；已回退 SFU 的成员对不会继续转发 P2P 信令。

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
- 后端给该成员的多个下行音频槽位、屏幕共享视频槽位和摄像头视频槽位。

主要状态：

- `sessions`：按 `(room_id, member_id)` 保存媒体会话。
- `member_can_speak`：发言权限缓存；即使媒体会话尚未建立，也可提前同步权限。
- `member_not_listening`：当前听众不接收哪些发布者的私有策略。
- `screen_share_owners` / `screen_share_viewers`：屏幕共享发布者和正在观看的成员。
- `video_call_publishers`：当前摄像头发布者。
- `event_sender`：媒体事件广播，携带实际新增下行的订阅者列表，用于定向通知重新协商。

媒体 offer 处理流程：

1. WebSocket 收到 `webrtc_offer` 后调用 `MediaController::handle_offer`。
2. 如果该成员已有 PeerConnection，复用旧连接做重新协商。
3. 如果是新连接，创建 PeerConnection，预创建固定数量的下行音频 sender 槽位，并按客户端 SDP 预留视频下行槽位。
4. 后端设置 `on_track` 回调，收到浏览器上行音频、屏幕视频或摄像头视频后登记 `InboundTrack`。
5. 为实际需要接收该发布者的同房间成员挂载 fanout track。
6. 返回 SDP answer，并把后端本地 ICE candidate 通过 WebSocket 流式返回。

RTP 转发逻辑：

- 后端不解码 Opus，也不混音。
- 上行 RTP 包从 `TrackRemote` 读出后写入对应的 `TrackLocalStaticRTP`。
- `can_speak = false` 时，服务端在 RTP 边界丢弃该成员上行包。
- 听众屏蔽某个发布者时，后端从该听众的下行表移除该发布者音轨，并把对应 sender 替换回空槽位 track。
- 听众恢复接收时，后端查找发布者已存在的上行 fanout track 并重新挂载。
- 新听众加入或重新 offer 后，会按当前私有不听策略接入已有发布者音轨。
- 屏幕共享视频只转发给正在观看屏幕共享的成员。
- 摄像头视频转发给同房间已建立视频下行槽位的成员。
- 视频下行 RTCP feedback 按成员会话、视频类型和槽位去重转发；重复 attach 会替换旧任务，停止观看、停止发布或会话关闭会取消任务。
- 媒体事件只通知实际新增下行的订阅者，避免未观看屏幕、不听发布者或没有听众时触发无效重新协商。

## P2P 与 SFU 路由

- 未记录的成员对默认使用 P2P。
- P2P 信令通过 WebSocket 由服务端校验并转发，不直接暴露跨房间或离线成员信息。
- 某成员对 P2P 失败后，服务端只把该成员对标记为 SFU，并广播 `media_route_updated`。
- 回退 SFU 后，前端关闭对应 P2P PeerConnection，后端拒绝继续转发该成员对迟到的 P2P 信令。
- SFU 路径继续使用 `webrtc_offer`、`webrtc_answer` 和 `ice_candidate`，兼容已有媒体协商逻辑。

## 错误模型

后端统一使用 `Error`：

- HTTP 响应包含稳定 `code` 和中文 `message`。
- WebSocket `error` 消息复用同一组错误码和中文消息。
- 常见错误码包括 `room_not_found`、`room_full`、`not_room_owner`、`member_not_found`、`invalid_resume_token`、`invalid_message`、`internal_error`。

协议字段和错误码保持英文，便于前端稳定判断；用户可读消息保持中文。

## 当前边界

- 未开启认证时，房间状态只保存在内存中，进程重启会丢失房间。
- 开启认证时，SQLite 会保存用户、session、邀请码和持久房间；运行时成员、媒体会话、聊天快照和 WebSocket 连接仍是内存状态。
- 一个成员同一时间只能有一个活跃 WebSocket 绑定。
- 后端不做音频混音、录音、房间密码、TURN 服务或多节点同步。
- 当前媒体实现依赖浏览器和后端之间的 WebRTC；普通 UDP 音频包不会进入 WebSocket。
- 默认每个成员预留 7 个下行音频槽位；摄像头视频下行槽位按客户端视频 offer 预留，槽位不足会返回内部错误。
