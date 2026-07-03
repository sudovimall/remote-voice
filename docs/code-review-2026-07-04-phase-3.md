# 代码审查 Phase 3 修复报告
创建时间：2026-07-04 04:03:47 CST
更新时间：2026-07-04 04:03:47 CST
更新概述：记录 Phase 3 针对认证恢复绑定、屏幕共享回滚、媒体槽位清理和前端恢复问题的修复与验证结果。

## 审查范围

本阶段基于 Phase 2 交接继续审查房间生命周期、认证恢复、媒体路由和前端重连边界，使用主代理加 3 个只读子代理并行检查：

- 后端生命周期和认证信令。
- 媒体控制器、音视频下行和屏幕共享状态。
- 前端 SFU/P2P 媒体启动、回退和恢复提示。

## 已修复问题

1. 认证模式下 `resume_room` 只校验 `member_id + resume_token`，未绑定当前登录用户，可能导致其他账号恢复房主成员。
2. 屏幕共享开始时领域状态先占用，媒体层同步失败后未回滚房间共享状态。
3. 音频下行槽位统计会把屏幕共享/摄像头视频下行也计入，导致语音槽位被误判耗尽。
4. 发布者媒体会话关闭后，其他听众保留该发布者音频 fanout 下行，旧槽位无法被后续发布者复用。
5. 前端 SFU 协商失败后没有释放已采集麦克风和已发布给 P2P 的本地音频轨道。
6. 初始 `self_muted` 或 `can_speak=false` 在媒体启动完成后才生效，存在短暂 enabled 轨道窗口。
7. P2P 成员对回退 SFU 或关闭时没有清理该成员的远端屏幕流，可能显示冻结画面。
8. 成功重连或媒体恢复后未清除旧的可恢复错误提示。

## 关键改动

- `Member` 增加运行时 `auth_user_id` 绑定字段，认证创建、加入、恢复运行时房间时写入当前用户；恢复成员时要求绑定用户与当前登录用户一致。
- WebSocket `resume_room` 分支把 `CurrentUser` 传入房间生命周期服务，跨账号恢复房主成员会返回 `invalid_resume_token`。
- `MediaRouteService::start_screen_share` 在媒体 owner 同步失败时回滚本次新建的房间屏幕共享占位；媒体层同步失败也会恢复原 owner 缓存。
- 媒体控制器音频选槽只统计 `MediaTrackKind::Audio`，并在发布者 `close_member` 时从所有听众下行中移除该发布者音频。
- `MediaSession` 增加 `initialMuted`，在本地轨道发布和 offer 协商前禁用初始静音轨道；协商失败路径执行完整 `close()` 清理。
- P2P 会话按成员记录远端屏幕流，成员关闭、回退 SFU、轨道 ended/mute 时触发 `onScreenStream(null, memberId)`。
- 房间会话连接成功和媒体启动成功后清空旧错误提示，保留房间关闭等不可恢复错误。

## 验证结果

- `npm run test:frontend`：通过，20/20。
- `cargo test`：通过，86 个库单元测试、28 个 `room_permissions` 测试、41 个 `signaling_ws` 测试全部通过。
- `npm run build:frontend`：通过。
- `npm run test:browser`：通过，4/4。
- `git diff --check -- <本阶段修改文件>`：通过。

说明：`git diff --check` 全量检查仍会报告用户既有 `AGENTS.md` 行尾空白；本阶段限定文件检查通过。`cargo fmt --check` 仍会报告仓库既有格式漂移，包含 `src/config/settings.rs`、旧的 `src/media/mod.rs` 段落和 `src/transport/http/mod.rs`；本阶段未做无关格式化。

## 保留问题

- 视频 RTCP 反馈转发任务在重复 attach 时可能累积，需要下一阶段做结构化任务生命周期管理。
- `renegotiation_needed` 仍按房间广播给所有其他成员，而不是只通知实际新增下行的订阅者。
- 前端 SFU 和 P2P 远端摄像头列表仍共享一个数组，混合路由场景可能相互覆盖，需要下一阶段拆分来源后合并。
