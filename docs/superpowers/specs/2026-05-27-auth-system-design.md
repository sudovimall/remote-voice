# 认证系统设计

日期：2026-05-27

## 目标

为 `remote-voice` 增加可配置启用的完整账号认证体系：

- 通过配置文件开启或关闭认证系统。
- 认证关闭时完全保持当前匿名房间模式。
- 认证开启时全站要求登录，包括大厅、房间页、房间 API 和 WebSocket。
- 配置文件提供初始管理员账号。
- 普通用户通过管理员生成的邀请码注册。
- 使用服务端 session cookie 维持登录状态。
- 第一版使用 SQLite 持久化用户、session、邀请码和房间归属。
- 后续可以在存储层扩展 PostgreSQL。
- 房间记录和房主归属在服务重启后仍可恢复，实时成员和媒体状态不持久化。

## 范围

本次设计包含：

- `auth.enabled` 总开关。
- 配置管理员同步到数据库。
- 用户名密码登录。
- 邀请码注册。
- 管理员后台的最小能力：生成邀请码、查看邀请码、查看用户、查看并关闭房间。
- 登录 session 的创建、校验、过期和退出。
- HTTP 页面、HTTP API 和 WebSocket 的认证保护。
- 认证开启时的房间持久化和房间归属。
- `auth.enabled=false` 时的匿名兼容路径。

本次不做：

- 邮箱验证、短信验证或第三方 OAuth。
- 公开注册。
- 密码找回。
- 多管理员管理界面之外的复杂权限体系。
- 房间成员、聊天历史、WebRTC 媒体状态的持久化。
- PostgreSQL 实现；只保留配置和接口扩展空间。
- 多节点部署、分布式 session 或跨节点房间同步。

## 已确认决策

- 第一版走分阶段单体认证架构。
- 管理员账号来自 `application.yaml`。
- 密码只保存哈希，使用 Argon2id。
- 存储第一版使用 SQLite，后续再支持 PostgreSQL。
- 认证开启后全站保护。
- 认证关闭后保持当前匿名模式，不要求数据库可用。
- SQLite 持久化房间记录和房主归属，重启后房间列表仍存在。
- 用户通过单次使用、可过期的邀请码注册。
- 创建房间的登录用户是稳定房主；管理员可以管理所有房间。
- 用户账号有 `display_name`，进房默认使用它，也允许临时改昵称。

## 配置设计

新增配置块：

```yaml
auth:
  enabled: true
  admin:
    username: admin
    password_hash: "$argon2id$v=19$m=19456,t=2,p=1$BASE64_SALT$BASE64_HASH"
    display_name: 管理员
  session:
    cookie_name: remote_voice_session
    ttl_hours: 168
    secure: auto
storage:
  kind: sqlite
  sqlite:
    path: remote-voice.db
```

配置语义：

- `auth.enabled=false`：
  - 不要求登录。
  - 不注册认证保护中间件。
  - 不强制初始化 SQLite。
  - 房间创建、加入、恢复保持当前 `RoomStore` 内存语义。
- `auth.enabled=true`：
  - 启动时必须能打开 SQLite。
  - 启动时必须存在管理员配置。
  - 启动时同步管理员到 `users` 表。
  - 页面、API 和 WebSocket 都需要有效 session。
- `auth.session.secure=auto`：
  - HTTPS 请求设置 `Secure` cookie。
  - HTTP 本地开发不设置 `Secure`，避免本地无法登录。

启动同步管理员时，以 `username` 为稳定键：

- 不存在则创建 `role=admin` 用户。
- 已存在则更新 `password_hash`、`display_name` 和 `role=admin`。
- 不通过配置删除管理员，避免误配置导致锁死。

## 数据模型

SQLite 第一版使用四组核心表。

```text
users
- id
- username
- password_hash
- display_name
- role: admin | user
- created_at
- updated_at
- disabled_at nullable
```

```text
sessions
- id
- token_hash
- user_id
- expires_at
- created_at
- last_seen_at
- revoked_at nullable
```

```text
invite_codes
- id
- code_hash
- created_by_user_id
- expires_at
- used_by_user_id nullable
- used_at nullable
- created_at
```

```text
persistent_rooms
- room_id
- owner_user_id
- created_at
- last_active_at
- closed_at nullable
```

约束：

- `users.username` 唯一。
- `sessions.token_hash` 唯一。
- `invite_codes.code_hash` 唯一。
- `persistent_rooms.room_id` 唯一。
- `persistent_rooms.owner_user_id` 引用 `users.id`。
- `sessions.user_id` 引用 `users.id`。

存储边界：

- `users` 是账号身份真源。
- `sessions` 是登录态真源。
- `invite_codes` 是注册入口真源。
- `persistent_rooms` 是房间归属和重启恢复真源。
- `RoomStore` 仍然负责运行时成员、`member_id`、`resume_token`、媒体状态和临时聊天。

## 后端模块

建议新增模块：

```text
src/auth/
- mod.rs
- model.rs
- password.rs
- session.rs
- service.rs
- middleware.rs

src/storage/
- mod.rs
- sqlite.rs
- migrations.rs
```

职责：

- `auth::password`：Argon2id 哈希和校验。
- `auth::session`：session token 生成、哈希、cookie 构造。
- `auth::service`：登录、退出、注册、邀请码和当前用户查询。
- `auth::middleware`：页面/API/WS 前置认证。
- `storage::sqlite`：SQLite 连接池和查询。
- `storage::migrations`：内置迁移，启动时自动执行。

`AppState` 增加认证状态：

```text
auth: AuthRuntime
```

`AuthRuntime` 可以是：

```text
Disabled
Enabled { auth_service, storage, settings }
```

这样认证关闭路径不需要假数据库对象，也不会污染当前匿名流程。

## 登录与注册流程

### 登录

1. 用户访问受保护页面。
2. 未登录时跳转到 `/login?next=原路径`。
3. 用户提交用户名和密码到 `POST /api/auth/login`。
4. 服务端查找用户，拒绝禁用用户。
5. 使用 Argon2id 校验密码。
6. 成功后生成随机 session token。
7. 只把 `token_hash` 写入 `sessions`。
8. 原始 token 写入 `HttpOnly` cookie。
9. 返回成功，前端跳转到 `next` 或 `/`。

登录失败统一返回 `invalid_credentials`，不区分用户名不存在和密码错误。

### 退出

1. 用户请求 `POST /api/auth/logout`。
2. 服务端根据 cookie 找到 session。
3. 设置 `revoked_at`。
4. 返回清空 cookie 的响应。

### 邀请码注册

1. 管理员在 `/admin` 生成邀请码，设置过期时间。
2. 服务端生成高熵随机 code，只保存 `code_hash`。
3. 用户访问 `/register?code={invite_code}`。
4. 前端提交邀请码、用户名、密码和显示名。
5. 服务端在同一事务内：
   - 校验邀请码存在。
   - 校验未过期。
   - 校验未使用。
   - 校验用户名未占用。
   - 创建 `role=user` 用户。
   - 标记邀请码 `used_by_user_id` 和 `used_at`。
6. 注册成功后自动登录并设置 session cookie。

邀请码是单次使用。邀请码原文只在创建后展示一次；后续只能看到状态和过期时间。

## 路由认证边界

认证开启时，公开路由：

```text
GET  /login
POST /api/auth/login
POST /api/auth/logout
GET  /register
POST /api/auth/register
GET  /assets/{asset}
GET  /health
```

认证开启时，受保护路由：

```text
GET  /
GET  /rooms/{room_id}
GET  /api/client-config
GET  /api/rooms
GET  /api/rooms/{room_id}
POST /api/rooms/{room_id}/members/{member_id}/speaking
GET  /ws
GET  /admin
POST /api/admin/invites
GET  /api/admin/invites
GET  /api/admin/users
GET  /api/admin/rooms
POST /api/admin/rooms/{room_id}/close
```

响应规则：

- 页面未登录：`302 /login?next={encoded_original_path}`。
- API 未登录：`401 {"code":"unauthenticated","message":"请先登录"}`。
- WebSocket 未登录：拒绝升级并返回 `401`。
- 登录但权限不足：`403 {"code":"forbidden","message":"没有权限执行该操作"}`。

认证关闭时，不启用上述保护；现有路由继续按匿名模式工作。

## 房间归属和恢复

认证开启时，房间有两层身份：

- 稳定身份：`persistent_rooms.owner_user_id`，来自登录用户。
- 运行时身份：`Room.owner_member_id`，来自当前连接的临时 member。

创建房间流程：

1. WebSocket 必须已认证。
2. `RoomStore::create_room()` 创建运行时房间。
3. SQLite 写入 `persistent_rooms(room_id, owner_user_id)`。
4. 如果 SQLite 写入失败，回滚运行时房间创建并返回错误。
5. 成功后返回现有 `joined_room` 信令。

加入房间流程：

1. WebSocket 必须已认证。
2. 先查运行时 `RoomStore`。
3. 运行时不存在时，查 `persistent_rooms`。
4. 如果持久化房间存在且 `closed_at` 为空，恢复空运行时房间。
5. 当前用户加入恢复后的房间。
6. 原房主用户进入恢复房间时，成为当前运行时房主成员。
7. 管理员进入恢复房间时可以拥有管理能力。
8. 普通用户加入时是普通成员。

关闭房间流程：

- 运行时房主可以关闭自己创建的房间。
- 管理员可以关闭所有房间。
- 关闭时写入 `persistent_rooms.closed_at`。
- 关闭后房间列表默认不显示。
- 关闭后房间链接不可加入。
- 关闭运行时房间并广播 `room_closed`。

房主断开或显式离开：

- 不再自动关闭持久化房间。
- 运行时仍可按现有逻辑清理成员和媒体。
- 持久化房间保持开放，直到房主或管理员显式关闭。

## 前端流程

新增页面：

```text
/login
/register?code={invite_code}
/admin
```

登录页：

- 显示用户名和密码输入。
- 登录成功后跳转 `next`。
- 登录失败显示统一错误文案。

注册页：

- 从 URL 读取邀请码。
- 用户填写 `username`、`password` 和 `display_name`。
- 注册成功后自动登录。
- 邀请码无效、过期或已使用时显示明确错误。

大厅页：

- 认证开启时右上角显示当前用户和退出按钮。
- 昵称输入框默认使用 `display_name`。
- 用户可以临时修改进房昵称。
- 临时昵称只影响本次房间成员，不回写账号资料。

房间列表：

- 来自持久化房间和运行时人数。
- 没有运行时成员的持久化房间显示 0 人。
- 点击加入时，后端可以恢复空运行时房间。

管理员页：

- 生成邀请码并显示原文一次。
- 查看邀请码状态：未使用、已使用、已过期。
- 查看用户列表。
- 查看房间列表。
- 关闭房间。

认证关闭时：

- 不显示登录、注册和管理员入口。
- 保持现有匿名大厅和房间体验。

## 错误模型

新增稳定错误码：

```text
unauthenticated
forbidden
invalid_credentials
invite_not_found
invite_expired
invite_used
username_taken
session_expired
auth_disabled
room_closed
```

错误返回规则：

- HTTP API 使用现有 JSON 错误风格，包含 `code` 和中文 `message`。
- WebSocket 继续使用 `error` 信令。
- 登录失败统一为 `invalid_credentials`。
- 禁用用户登录也返回 `invalid_credentials`，避免泄露账号状态。
- 注册时用户名重复返回 `username_taken`。
- 邀请码过期、已用、不存在分别返回稳定错误码。

## 安全策略

- 密码使用 Argon2id。
- session token 使用高熵随机值。
- 数据库只保存 session token 哈希。
- 邀请码只保存哈希。
- 登录 cookie 设置 `HttpOnly`。
- 登录 cookie 设置 `SameSite=Lax`。
- HTTPS 下设置 `Secure`。
- 登录成功后可以更新 `last_seen_at`。
- 启动时清理过期 session 和过期邀请码。
- 管理员接口全部要求 `role=admin`。
- API 不把 `password_hash`、`token_hash` 或 `code_hash` 返回给前端。

## 测试策略

配置测试：

- `auth.enabled=false` 时不要求管理员配置。
- `auth.enabled=true` 时缺少管理员配置会启动失败。
- SQLite 路径不可用时启动失败。
- `secure=auto` 在 HTTP 和 HTTPS 场景下生成不同 cookie 属性。

认证服务测试：

- 密码哈希和校验。
- 登录成功创建 session。
- 登录失败不区分用户名和密码错误。
- session 过期后不可用。
- logout 后 session 不可用。
- 禁用用户不能登录。

邀请码测试：

- 管理员可以创建邀请码。
- 普通用户不能创建邀请码。
- 有效邀请码可以注册用户。
- 邀请码注册后被标记为已使用。
- 过期邀请码不能注册。
- 已使用邀请码不能重复注册。
- 用户名重复返回 `username_taken`。

HTTP 测试：

- 未登录访问页面跳转登录页。
- 未登录访问 API 返回 `401`。
- 登录成功设置 cookie。
- 注册成功自动登录。
- 无权限访问管理员 API 返回 `403`。

WebSocket 测试：

- 未登录访问 `/ws` 拒绝升级。
- 已登录用户可以创建房间。
- 已登录用户可以加入房间。
- `auth.enabled=false` 时现有匿名 WebSocket 流程保持可用。

房间持久化测试：

- 创建房间写入 `persistent_rooms`。
- 数据库写入失败时不留下运行时房间。
- 重启式恢复空房间。
- 房主用户恢复后成为运行时房主。
- 普通用户加入恢复房间是普通成员。
- 管理员可以关闭任意房间。
- 关闭后的房间不可加入。
- `auth.enabled=false` 时房间仍只在内存里。

## 后续扩展

PostgreSQL 扩展方向：

- 将 SQLite 查询集中在 `storage` 模块。
- 对认证和房间归属暴露 repository 风格接口。
- 保持领域层不依赖具体数据库。
- 后续实现 `storage.kind=postgres` 时复用认证服务和 HTTP/WS 边界。

多用户权限扩展方向：

- 为房间增加成员授权表。
- 支持只允许被邀请用户加入特定房间。
- 支持多管理员和管理员禁用用户。
- 支持用户修改自己的密码和显示名。

## 验收标准

- `auth.enabled=false` 时，当前匿名创建、加入、刷新恢复、语音和屏幕共享流程不变。
- `auth.enabled=true` 时，未登录用户无法访问大厅、房间、API 和 WebSocket。
- 配置管理员可以登录并生成邀请码。
- 用户可以通过有效邀请码注册并自动登录。
- 登录用户可以创建和加入房间。
- 房间创建后写入 SQLite，并记录稳定房主用户。
- 服务重启后，持久化房间仍出现在房间列表中。
- 用户加入重启后的房间时，会恢复一个空运行时房间。
- 房主或管理员可以关闭房间。
- 关闭后的房间不能继续加入。
