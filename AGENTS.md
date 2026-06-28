# AGENTS.md

## 项目定位

本项目是一个用于远程声音和共享桌面互联的 Web 应用。后端使用 Rust 提供房间管理、认证、WebSocket 信令、WebRTC 媒体转发和静态资源服务；前端使用 Vue/Vite 提供大厅、语音房、屏幕共享、成员控制和聊天界面。

## 开发原则

- 修改代码前先阅读相关模块，优先沿用现有目录结构、命名方式和错误处理风格。
- 不做与任务无关的重构、格式化或依赖升级。
- 不覆盖用户已有改动；提交前只暂存本次任务相关文件。
- 涉及远程声音、共享桌面、房间权限、断线恢复、WebSocket 信令、WebRTC 媒体协商的改动必须保持向后兼容，除非任务明确要求破坏性变更。

## 注释规范

- 每个 Rust 函数、方法、结构体实现中的公开行为入口，以及每个前端 JavaScript/Vue 方法都必须在上方保留中文注释。
- 注释要说明“做什么”和“为什么这样做”，不要只重复函数名或代码字面含义。
- 关键代码处必须补充中文注释，尤其包括：
  - 房间创建、加入、退出、恢复和权限判断。
  - WebSocket 消息解析、广播和错误处理。
  - WebRTC offer/answer、ICE candidate、音频轨道、屏幕共享轨道处理。
  - 认证、会话、密码校验、Cookie、存储迁移。
  - 前端媒体权限、设备状态、连接状态、重连、音量和成员偏好控制。
- 修改已有代码时，如果目标方法缺少中文注释，应在本次修改范围内补齐。

## 常用命令

```bash
cargo test
npm run test:frontend
npm run build:frontend
npm run test:browser
cargo run
```

## 测试要求

- 后端逻辑、权限、信令或存储相关改动后运行 `cargo test`。
- 前端 composable、组件、静态脚本或浏览器交互相关改动后运行 `npm run test:frontend`。
- 修改 Vue/Vite 前端后运行 `npm run build:frontend`。
- 修改端到端流程、登录、建房、入房或浏览器媒体交互后尽量运行 `npm run test:browser`；如果环境缺少浏览器、权限或服务依赖，最终说明中必须记录原因。

## 提交要求

- 测试通过后再提交。
- 提交前检查 `git status --short`，确认只暂存本次任务相关文件。
- 本项目当前工作区可能已有用户未提交改动，不要把这些改动加入提交。
- 推荐提交信息格式为简短英文，例如 `docs: add agent development guidelines`。
