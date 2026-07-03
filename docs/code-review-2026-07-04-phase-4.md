# 代码审查 Phase 4 修复报告
创建时间：2026-07-04 04:28:59 CST
更新时间：2026-07-04 04:28:59 CST
更新概述：记录 Phase 4 针对视频 RTCP 任务、定向协商和混合摄像头流的修复与验证结果。

## 修复范围

本阶段处理 Phase 3 交接中的 3 个剩余问题：

1. 视频 RTCP feedback 转发任务在重复 attach、停止观看、摄像头重复发布和重连恢复时可能累积。
2. `renegotiation_needed` 按房间广播给所有其他成员，未按实际新增下行订阅者定向发送。
3. 前端 SFU 和 P2P 远端摄像头流共用一个数组，混合路由场景会互相覆盖。

## 关键改动

- 媒体会话新增 `video_feedback_tasks`，按视频类型和下行槽位保存 RTCP feedback 任务；重复 attach 会替换旧任务，detach 和会话 Drop 会 abort 任务。
- 媒体事件新增 `subscriber_member_ids`，只在实际有订阅者挂上下行时发送事件；没有听众、未观看屏幕或“不听”过滤后不会触发无效重新协商。
- 信令层 `renegotiation_signal_for_event` 改为只给 `subscriber_member_ids` 中的当前 WebSocket 成员生成 `renegotiation_needed`。
- 前端媒体组合层拆分 SFU 与 P2P 远端摄像头来源，并通过 computed 列表合并展示；同一成员优先显示 P2P 流。
- P2P 组合层按房间摄像头发布者快照清理过期 P2P 远端流，重连和全量快照不会保留旧 tile。

## 测试补充

- 屏幕共享重复观看不会累积 RTCP feedback 任务，停止观看后任务归零。
- 摄像头重复发布不会累积 RTCP feedback 任务，停止发布后任务归零。
- 没有实际订阅者时媒体事件不再触发重新协商。
- 前端 SFU/P2P 远端摄像头流合并后同成员 P2P 覆盖 SFU。
- P2P 组合层会根据发布者快照清理过期远端摄像头流。

## 验证结果

- `cargo test`：通过，88 个库单元测试、28 个 `room_permissions` 集成测试、41 个 `signaling_ws` 集成测试全部通过。
- `npm run test:frontend`：通过，20/20。
- `npm run build:frontend`：通过。
- `npm run test:browser`：通过，4/4。
- `git diff --check -- <本阶段修改文件>`：通过。

说明：`cargo fmt --check` 仍会报告仓库既有格式漂移，剩余位置为 `src/config/settings.rs` 和 `src/transport/http/mod.rs`；本阶段已整理 `src/media/mod.rs` 中本阶段相关格式。全量 `git diff --check` 仍会因为用户既有 `AGENTS.md` 行尾空白失败，本阶段提交不包含该文件。

## 后续建议

- Phase 5 做剩余全量扫尾审查，重点确认前四阶段修改后是否还有安全、恢复、存储、文档一致性或测试缺口。
- 若只剩格式漂移，建议单独安排格式化提交，不和行为修复混合。
