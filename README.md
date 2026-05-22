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
media:
  udp_port_min: 40000
  udp_port_max: 40100
```

如果服务部署在云厂商公网 IP 到实例私网 IP 的 NAT 后面，还要配置对外发布的公网 IP：

```yaml
media:
  udp_port_min: 40000
  udp_port_max: 40100
  public_ip: 111.228.39.21
```

不配置 `media.public_ip` 时，服务端只按本机网卡地址收集 ICE host candidate，适合本机或局域网直连测试。

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
| WebRTC 媒体端口范围 | `40000-40100` | host network | UDP |

访问示例：

```text
https://127.0.0.1:10000
https://<宿主机局域网 IP>:10000
```

Nginx 会把普通页面请求和 `/ws` WebSocket 请求转发到宿主机 `8080` 上的 Rust 服务。

### 3. 防火墙和媒体 UDP 端口

服务端 WebRTC 媒体使用受限 UDP 端口范围。默认配置在 `application.yaml` 中固定为：

```yaml
media:
  udp_port_min: 40000
  udp_port_max: 40100
```

局域网或服务器防火墙至少要允许：

- `80/tcp`
- `10000/tcp`
- `40000-40100/udp`，或你在 `media.udp_port_min` 到 `media.udp_port_max` 中改成的范围

每个 WebRTC 会话会从这个范围内申请 UDP socket。范围过小、端口已被占用或被系统拒绝时，新的媒体会话可能无法建立；服务不会自动回退到范围外的随机 UDP 端口。

如果部署机网卡上只有私网地址，例如 `172.16.x.x`，但用户通过云厂商公网 IP 访问，必须把该公网 IP 配到 `media.public_ip`。否则浏览器收到的服务端 ICE candidate 仍是私网地址，页面和 WebSocket 可以连通，媒体会协商失败。

### 4. 查看状态

```bash
docker compose ps
docker compose logs -f voice nginx
```

如果房间能加入但媒体状态失败，优先检查：

1. 是否通过 `https://<宿主机地址>:10000` 访问。
2. `voice` 服务是否仍为 `network_mode: host`。
3. `media.udp_port_min` 到 `media.udp_port_max` 对应的 UDP 端口范围是否已放行，且没有被其他进程占满。
4. 云主机 NAT 部署时 `media.public_ip` 是否等于对外访问的公网 IP。
5. 浏览器麦克风权限是否允许。

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
