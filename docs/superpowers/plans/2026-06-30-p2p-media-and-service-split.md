# P2P 优先媒体链路与后端 Service 拆分长期实施计划

> **给阶段执行代理的要求：** 本任务必须采用分阶段、多会话方式完成。每个阶段会话使用主代理统筹，按阶段需要调用子代理并行协作；阶段结束前必须生成开发进度文档和会话交接文档，完成测试后只提交本阶段相关文件。

日期：2026-06-30

## 目标

本长期任务包含两个顺序执行的大目标：

1. 当前屏幕共享与视频通话已经支持 SFU 转发，但多人场景下后端带宽压力过大。需要新增 P2P 优先的媒体链路：默认成员之间直接 P2P 传输音频、摄像头视频和屏幕共享视频；当某一对成员 P2P 无法连接时，仅这一对成员回退到现有 SFU 链路。
2. P2P 功能完成并通过全部测试后，再将后端按 service 形式拆分，明确模块职责和中文注释，并保持现有协议和行为兼容。

这两个目标不能交叉实施。先完成 P2P 优先与 SFU 回退，再进行后端 service 拆分。

## 当前实现基础

项目当前状态：

- 后端 Rust/Axum 负责房间、认证、WebSocket 信令、WebRTC SFU 媒体转发和静态资源。
- 前端 Vue/Vite 负责大厅、语音房、屏幕共享、摄像头视频、成员控制和聊天。
- 当前房间主 UI 已迁移到 Vue/Vite，旧 `static/room.*` 已归档，不再作为主要实现入口。
- `src/media/mod.rs` 已经区分音频、屏幕共享视频和摄像头视频，并保留现有 SFU 转发链路。
- `src/transport/http/signaling.rs` 当前把 `webrtc_offer`、`webrtc_answer`、`ice_candidate` 语义定义为浏览器和后端 SFU PeerConnection 之间的协商，不是成员间 P2P 信令。
- 后端已有屏幕共享状态、摄像头发布状态、成员断线恢复和 WebSocket 广播能力。

重要兼容约束：

- 不破坏现有 SFU 信令和媒体路径。
- P2P 信令必须使用新的消息类型，不能改变现有 `webrtc_offer`、`webrtc_answer`、`ice_candidate` 的 SFU 语义。
- 屏幕共享视频和摄像头视频必须继续独立处理，不能互相覆盖。
- 断线恢复、房主关闭房间、成员离开、权限控制和聊天行为必须保持兼容。

## 总体方案

新增一个“媒体路由”概念，用于描述成员对之间当前采用的媒体路径：

- `p2p`：默认路径。成员之间直接建立浏览器到浏览器的 PeerConnection。
- `sfu`：回退路径。成员之间无法 P2P 建连时，继续使用现有后端 SFU 转发。

媒体路由按成员对记录，而不是按整个房间记录。这样某一对成员 P2P 失败时，只回退这一对，其他成员之间的 P2P 连接继续保持。

后端职责：

- 校验同房间、在线成员之间的 P2P 信令目标。
- 转发成员间 P2P offer、answer 和 ICE candidate。
- 记录成员对路由状态，并广播路由变化。
- 保留现有 SFU 媒体层，作为 P2P 失败、浏览器不支持或测试强制失败时的兜底。

前端职责：

- 入房后默认为其他在线成员建立 P2P PeerConnection。
- 本地麦克风、摄像头和屏幕共享轨道同时支持发布到成功的 P2P 连接和必要的 SFU 回退链路。
- 监听 P2P 连接状态，失败后发送回退信令。
- 收到路由更新后切换对应成员对的媒体路径，并清理不再使用的连接资源。

## 新增信令草案

新增客户端信令：

```json
{
  "type": "p2p_offer",
  "request_id": "...",
  "target_member_id": "...",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_answer",
  "request_id": "...",
  "target_member_id": "...",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_ice_candidate",
  "request_id": "...",
  "target_member_id": "...",
  "candidate": {}
}
```

```json
{
  "type": "p2p_connection_failed",
  "request_id": "...",
  "target_member_id": "...",
  "reason": "ice_failed"
}
```

新增服务端信令：

```json
{
  "type": "p2p_offer",
  "from_member_id": "...",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_answer",
  "from_member_id": "...",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_ice_candidate",
  "from_member_id": "...",
  "candidate": {}
}
```

```json
{
  "type": "media_route_updated",
  "member_ids": ["...", "..."],
  "route": "sfu",
  "reason": "p2p_failed"
}
```

服务端校验规则：

- 未加入房间时沿用当前 `not joined` 错误。
- 目标成员必须在同一房间。
- 目标成员必须在线。
- 不能向自己发送 P2P 信令。
- 未知字段继续由 `deny_unknown_fields` 拒绝。
- P2P 失败只影响发送失败上报的成员对。

## 分阶段会话规则

每个阶段会话必须遵守：

- 阶段开始先读取最新 `docs/dev-session/handoff-*.md`。
- 主代理负责阶段边界、测试选择、提交范围和最终合并判断。
- 子代理只处理明确分配的独立任务，不能回滚其他代理或用户已有改动。
- 阶段结束前生成：
  - `docs/dev-session/progress-YYYY-MM-DD-p2p-phase-N.md`
  - `docs/dev-session/handoff-YYYY-MM-DD-p2p-phase-N.md`
- 交接文档必须包含：
  - 当前完成进度。
  - 本阶段修改文件。
  - 已运行测试和结果。
  - 未完成事项。
  - 下一阶段实现目标。
  - 下一阶段建议的主代理/子代理分工。
- 提交前执行 `git status --short`，只暂存本阶段相关文件。
- 阶段提交信息使用简短英文，例如 `docs: plan p2p media rollout`、`feat: add p2p signaling routes`。
- 阶段结束后，下一会话必须依据最新交接文档继续，不依赖旧会话上下文。

## Phase 0：文档落地

目标：只生成长期实施计划、初始进度和交接文档，不修改运行时代码。

工作内容：

- 阅读仓库当前结构、已有设计文档、现有 `docs/dev-session` 材料。
- 确认当前主 UI 是 Vue/Vite，旧静态房间文件已归档。
- 写入长期实施计划。
- 写入 Phase 0 进度文档和交接文档。

验收：

- 文档明确 P2P 默认、按成员对失败回退 SFU。
- 文档明确任务一完成后才做 service 拆分。
- 不运行代码测试。
- 不提交任何运行时代码改动。

## Phase 1：P2P 协议与测试骨架

目标：在不改运行时行为的前提下，完善 P2P 协议设计和测试骨架。

主代理职责：

- 读取 Phase 0 handoff。
- 确认信令字段、媒体路由状态结构和测试边界。
- 协调子代理输出后端、前端、测试三类实现细节。

子代理建议：

- 后端探索代理：梳理 `src/transport/http/signaling.rs`、`src/domain/room.rs`、`src/media/mod.rs` 的现有边界。
- 前端探索代理：梳理 `frontend/src/lib/media-session.js`、`frontend/src/composables/useRoomMediaSession.js`、`frontend/src/lib/room-connection.js`。
- 测试代理：梳理 `tests/signaling_ws.rs`、`tests/room_permissions.rs`、`tests/frontend/*.test.mjs` 的新增覆盖点。

输出：

- P2P 协议设计补充文档。
- 后续阶段测试清单。
- Phase 1 progress/handoff。

验收：

```bash
git status --short
```

## Phase 2：后端 P2P 信令与媒体路由状态

目标：服务端能够转发成员间 P2P 信令，并记录按成员对的媒体路由状态。

实现要点：

- 扩展 `ClientSignal` 和 `ServerSignal`，新增 P2P offer、answer、ICE、失败上报和路由更新信令。
- 在房间领域层或 service 准备层新增成员对路由状态，默认 `p2p`。
- 成员对 key 必须规范化，避免 A-B 和 B-A 两份状态。
- P2P 信令仅转发给目标成员，不广播全房间。
- `p2p_connection_failed` 将对应成员对切换为 `sfu`，并向相关成员广播 `media_route_updated`。
- 成员离开、断线过期或房间关闭时清理相关路由状态。
- 为新增 Rust 函数和公开行为入口补齐中文注释。

测试：

- 同房间在线成员可以互发 P2P offer/answer/ICE。
- 向自己发送 P2P 信令会失败。
- 向不同房间或不存在成员发送 P2P 信令会失败。
- 目标成员离线时会失败。
- 单对 P2P 失败只把这一对路由切为 SFU。
- 成员离开后清理相关路由。

验收命令：

```bash
cargo test --test room_permissions
cargo test --test signaling_ws
```

## Phase 3：前端 P2P 会话管理

目标：前端默认建立成员间 P2P 连接，并能按成员对回退到 SFU。

实现要点：

- 新增 P2P 会话管理模块，建议放在 `frontend/src/lib/`。
- 入房和成员加入时，为其他在线成员建立 P2P PeerConnection。
- P2P 连接负责转发本地麦克风、摄像头和屏幕共享 track。
- 轨道元数据必须能区分 `audio`、`camera`、`screen`。
- 连接失败时发送 `p2p_connection_failed`。
- 收到 `media_route_updated` 后，对应成员对停用 P2P 接收，改用现有 SFU 媒体路径。
- 保留现有 `MediaSession`，避免 P2P 失败导致全房间媒体不可用。
- 为新增前端方法补齐中文注释。

测试：

- P2P manager 能创建连接、发送 offer、处理 answer 和 ICE。
- 收到远端 offer 后能生成 answer。
- ICE 或连接失败会上报回退。
- 成员离开会关闭对应 PeerConnection。
- 摄像头和屏幕共享开启后能发布到已成功的 P2P 连接。
- 对某成员对回退 SFU 后，不影响其他成员对 P2P。

验收命令：

```bash
npm run test:frontend
npm run build:frontend
```

## Phase 4：浏览器端联调与全量回归

目标：验证真实浏览器内 P2P 优先和 SFU 回退可用。

实现要点：

- 扩展浏览器测试覆盖双人或多人入房。
- 验证默认尝试 P2P。
- 验证摄像头视频和屏幕共享可通过 P2P 显示。
- 增加测试专用开关或 mock，强制某一对成员 P2P 失败并验证只回退这一对。
- 验证刷新、恢复、离开、房主关闭房间后的资源清理。

验收命令：

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
```

## Phase 5：后端 Service 拆分设计

目标：P2P 功能完成后，设计后端 service 拆分边界，暂不做大规模代码移动。

建议 service 边界：

- 房间生命周期 service：创建、加入、恢复、离开、断线清理。
- 成员控制 service：禁言、静音、收听偏好、延迟和发言状态。
- 媒体路由 service：P2P/SFU 路由状态、屏幕共享、摄像头发布。
- 聊天 service：消息校验、mention 校验、历史记录。
- 认证房间 service：认证开启时的持久房间创建、归属和关闭。

输出：

- Service 拆分设计文档。
- 公开方法草案。
- 错误映射规则。
- 测试迁移清单。
- Phase 5 progress/handoff。

验收：

```bash
git status --short
```

## Phase 6：后端 Service 拆分实施

目标：将后端业务逻辑拆到 service 层，保持行为和协议兼容。

实现要点：

- 新增 `src/service/` 模块，或采用阶段设计文档确认的等价目录。
- `transport/http` 只负责 HTTP/WebSocket 协议解析、socket 生命周期和响应发送。
- 业务判断移动到 service 方法。
- 每次移动一组相关逻辑后运行对应测试，避免一次性大改。
- 修改范围内缺少中文注释的 Rust 公开行为入口必须补齐。
- 不改变现有 JSON 字段，除已实现的 P2P 新增信令外不做破坏性协议变更。

验收命令：

```bash
cargo test
```

## Phase 7：最终回归、注释审查和收尾提交

目标：完成长期任务最终检查。

检查项：

- P2P 默认、单对失败回退 SFU 行为已实现。
- 现有 SFU 路径仍可用。
- 屏幕共享和摄像头视频互不覆盖。
- 后端已经按 service 形式拆分。
- 本次修改范围内 Rust 和前端方法中文注释完整。
- 所有阶段 progress/handoff 文档齐全。

最终验收命令：

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
git status --short
```

## 风险与处理

- 浏览器 NAT 或网络环境导致 P2P 难以稳定建连：保留 SFU 回退，浏览器测试中增加可控失败开关。
- 多人房间 P2P 连接数量随人数增长较快：先按当前房间规模实现，后续再引入连接上限或策略配置。
- SFU 和 P2P 同时存在可能造成重复播放：前端必须按成员对媒体路由选择接收来源。
- Service 拆分可能引入行为回归：必须在 P2P 完成后单独拆分，并逐步运行测试。

## 默认假设

- P2P 回退粒度为成员对，不是全房间。
- 现有 SFU 媒体链路必须保留。
- 不引入新的外部依赖，除非某阶段文档明确说明并经过确认。
- TURN/STUN 部署策略不在本长期任务内调整。
- 旧静态房间页面不作为实现入口，前端改动以 `frontend/src` 为准。
