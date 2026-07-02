# 文档与功能状态对齐实施计划

> 创建时间：2026-07-02 17:39:44 CST
> 更新时间：2026-07-02 17:39:44 CST
> 更新概述：新增文档先行阶段计划，用于指导功能盘点、视频入口收尾、文档同步和后端硬化的多阶段实施。

## 目标

先把当前功能状态和后续阶段边界写清楚，再进入代码实现。每个阶段都要生成进度文档和交接文档，提交时只暂存本阶段相关文件。

## 当前判断

- P2P 优先媒体链路、单成员对 SFU 回退、视频通话后端/媒体层、P2P 前端会话、后端 service 拆分已经完成到 Phase 7。
- 独立“视频”tab 入口已经在工作区基本实现，但尚未提交，且恢复语义和浏览器验收仍需收尾。
- `README.md`、`docs/backend-logic.md` 和早期 specs/plans 与当前实现存在明显不同步。
- 后端仍有硬化项：摄像头下行槽位配置化、camera/screen track 来源契约、service 测试决策。

## 阶段规则

- 阶段开始先读最新 `docs/dev-session/handoff-*.md`。
- 主代理负责阶段目标、提交范围、验证命令和最终提交。
- 子代理并发数不超过 4；子代理只处理明确分配的只读审查或互不重叠的实现范围。
- 每个阶段结束前生成：
  - `docs/dev-session/progress-YYYY-MM-DD-<topic>.md`
  - `docs/dev-session/handoff-YYYY-MM-DD-<topic>.md`
- 每个新生成或更新的文档必须包含创建时间、更新时间和更新概述。
- 提交前运行 `git status --short`，只暂存本阶段相关文件。

## Phase 0：文档落盘和规范补充

目标：只新增当前功能状态、后续阶段计划、进度和交接文档，并补充 `AGENTS.md` 的文档落款规范。

工作内容：

- 新增 `docs/dev-session/feature-status-2026-07-02.md`。
- 新增本计划文档。
- 新增 Phase 0 progress/handoff。
- 更新 `AGENTS.md`，要求新增和更新文档时保留时间落款、更新时间和更新概述。

验收：

```bash
git status --short
```

代码测试：不运行。此阶段只修改文档和协作规范。

## Phase 1：视频 tab 收尾

目标：把当前未提交的视频入口调整收敛为可提交功能。

工作内容：

- 确认并暂存 `VideoCallPanel.vue` 等视频入口相关文件。
- 收紧 `room-entry` 面板偏好恢复语义，确保只在同一房间和同一本地会话恢复视频 tab。
- 主动离房时清理对应 panel 偏好，避免后续同房间新会话误恢复到视频 tab。
- 补浏览器验收：
  - 首次入房默认成员页。
  - 手动切到视频 tab 后刷新恢复视频 tab。
  - 恢复到视频 tab 不自动请求摄像头权限。
- 生成 Phase 1 progress/handoff 并提交。

验收命令：

```bash
npm run test:frontend
npm run build:frontend
npm run test:browser
git status --short
```

## Phase 2：当前文档同步

目标：让项目入口文档反映当前实现。

工作内容：

- 更新 `README.md`：
  - 项目定位从语音房 MVP 更新为远程声音、摄像头视频、屏幕共享、P2P-first 和 SFU fallback 的 Web 应用。
  - 同步 Vue/Vite、认证、持久房间和部署说明中的当前边界。
- 更新 `docs/backend-logic.md`：
  - 同步 P2P 信令、视频通话、媒体路由、service 层和当前 WebSocket 消息。
  - 移除或标记旧的“拒绝成员间 P2P 字段”等过期描述。
- 给早期 specs/plans 添加状态说明，标记 implemented、superseded 或 historical。
- 生成 Phase 2 progress/handoff 并提交。

验收：

```bash
git status --short
```

代码测试：不运行，除非阶段中改动了运行时代码。

## Phase 3：后端媒体硬化

目标：补齐视频通话媒体层的配置化和来源识别风险。

工作内容：

- 让摄像头下行槽位按 `room.max_members - 1` 初始化，至少覆盖默认 8 人房间。
- 增加 `room.max_members > 8` 的后端测试，防止多人摄像头下行槽位不足。
- 强化 camera/screen track 来源契约，减少启发式兜底带来的误分类风险。
- 生成 Phase 3 progress/handoff 并提交。

验收命令：

```bash
cargo test
npm run test:browser
git status --short
```

## Phase 4：service 测试决策和补强

目标：决定是否补 service 专项测试；如果补，则只覆盖跨组件编排风险最高的薄测试。

工作内容：

- 写明 service-specific unit tests 未新增的取舍是否继续接受。
- 如果补测试，优先覆盖：
  - `MediaRouteService::start_video_call` 媒体层失败后的房间状态回滚。
  - `MediaRouteService::report_p2p_failure` 只影响单个成员对。
  - `RoomLifecycleService::resume_room` 持久房间 activity touch 的 best-effort 语义。
- 生成 Phase 4 progress/handoff 并提交。

验收命令：

```bash
cargo test
git status --short
```

## 下一会话入口

下一阶段应读取：

- `docs/dev-session/handoff-2026-07-02-docs-phase-0.md`
- `docs/dev-session/feature-status-2026-07-02.md`
- 本计划文档

然后执行 Phase 1 视频 tab 收尾。
