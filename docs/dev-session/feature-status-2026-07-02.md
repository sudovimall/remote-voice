# 功能完成度盘点 - 2026-07-02

> 创建时间：2026-07-02 17:39:44 CST
> 更新时间：2026-07-02 17:39:44 CST
> 更新概述：新增当前功能完成度盘点，区分已提交完成、未提交已实现、未完成风险和后续阶段目标。

## 盘点依据

- 最新长期阶段交接文档：
  `docs/dev-session/handoff-2026-07-01-p2p-phase-7.md`。
- 近期设计与计划：
  `docs/superpowers/plans/2026-06-30-p2p-media-and-service-split.md`、
  `docs/superpowers/specs/2026-06-29-video-call-design.md`、
  `docs/superpowers/specs/2026-07-01-video-call-panel-entry-design.md`、
  `docs/superpowers/specs/2026-07-01-backend-service-split-design.md`。
- 当前工作区只读盘点结果和本阶段前端验证结果。

## 已提交完成

- P2P 优先媒体链路已经完成到 Phase 7：
  - `p2p_offer`、`p2p_answer`、`p2p_ice_candidate` 用于成员间信令。
  - `media_route_updated` 按成员对广播 P2P 失败后的 SFU 回退。
  - 未显式记录的成员对默认使用 P2P。
  - 单个成员对回退 SFU 不影响其他成员对。
- 现有 SFU 路径保持兼容：
  - `webrtc_offer`、`webrtc_answer`、`ice_candidate` 仍表示浏览器到后端 SFU 的协商。
  - P2P 信令没有复用或改变旧 SFU 消息语义。
- 视频通话后端和媒体层已经落地：
  - `video_call` 配置和 `/api/client-config` 下发已经存在。
  - 房间快照包含 `video_call_publishers`。
  - `start_video_call`、`stop_video_call` 和发布人数广播已经实现。
  - 媒体层区分音频、屏幕共享视频和摄像头视频。
  - 屏幕共享和摄像头视频下行槽位互不替换。
- Vue/Vite 前端主体已经迁移：
  - 大厅由 Vue 状态和 `useLobbySession` 驱动。
  - 房间会话拆分为连接、媒体、P2P、屏幕共享、聊天和成员偏好边界。
  - `P2PMediaSession` 独立管理浏览器到浏览器 PeerConnection。
  - `VideoGridPanel` 已用于摄像头视频宫格。
- 后端 service 拆分已经完成：
  - `src/service/` 下已有认证房间、房间生命周期、成员控制、媒体路由和聊天服务。
  - HTTP/WebSocket 入口已经把主要业务编排委托给 service 层。

## 未提交但已实现

当前工作区存在视频入口调整相关未提交改动：

- `frontend/src/components/room/RoomTabs.vue` 新增独立“视频”tab。
- `frontend/src/components/RoomView.vue` 把视频通话区域移动到 `VideoCallPanel`。
- `frontend/src/components/room/VideoCallPanel.vue` 是新增未跟踪文件，承载摄像头按钮、状态和视频宫格。
- `frontend/src/components/room/VoicePanel.vue` 已移除摄像头控制，只保留本地语音控制。
- `frontend/src/lib/room-entry.js` 已允许保存和恢复 `video` 面板偏好。
- `tests/frontend/room-entry.test.mjs` 和
  `tests/frontend/vue-room-layout.test.mjs` 已覆盖视频 tab 与布局归属。
- `tests/browser/p2p-media.spec.mjs` 已调整为进入视频 tab 后再开启摄像头。

本阶段已验证：

```bash
npm run test:frontend
npm run build:frontend
```

结果：两个命令均通过。

## 未完成和风险

- 文档不同步：
  - `README.md` 仍把项目描述为语音房 MVP，未反映 P2P-first、摄像头视频、认证、Vue/Vite 和 service 层现状。
  - `docs/backend-logic.md` 仍描述旧 SFU 音频边界，未同步 P2P 信令、视频通话和 service 拆分。
  - 早期 specs/plans 缺少状态标记，读者不容易判断哪些已实现、哪些被后续设计覆盖。
- 视频 tab 收尾仍有缺口：
  - 当前面板偏好按房间号保存，尚未严格绑定到同一本地会话。
  - 缺少浏览器测试覆盖首次入房默认成员页、刷新恢复视频 tab、恢复视频 tab 不自动请求摄像头权限。
  - 新增 `VideoCallPanel.vue` 仍是未跟踪文件，下一阶段提交时必须显式暂存。
- 后端硬化仍有后续空间：
  - 摄像头下行槽位当前默认 7 个，配置调大 `room.max_members` 后没有自动扩容。
  - camera/screen track 来源识别仍带有启发式兜底，同时开启摄像头和屏幕共享时应继续强化协议契约。
  - service 专项单元测试未新增，当前依赖领域、WebSocket、HTTP、前端和浏览器回归测试。

## 后续阶段建议

- Phase 1：完成视频 tab 收尾，修正恢复语义并补浏览器验收。
- Phase 2：同步 `README.md`、`docs/backend-logic.md` 和旧 specs/plans 状态。
- Phase 3：后端媒体硬化，处理摄像头下行槽位和 track 来源契约。
- Phase 4：根据阶段决策补 service 薄测试或写明继续以集成回归作为保障。
