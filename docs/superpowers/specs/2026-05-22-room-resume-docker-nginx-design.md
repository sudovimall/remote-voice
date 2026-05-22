# Room Resume And Docker Nginx 设计文档

日期：2026-05-22

## 目标

为当前 MVP 增加刷新恢复和临时断线重连，包含房主刷新场景；同时提供 Docker + Nginx HTTPS 反代部署入口，便于用户准备证书后启动服务。

## 范围

### 本阶段做

- 服务端为成员签发不可猜恢复令牌。
- `joined_room` 返回恢复凭据。
- 新增 WebSocket `resume_room`。
- 非显式断线进入 30 秒恢复宽限期。
- 房主断线在宽限期内可恢复原房间与房主身份。
- 普通成员断线在宽限期内可恢复原成员身份。
- 宽限期超时后：
  - 普通成员移除并广播离开。
  - 房主关闭房间并广播房间关闭。
- 前端当前标签页保存房间恢复会话。
- 前端页面存活期间 WebSocket 断线自动重连并重新建媒体会话。
- 显式离开继续立即退出；房主显式离开继续立即关闭房间。
- 增加 Docker 多阶段构建、Docker Compose、Nginx HTTPS/WebSocket 反代配置和最小部署说明。

### 本阶段不做

- 不实现跨标签页共享恢复状态。
- 不把恢复令牌持久化成账号或长期会话。
- 不做 TURN/STUN 部署。
- 不做完整生产安全加固或观测系统。

## 恢复方案

### 采用：恢复令牌 + 断线宽限期

成员创建成功后，服务端生成独立恢复令牌。浏览器恢复成员身份必须同时提交：

- `room_id`
- `member_id`
- `resume_token`

这样 `member_id` 保持标识语义，不单独充当恢复凭据。

### 未采用：只靠成员 ID 恢复

只靠 `member_id` 会让任何拿到成员 ID 的客户端接管房主或普通成员身份，恢复基础过弱。

### 未采用：刷新后重新加入为新成员

刷新后重新加入无法保持房主身份，也会和房主断开即关房的现有规则冲突。

## 协议

### 客户端消息

新增：

- `resume_room`
  - `request_id`
  - `room_id`
  - `member_id`
  - `resume_token`

保留：

- `leave_room`
  - 作为显式离开信号。

### 服务端消息

扩展 `joined_room`：

- `request_id`
- `room`
- `member_id`
- `resume_token`

创建、加入和恢复成功都返回这一结构。

## 服务端生命周期

### 成员状态

房间成员记录增加恢复令牌，序列化给房间快照时不暴露该令牌。

成员新增生命周期操作：

- 连接断开后标记 `connected = false`。
- 恢复成功后标记 `connected = true`。
- 显式离开时按当前离开逻辑移除或关闭房间。
- 断线超时后按成员角色清理。

### 非显式断线

WebSocket 处理循环因为刷新、网络断开或 socket close 结束，且没有收到显式 `leave_room` 时：

1. 关闭旧媒体会话。
2. 取消该成员 SignalHub 注册。
3. 将成员标记为离线并广播房间快照更新。
4. 启动 30 秒延迟清理任务。

延迟任务执行时必须重新检查成员仍离线，避免恢复后的旧任务误清理新连接。

### 恢复成功

`resume_room`：

1. 校验房间存在。
2. 校验成员存在。
3. 校验恢复令牌匹配。
4. 注册新的 SignalHub sender。
5. 标记成员在线。
6. 返回 `joined_room`。
7. 浏览器重建 WebRTC 媒体会话。

### 房主

- 房主刷新或临时断线时，房间进入 30 秒恢复期。
- 房主恢复成功后继续保有房主权限。
- 房主恢复超时后关闭房间并向仍在线成员广播 `room_closed`。
- 房主显式离开不等待恢复期，立即关闭房间。

### 普通成员

- 普通成员恢复超时后从成员列表移除并广播 `member_left`。
- 普通成员显式离开立即移除。

## 前端恢复

### 当前标签页房间会话

`sessionStorage` 保存：

```json
{
  "roomId": "ABC123",
  "memberId": "m_xxx",
  "resumeToken": "r_xxx",
  "nickname": "房主"
}
```

当前标签页会话与大厅一次性 entry intent 分开：

- entry intent 用于首次 create/join。
- room session 用于刷新和重连 resume。

### 加载优先级

房间页加载时：

1. 当前 URL 对应 entry intent 存在时，执行 create/join。
2. 否则当前 URL 对应 room session 存在时，执行 `resume_room`。
3. 否则显示回大厅提示。

### 页面内自动重连

如果页面还活着但 WebSocket 断开：

- UI 显示重连中。
- 关闭当前媒体会话。
- 使用短退避重新连 `/ws`。
- 连接成功后发 `resume_room`。
- 恢复成功后重启 WebRTC 媒体会话。

### 显式离开

房间页断开按钮：

- 发送 `leave_room`。
- 清理 room session。
- 清理媒体。
- 返回大厅。

显式离开不能被页面自动重连逻辑重新恢复。

## Docker And Nginx

### 容器形状

- `Dockerfile`
  - Rust builder 阶段编译 release 二进制。
  - runtime 阶段带应用二进制和 `application.yaml`。
- `.dockerignore`
  - 排除 `target`、IDE 文件和本地测试产物。
- `docker-compose.yml`
  - 应用服务。
  - Nginx 反代服务。
- `deploy/nginx/nginx.conf`
  - HTTPS server。
  - HTTP 到 HTTPS 跳转可选由配置直接提供。
  - `/ws` 配置 upgrade 与长连接 timeout。
  - 其余请求反代给应用服务。

### 证书挂载

约定用户准备证书目录并挂载到 Nginx：

- `deploy/certs/fullchain.pem`
- `deploy/certs/privkey.pem`

证书文件不进入仓库。

### 部署说明

README 补充最小步骤：

- 准备证书。
- 检查域名/server_name 配置。
- 执行 `docker compose up --build -d`。
- 暴露 HTTPS 端口。
- 说明当前未提供 TURN/STUN 容器，WebRTC 外网连通性仍需要在目标网络验收。

## 测试

### Rust

- 断线后成员标记离线，房间不立刻删除。
- `resume_room` 用正确 token 恢复并返回在线快照。
- 错误 token 恢复被拒绝。
- 普通成员离线超时后被移除。
- 房主离线超时后房间关闭。
- 显式房主离开仍立即关闭房间。

### 前端 Node

- room session 保存、加载和清理。
- entry intent 优先于 room session。
- resume 信令构造。
- 显式离开清理恢复会话。
- 重连辅助逻辑如果抽成 DOM 无关模块则覆盖调度边界。

### Playwright

- 房主创建房间后刷新，恢复仍是房主。
- 普通成员刷新，恢复原成员而不是新增成员。
- 房主显式离开后成员看到房间关闭。
- 媒体在恢复后重新连接。

### Docker

- `docker compose config` 可解析配置。
- Nginx 配置包含 WebSocket upgrade、HTTPS 证书路径和应用 upstream。

## 完成标准

- 房主和普通成员刷新后 30 秒内恢复原身份。
- 页面内 WebSocket 断线能自动恢复房间和媒体链路。
- 显式离开不进入恢复流程。
- 仓库有可用 Docker + Nginx HTTPS 反代入口和最小 README 说明。
