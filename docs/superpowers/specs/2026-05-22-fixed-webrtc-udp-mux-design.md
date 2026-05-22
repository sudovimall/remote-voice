# Fixed WebRTC UDP Mux 设计文档

日期：2026-05-22

## 目标

把当前 WebRTC 后端从宿主机临时 UDP 端口切换到固定单 UDP 端口，降低 Docker HTTPS 部署时的防火墙配置成本。

## 范围

### 本阶段做

- 配置固定 WebRTC UDP mux 端口，默认使用 `40000`。
- 服务启动时一次性绑定媒体 UDP socket，并把它交给 `webrtc-rs` UDP mux。
- 房间和成员继续为每个浏览器维护独立 PeerConnection，但服务端 ICE host candidate 使用 mux socket 端口。
- 固定端口被占用或无法绑定时让服务启动失败，并保留可定位的错误信息。
- README 和示例配置只要求部署方放行固定媒体 UDP 端口。

### 本阶段不做

- 不加入 STUN/TURN。
- 不引入多端口 UDP 范围模式。
- 不改变 WebSocket 信令、房间生命周期或浏览器媒体控制逻辑。

## 方案

### 采用：单端口 UDP mux

配置层提供 `media.udp_mux_port`。`AppState::from_settings` 把这个端口传给媒体控制器，媒体控制器绑定 `0.0.0.0:<port>`，创建 `UDPMuxDefault`，再通过 `SettingEngine` 把 UDP 网络切到 mux 模式。

这样 Docker host network 下发布的服务端 host candidate 仍是宿主机可达地址，但端口不再从系统 ephemeral UDP 范围中随机选择。

### 未采用：固定 UDP 端口范围

受控范围比临时端口范围容易部署，但仍要求防火墙放行一段 UDP 端口，当前 MVP 没有这个复杂度需求。

### 未采用：继续使用临时 UDP 端口

临时端口会让部署说明依赖宿主机端口范围和防火墙策略，局域网联调也更难排查。

## 组件与数据流

### 配置

示例配置增加：

```yaml
media:
  udp_mux_port: 40000
```

缺少 `media` 配置时仍使用默认值，最小配置保持只写 HTTP `port` 也能启动。

### 媒体初始化

默认生产初始化：

1. 绑定 UDP socket。
2. 创建 UDP mux。
3. 把 mux 写入 WebRTC `SettingEngine`。
4. 复用现有 media engine、interceptor 和 PeerConnection 创建逻辑。

测试专用 VNet 初始化继续走现有网络注入路径，不把 UDP mux 强行叠到 VNet 测试上。

### 启动失败

绑定失败视为配置或部署错误，直接向上返回媒体初始化错误。应用不降级回随机端口，避免部署方以为固定端口已经生效。

## 部署

Docker 仍保留 Rust 服务 host network，Nginx 继续只负责 HTTPS 和 WSS 反代。防火墙说明改成：

- `80/tcp` 用于 HTTP 跳转。
- `10000/tcp` 用于 HTTPS 页面和 WSS。
- `40000/udp` 用于服务端 WebRTC 媒体流量，或配置文件中的自定义 mux 端口。

## 测试

- 配置测试覆盖 `media.udp_mux_port` 默认值、反序列化和日志显示。
- 媒体测试用测试 socket 绑定端口并验证服务端 ICE candidate 使用该 mux 端口。
- 最终验证包含 Rust 测试、格式检查、README 配置检查和 Docker Compose 配置检查。
