# 代码审查 Phase 2 修复报告
创建时间：2026-07-04 03:34:41 CST
更新时间：2026-07-04 03:34:41 CST
更新概述：记录 Phase 2 针对 Phase 1 关键媒体权限和 P2P 回退问题的修复与验证结果。

## 修复范围

本阶段处理 Phase 1 报告中的 4 个 Critical 问题：

1. `can_speak=false` 未阻断本地 P2P 上行。
2. 成员断线恢复后媒体层丢失禁言和“不听”策略。
3. P2P 失败回退 SFU 后仍可能继续转发或处理迟到 P2P 信令。
4. “不听成员”偏好未作用于 P2P 音频播放。

## 关键改动

- 前端媒体组合层新增有效静音规则：`self_muted || can_speak === false`，启动媒体、成员权限更新和静音按钮都复用该规则。
- P2P 播放层新增成员收听策略，`notListeningMembers` 优先把对应成员音频音量降为 `0`，同时保留原始音量偏好。
- P2P 回退后前端忽略迟到的 `p2p_offer`、`p2p_answer` 和 `p2p_ice_candidate`，避免重新创建直连。
- 后端 `MediaRouteService::forward_p2p_signal` 在成员对已回退 SFU 时返回 `invalid_message`，不再继续转发 P2P 信令。
- 后端 `MediaController::sync_member_audio_policy` 以替换式策略恢复 `can_speak` 和“不听”名单；`MemberControlService::sync_room_media_policies` 在 `resume_room` 成功后把房间快照同步回媒体层。
- 媒体会话快照补充 `can_speak` 和 `not_listening_member_ids`，用于测试和诊断恢复策略是否落入媒体层。

## 验证结果

- `npm run test:frontend`：通过，20 个前端单元测试全部通过。
- `cargo test`：通过，83 个单元测试、28 个 `room_permissions` 集成测试、40 个 WebSocket 集成测试全部通过。
- `npm run build:frontend`：通过，Vite 构建成功。
- `npm run test:browser`：通过，4 个 Playwright 浏览器测试全部通过。
- `git diff --check -- <本阶段修改文件>`：通过。

说明：`cargo fmt --check` 仍会报告仓库中既有格式漂移，包含 `src/config/settings.rs`、`src/transport/http/mod.rs` 以及 `src/media/mod.rs` 的历史格式建议；本阶段未做无关格式化。

## 后续建议

- 下一阶段继续按最新交接文档检查剩余审查项，重点寻找未被 Phase 2 覆盖的中高风险信令、恢复和媒体边界问题。
- 若后续阶段计划统一执行 `cargo fmt`，应单独安排格式化提交，避免和行为修复混在一起。
