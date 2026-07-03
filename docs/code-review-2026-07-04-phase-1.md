# Code Review Report - 2026-07-04 Phase 1

创建时间：2026-07-04 02:46:23 CST
更新时间：2026-07-04 02:46:23 CST
更新概述：记录全量 code review 第一阶段对后端信令/媒体和前端 P2P/媒体关键链路的审查发现。

## Findings

### Critical

- [frontend/src/composables/useRoomMediaSession.js:151] 被房主禁言后 P2P 上行音频仍可能继续发送
  - 影响：成员 `can_speak=false` 时，SFU 后端会在 RTP 边界丢弃音频，但 P2P 直连音轨仍由前端继续发送，其他成员仍可能听到被禁言成员的声音，破坏房主权限语义。
  - 证据：`sendMemberSpeaking()` 只把发言状态归一化为 false；`startMedia()` 只按 `self_muted` 调用 `setMuted()`（frontend/src/composables/useRoomMediaSession.js:214）；`MediaSession.setMuted()` 切换的是本地输出音轨 enabled（frontend/src/lib/media-session.js:392），当前成员权限变化路径没有把 `!can_speak` 也应用到本地 P2P/SFU 发送轨道。
  - 建议：引入“有效静音”状态 `self_muted || !can_speak`，在启动媒体、成员权限更新、恢复连接时统一调用 `setMuted()`；补充 P2P 禁言后远端收不到音频的前端和浏览器回归测试。

- [src/transport/http/signaling.rs:1197] 断线恢复后媒体层丢失禁言和不听策略
  - 影响：成员普通断开时会关闭媒体会话并清掉媒体层策略；随后用 `resume_room` 恢复身份时，领域层仍保留 `can_speak` 和不听名单，但媒体层没有重新同步。被禁言成员恢复后可能重新以 `can_speak=true` 建立 SFU 转发；听众恢复后也可能重新收到已屏蔽发布者音频。
  - 证据：断开清理先调用 `state.media.close_member()`（src/transport/http/signaling.rs:1197）；`close_member()` 删除 `member_can_speak` 和 `member_not_listening`（src/media/mod.rs:619、src/media/mod.rs:621）；新 offer 建会话时用 `unwrap_or(true)` 作为默认发言权限（src/media/mod.rs:481）；`RoomStore::resume_room()` 只把成员标记在线（src/domain/room.rs:318）。
  - 建议：恢复成功后从房间快照重新同步当前成员的发言权限和所有私有收听策略到媒体层，或只在显式离开/过期时清理媒体策略；补充“禁言/不听后断线恢复仍生效”的后端媒体测试。

- [src/transport/http/signaling.rs:989] P2P 失败回退 SFU 后仍允许继续转发 P2P 信令
  - 影响：成员对已经通过 `p2p_connection_failed` 标记为 SFU 后，旧客户端或恶意客户端仍能继续发送 `p2p_offer`、`p2p_answer`、`p2p_ice_candidate`，后端会定向转发，前端也可能被迟到信令重新拉起 P2P，导致路由状态和实际媒体链路冲突。
  - 证据：失败路径会写入 `MediaRoute::Sfu`（src/domain/room.rs:737）；但 `forward_p2p_signal()` 只校验目标成员同房间在线（src/service/media_route.rs:179），没有检查当前成员对路由；前端收到回退后加入 `fallbackMembers`（frontend/src/lib/p2p-media-session.js:227），但 `handleOffer()` 和 `handleIceCandidate()` 仍会 `ensurePeer()`（frontend/src/lib/p2p-media-session.js:183、frontend/src/lib/p2p-media-session.js:216）。
  - 建议：后端转发 P2P 信令前拒绝已回退 SFU 的成员对；前端处理 P2P offer/ICE 前也检查 fallback 状态；补充“P2P failed 后继续发送 offer/ICE 应失败且不重建 peer”的 WebSocket 和前端测试。

- [frontend/src/composables/useRoomMemberPreferences.js:94] “不听某成员”不会作用到 P2P 音频播放
  - 影响：用户选择不听某成员后，SFU 下行策略可能生效，但 P2P 音频节点仍按成员音量播放，私有收听偏好在 P2P-first 路径下失效。
  - 证据：`rememberListeningState()` 只更新本地 Set 和存储；P2P 播放音频时只读取成员音量（frontend/src/lib/p2p-media-session.js:521、frontend/src/lib/p2p-media-session.js:527），没有读取不听名单或强制静音。
  - 建议：把不听名单纳入 P2P 播放控制，保留用户音量偏好但对 blocked 成员强制音量为 0 或关闭对应 audio 节点；补充 P2P 不听成员的单元测试。

### Improvements

- [src/config/settings.rs:26] 房间人数可配置但 SFU 下行槽位固定为 7
  - 影响：配置 `room.max_members > 8` 时，房间允许更多成员加入，但每个听众最多只有 7 个音频/摄像头下行槽位，第 8 个以上发布者会触发槽位不足或无法订阅，配置和媒体能力不一致。
  - 证据：`RoomSettings.max_members` 可配置；生产状态按该值创建 `RoomStore`（src/state.rs:72）；媒体层仍固定 `DEFAULT_DOWNLINK_SLOT_COUNT = 7`（src/media/mod.rs:48），无空槽时返回内部错误（src/media/mod.rs:1491）。
  - 建议：用 `max_members.saturating_sub(1)` 初始化媒体槽位，或在配置校验中限制 `max_members <= 8`；补充 9 人以上房间媒体测试。

- [src/media/mod.rs:1285] SFU 视频轨道依赖 track id 文本和到达顺序推断来源
  - 影响：真实浏览器的 `track.id`/`stream_id` 通常不是稳定的 `screen` 或 `camera` 文本；同一成员同时屏幕共享和开摄像头时，后端可能把摄像头转到屏幕区域，或把屏幕共享转到摄像头下行槽。
  - 证据：`webrtc_offer` 只有 SDP，没有携带 track source 元数据（src/transport/http/signaling.rs:92）；`classify_inbound_video_track()` 先查文本 marker（src/media/mod.rs:1309），否则按房间状态和已有 track 顺序推断（src/media/mod.rs:1316）。
  - 建议：为 SFU 协商增加明确的 camera/screen source 映射，例如按 transceiver mid 或 track id 上报；补充随机 track id 且同时发布 camera/screen 的测试。

- [src/storage/sqlite.rs:431] 持久房间 room_id 冲突会重新打开旧房间并改写 owner
  - 影响：运行时创建房间只检查内存房间 ID；如果随机 ID 命中持久表里的已关闭房间，`create_persistent_room()` 会用 `ON CONFLICT` 更新 owner 并清空 `closed_at_epoch_seconds`，可能误重开旧房间且保留旧 `created_at`。
  - 证据：`ON CONFLICT(room_id) DO UPDATE` 设置 `owner_user_id = excluded.owner_user_id` 且 `closed_at_epoch_seconds = NULL`（src/storage/sqlite.rs:449）；房间 ID 生成只检查 `RoomStore` 内存表（src/domain/room.rs:207）。
  - 建议：持久房间创建遇到冲突时返回明确错误并重新生成房间 ID；如果需要重开，应设计独立的受权限保护流程。

- [src/service/room_lifecycle.rs:51] 持久房间加入后 touch 失败会留下半加入运行时状态
  - 影响：`join_room()` 先修改运行时房间，再刷新持久房间活跃时间；如果 SQLite touch 失败，客户端收到错误且不会注册信令，但成员可能已加入房间，房主身份也可能被转移。
  - 证据：`join_room_with_role()` / `restore_room_with_member()` 先返回 join（src/service/room_lifecycle.rs:51），之后才调用 `touch_if_persistent()`（src/service/room_lifecycle.rs:66）；`touch_if_persistent()` 会写 SQLite 并返回错误（src/service/authenticated_room.rs:69）。
  - 建议：在运行时变更前完成 touch，或在 touch 失败时回滚加入和 owner 变更；补充服务层失败注入测试。

- [src/service/member_control.rs:86] 收听偏好写入房间后媒体同步失败会造成状态不一致
  - 影响：客户端收到错误时会认为操作失败，但 `RoomStore` 中不听名单已改变；媒体层可能没有同步，导致恢复快照、前端状态和实际 SFU 下行策略不一致。
  - 证据：`rooms.set_member_listening()` 先修改领域状态（src/domain/room.rs:442），随后才 `media.set_member_listening(...).await?`（src/service/member_control.rs:92）；媒体层 replace track 失败会返回错误（src/media/mod.rs:1523、src/media/mod.rs:1571）。
  - 建议：媒体同步失败时回滚领域状态，或明确把操作语义改为领域状态成功且媒体层异步重试。

- [frontend/src/composables/useRoomMediaSession.js:250] 摄像头启动信令失败会让按钮卡在 busy
  - 影响：`start_video_call` 被服务端拒绝时，`cameraBusy` 已经置为 true，但普通 `error` 信令只展示错误，不复位按钮状态，用户可能无法再次点击摄像头按钮。
  - 证据：`toggleCamera()` 设置 `cameraBusy=true` 后只 fire-and-forget 发送控制信令（frontend/src/composables/useRoomMediaSession.js:260）；普通 error 分支只 `showError()`（frontend/src/composables/useRoomSession.js:316）；busy 复位依赖 `video_call_started` 后的权限流程或 `video_call_stopped`。
  - 建议：摄像头/屏幕共享控制改用 request/ack，或跟踪 request_id 前缀在错误响应中复位 UI 状态；补充拒绝启动摄像头的测试。

- [frontend/src/lib/media-session.js:848] WebAudio 增益路径下静音后仍可能继续上报 speaking=true
  - 影响：用户静音后，UI/服务端仍可能收到“正在说话”，成员列表发言状态错误。
  - 证据：`setMuted()` 禁用的是 `outboundStream` 音轨（frontend/src/lib/media-session.js:392）；启用 WebAudio 时 `outboundStream` 是 destination stream（frontend/src/lib/media-session.js:417）；`sampleSpeakingStats()` 检查的是原始 `localStream` 音轨 enabled（frontend/src/lib/media-session.js:852）。
  - 建议：保存显式 muted 状态，或采样时检查 outbound 音轨；补充 WebAudio 静音后不再上报 speaking=true 的单元测试。

- [frontend/src/composables/useRoomScreenShareSession.js:100] 屏幕共享停止后远端流未清空
  - 影响：远端共享停止后 `remoteScreenStream` 保留旧对象，下一次其他成员开始共享时可能短暂显示上一轮旧画面。
  - 证据：`handleScreenShareStopped()` 只停止本地共享并同步观看状态；`resetScreenStreams()` 清理远端流（frontend/src/composables/useRoomScreenShareSession.js:107），但该路径未在停止广播中调用。
  - 建议：任意 `screen_share_stopped` 都清空远端屏幕流；远端 `screen_share_started` 前也可先置空等待新 track。

- [src/media/mod.rs:1662] 屏幕共享 RTCP 反馈转发任务没有显式生命周期
  - 影响：同一听众反复观看/停止屏幕共享时，每次 attach 都会 spawn 一个 `read_rtcp` 转发任务；停止观看只替换为空 track，不取消旧任务，可能导致后台任务堆积或反馈竞争。
  - 证据：`attach_screen_video_to_subscriber()` 每次都会调用 `forward_subscriber_video_rtcp_feedback()`（src/media/mod.rs:1662），后者直接 `tokio::spawn` 循环读取（src/media/mod.rs:1806）；`detach_current_video_from_subscriber()` 只替换空槽位（src/media/mod.rs:2353）。
  - 建议：按 `(listener, publisher, kind)` 管理任务句柄，detach/re-attach 时取消旧任务，或在循环中检查当前 outbound track 是否仍匹配。

- [src/domain/room.rs:203] 房间昵称缺少 trim、空值和长度限制
  - 影响：恶意客户端可创建/加入空昵称或超长昵称，放大内存占用和 WebSocket 广播 JSON 体积，也会造成 UI 显示异常。
  - 证据：`create_room()` 和 `join_room_with_role()` 直接把昵称传入 `new_member()`（src/domain/room.rs:246），`new_member()` 直接保存 `nickname.into()`（src/domain/room.rs:765）。
  - 建议：为房间昵称增加统一校验：trim、拒绝空值、限制字符数；补充空昵称和超长昵称测试。

- [src/storage/migrations.rs:1] SQLite 迁移缺少版本化升级路径
  - 影响：已有旧 schema 数据库升级时，不会补列、改约束或做数据迁移，后续查询新字段可能在运行时失败。
  - 证据：迁移只有 `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`（src/storage/migrations.rs:4），执行方式只是 `execute_batch(SQLITE_SCHEMA)`（src/storage/sqlite.rs:74），没有 `PRAGMA user_version` 或版本表。
  - 建议：引入版本化迁移，并添加“旧 schema 文件打开后自动迁移”的测试。

### Nitpicks

- [frontend/src/lib/signaling-client.js:15] 前端类方法缺少中文注释
  - 建议：按 AGENTS.md 当前规范，为 `SignalingClient`、`RoomConnection`、`useRoomScreenShareSession` 等公开行为入口和关键方法补中文注释，说明做什么以及为什么这样处理。

## Test Gaps

- 缺口：P2P 权限语义缺少端到端覆盖，尤其是禁言、不听、P2P failed 后迟到信令。
- 缺口：断线恢复只覆盖身份和 UI 面板恢复，未覆盖媒体层策略恢复。
- 缺口：SFU 视频 camera/screen 来源契约缺少随机 track id 和双视频源测试。
- 缺口：SQLite 持久房间冲突和旧 schema 迁移没有失败注入/升级测试。

## Verification

- 已运行：`git status --short`、`git diff`、`git diff --staged`、`rg --files src frontend/src tests docs Cargo.toml package.json playwright.config.mjs`，并按审查范围只读阅读关键源码、测试和文档。
- 未运行：`cargo test`、`npm run test:frontend`、`npm run build:frontend`、`npm run test:browser`。
- 结果：本阶段只做文档化审查和问题盘点，未修改业务代码；按 AGENTS.md 文档阶段要求不运行代码测试。

## Open Questions

- P2P-first 语义下，房主禁言是否应立刻停止本地发送轨道，还是只允许继续采集但不发送给任何对端？
- P2P 失败回退 SFU 后是否允许未来显式恢复 P2P，若允许需要怎样的重置协议？
- SFU camera/screen 来源契约应使用 SDP mid、track id 上报，还是单独信令声明？

## Conclusion

Request Changes. Phase 1 已发现多处会影响房间权限、断线恢复、P2P/SFU 路由一致性和媒体状态的缺陷；下一阶段应先修复 Critical 项并补对应回归测试。
