# remote-voice

`remote-voice` 是一个浏览器语音房 MVP：

- Rust 提供房间、WebSocket 信令和服务端 WebRTC 音频转发。
- 前端由 Rust 服务直接提供静态页面。
- 昵称保存在浏览器 `localStorage`。
- 房间恢复凭据保存在当前标签页 `sessionStorage`，刷新房间页后可在断线宽限期内恢复房主或成员身份。

## 本地运行

```bash
cargo run
```

默认访问：

```text
http://127.0.0.1:8080
```

配置文件为 `application.yaml`：

```yaml
port: 8080
room:
  max_members: 8
  disconnect_grace_seconds: 30
```

## Docker HTTPS 部署

Docker 部署由两个服务组成：

- `voice`：Rust 服务，使用 host network，监听宿主机 `8080`。
- `nginx`：HTTPS 入口和 WebSocket 反向代理。

`voice` 使用 host network 是为了让 WebRTC 后端发布宿主机可达的 ICE 地址和 UDP 媒体 socket。不要把它改回普通 Docker bridge 网络，否则局域网其他设备可能只能拿到容器内网 candidate，页面和 WebSocket 正常但媒体协商失败。

### 1. 准备证书

Nginx 固定读取：

```text
deploy/certs/fullchain.pem
deploy/certs/privkey.pem
```

开发或局域网自用可以使用自签名证书；浏览器会提示证书不受信任，需要手动继续访问。

### 2. 启动服务

```bash
docker compose up --build -d
```

当前 Compose 端口：

| 用途 | 宿主机端口 | 容器端口 | 协议 |
| --- | --- | --- | --- |
| HTTP 跳转 HTTPS | `80` | `80` | TCP |
| HTTPS 页面和 WSS | `10000` | `443` | TCP |
| Rust 服务内部入口 | `8080` | host network | TCP |

访问示例：

```text
https://127.0.0.1:10000
https://<宿主机局域网 IP>:10000
```

Nginx 会把普通页面请求和 `/ws` WebSocket 请求转发到宿主机 `8080` 上的 Rust 服务。

### 3. 防火墙和媒体 UDP 端口

当前代码没有固定 WebRTC 媒体 UDP 端口。`webrtc-rs` 当前走默认的 ephemeral UDP 模式，ICE 收集时由宿主机系统分配临时 UDP 端口。

Linux 上查看部署机临时端口范围：

```bash
cat /proc/sys/net/ipv4/ip_local_port_range
```

本开发机当前输出为：

```text
32768 60999
```

部署时以目标宿主机自己的输出为准。局域网或服务器防火墙至少要允许：

- `80/tcp`
- `10000/tcp`
- 宿主机临时 UDP 端口范围，例如 `32768-60999/udp`

如果部署环境不能放行临时 UDP 端口范围，后续需要把媒体层改成受控 UDP 端口范围或 UDP mux，而不是只改 Nginx。

### 4. 查看状态

```bash
docker compose ps
docker compose logs -f voice nginx
```

如果房间能加入但媒体状态失败，优先检查：

1. 是否通过 `https://<宿主机地址>:10000` 访问。
2. `voice` 服务是否仍为 `network_mode: host`。
3. 宿主机 UDP 临时端口范围是否被防火墙拦截。
4. 浏览器麦克风权限是否允许。

## Docker 镜像构建和打包

### 构建镜像

只构建 Rust 镜像：

```bash
docker compose build voice
```

当前构建出的镜像名通常是：

```text
remote-voice-voice:latest
```

查看确认：

```bash
docker image ls remote-voice-voice
```

### 打包镜像

只导出 Rust 镜像：

```bash
docker image save remote-voice-voice:latest -o remote-voice-voice.tar
```

目标机器导入：

```bash
docker image load -i remote-voice-voice.tar
```

如果目标机器不能从镜像仓库拉取 Nginx，也把 Nginx 一起导出：

```bash
docker image pull nginx
docker image save remote-voice-voice:latest nginx:latest -o remote-voice-images.tar
```

目标机器导入：

```bash
docker image load -i remote-voice-images.tar
```

除镜像文件外，目标机器还需要带上：

- `docker-compose.yml`
- `deploy/nginx/nginx.conf`
- `deploy/certs/fullchain.pem`
- `deploy/certs/privkey.pem`
- `application.yaml`

当前部署没有内置 STUN 或 TURN。跨公网、复杂 NAT 或严格防火墙环境下，仍需做真实网络验证。
