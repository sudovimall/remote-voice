# Remote Voice WebRTC SFU Design

Date: 2026-05-20

## Goal

Build a low-latency team voice platform that can run on a personal server with minimal operational dependencies.

The MVP is a browser-based voice room system:

- A user opens the web page, enters a nickname, and creates or joins a room.
- The room creator becomes the room owner.
- Browser clients send microphone audio to the Rust backend through WebRTC.
- The backend forwards audio from each speaker to other members in the same room.
- The room owner can control whether a member is allowed to speak.
- Text chat, persistent accounts, room passwords, and recording are later extensions.

## Non-Goals For MVP

- No video.
- No persistent database.
- No full account system.
- No audio mixing.
- No audio recording.
- No end-to-end encryption beyond normal WebRTC transport encryption.
- No clustered multi-server deployment.

## Architecture

The system uses a lightweight SFU architecture.

```text
Browser
  | HTTPS page load
  v
Rust backend

Browser
  | WebSocket signaling over TCP
  v
Rust backend

Browser
  | WebRTC audio: ICE + DTLS + SRTP over UDP/TCP fallback
  v
Rust backend
  | forwards RTP audio packets
  v
Other browsers in the same room
```

The backend is not a raw UDP voice server. Browsers cannot safely send plain UDP audio directly from JavaScript. Instead, each browser creates a WebRTC peer connection with the backend. The backend receives encrypted WebRTC media, terminates the WebRTC connection, and forwards RTP audio packets to other room members through their own WebRTC peer connections.

## Main Components

### HTTP Server

The Axum HTTP server owns:

- Static frontend file serving.
- `GET /health` for deployment checks.
- `POST /api/rooms` for room creation.
- `GET /api/rooms/:room_id` for room status.
- `GET /ws` for WebSocket signaling.

The HTTP layer should stay thin. It parses requests, calls domain services, and returns JSON responses or WebSocket messages.

### Room Domain

Room state is held in memory for MVP.

Each room contains:

- Room ID.
- Owner member ID.
- Members.
- Per-member speaking permission.
- Creation time.
- Last activity time.

Each member contains:

- Member ID.
- Nickname.
- Role: `owner` or `member`.
- Connection status.
- Speaking permission.
- Mute state reported by the client.

Rules:

- The creator of a room is the owner.
- Only the owner can change another member's speaking permission.
- The owner cannot be removed by non-owners.
- If the owner leaves, MVP should close the room and disconnect remaining members.
- Empty rooms are removed automatically.
- Rooms have a configurable member limit.

### Signaling

WebSocket signaling coordinates room membership and WebRTC negotiation.

Client-to-server messages:

- `join_room`
- `leave_room`
- `webrtc_offer`
- `webrtc_answer`
- `ice_candidate`
- `set_self_muted`
- `set_member_can_speak`

Server-to-client messages:

- `joined_room`
- `member_joined`
- `member_left`
- `room_closed`
- `member_updated`
- `webrtc_offer`
- `webrtc_answer`
- `ice_candidate`
- `error`

Signaling messages are JSON. Every message should include a `type` field and a request ID when a direct response is expected.

### Media Forwarding

The media layer owns backend WebRTC peer connections.

For each connected member:

- The browser sends one local microphone audio track to the backend.
- The backend receives that audio as an incoming track.
- The backend creates outgoing audio tracks for other room members as needed.
- Incoming RTP packets are forwarded to allowed recipients.

The backend does not decode Opus audio and does not mix multiple speakers into one stream. It forwards RTP packets to keep CPU usage and latency low.

Speaking permission is enforced on the server:

- If `can_speak = false`, incoming audio from that member is dropped.
- Other members are notified that the member cannot speak.
- The client UI should also disable or mark the microphone, but server enforcement is authoritative.

Receiving permission is not part of MVP. All connected room members can hear allowed speakers.

## Permission Model

Roles:

- `owner`: created the room and can control room permissions.
- `member`: can speak only when allowed by the owner.

Actions:

| Action | Owner | Member |
| --- | --- | --- |
| Create room | Yes | Yes |
| Join room | Yes | Yes |
| Speak when `can_speak = true` | Yes | Yes |
| Speak when `can_speak = false` | No | No |
| Change own mute state | Yes | Yes |
| Change another member's speaking permission | Yes | No |
| Close room | Yes | No |

Default MVP behavior:

- The owner has `can_speak = true`.
- New members have `can_speak = true` by default.
- The owner can turn a member's `can_speak` value on or off.

This default keeps small friend/team rooms convenient. A later room setting can make new members join muted by default.

## Frontend

The frontend is a single browser page for MVP.

Views:

- Lobby: nickname input, create room, join room by room ID.
- Room: room ID, member list, owner controls, microphone toggle, disconnect button.

Owner controls:

- Show a speaking permission toggle next to each member.
- Disable owner-only controls for normal members.
- Reflect permission changes immediately from server messages.

Audio behavior:

- Request microphone permission only when joining or creating a room.
- Use `RTCPeerConnection` for backend media transport.
- Play remote audio tracks from the backend.
- Allow local mute without leaving the room.

## Dependencies

Current dependencies already fit the service skeleton:

- `tokio`
- `axum`
- `tracing`
- `tracing-subscriber`
- `serde`
- `serde_yaml`
- `anyhow`
- `thiserror`

Additional likely dependencies:

- `serde_json` for WebSocket messages.
- `tower-http` for static file serving.
- `webrtc` for WebRTC, ICE, DTLS, SRTP, RTP, and media tracks.
- `nanoid` or `uuid` for room and member IDs.
- `dashmap` or `tokio::sync::RwLock<HashMap<...>>` for shared room state.

The `webrtc` crate is the main unavoidable dependency. Implementing ICE, DTLS, SRTP, RTP, and WebRTC negotiation directly is not realistic for this project.

## Configuration

`application.yaml` should grow to include:

```yaml
port: 8080
static_dir: "static"
index_file: "index.html"
room:
  id_length: 6
  max_members: 8
  empty_ttl_seconds: 60
webrtc:
  udp_port_min: 40000
  udp_port_max: 40100
  stun_urls:
    - "stun:stun.l.google.com:19302"
```

For personal server deployment, production should run behind HTTPS. WebRTC microphone access requires a secure context except on localhost.

## Error Handling

The backend should return structured errors:

- `room_not_found`
- `room_full`
- `not_room_owner`
- `member_not_found`
- `invalid_message`
- `webrtc_error`
- `internal_error`

WebSocket errors should be sent as JSON `error` messages when the connection can continue. Fatal errors should close the connection with a clear close reason.

## Testing Strategy

Unit tests:

- Room creation and deletion.
- Member join and leave.
- Owner permission checks.
- Speaking permission updates.
- Room cleanup behavior.

Integration tests:

- HTTP health endpoint.
- Room creation API.
- WebSocket join flow.
- Rejection when non-owner changes speaking permission.

Manual browser tests:

- Two browser tabs can join one room.
- One user speaks and the other hears audio.
- Owner disables a member's speaking permission.
- Disabled member's audio is not forwarded.
- Re-enabling speaking restores audio forwarding.

## Implementation Phases

### Phase 1: Service Skeleton

- Complete `lib.rs`, `app.rs`, `error.rs`, and `state.rs`.
- Start Axum from config.
- Add `/health`.
- Serve static frontend files.
- Add shared `AppState`.

### Phase 2: Room System

- Add room and member domain types.
- Generate short room IDs.
- Add room creation and status APIs.
- Add in-memory room storage.
- Add owner role and speaking permission model.

### Phase 3: WebSocket Signaling

- Define signaling message types.
- Implement `/ws`.
- Add join and leave flows.
- Broadcast member updates.
- Enforce owner-only permission changes.

### Phase 4: Backend WebRTC/SFU

- Add server-side WebRTC peer connection handling.
- Accept client microphone tracks.
- Create outgoing tracks for room members.
- Forward RTP packets without decoding or mixing.
- Drop incoming RTP from members with `can_speak = false`.

### Phase 5: Browser Client

- Build lobby and room UI.
- Add microphone permission flow.
- Connect WebSocket signaling.
- Create WebRTC connection to backend.
- Render member list and owner controls.

### Phase 6: Deployment

- Build release binary.
- Deploy binary plus static files.
- Add sample `systemd` service.
- Document Caddy or Nginx HTTPS reverse proxy.
- Document required TCP and UDP ports.

### Phase 7: Later Extensions

- Text chat over the existing room WebSocket.
- Room password.
- Persistent users.
- Owner transfer when owner leaves.
- TURN/coturn support for restrictive networks.
- Optional recording.
- Metrics and status page.

## Open Decisions

- Whether new members should default to `can_speak = true` or wait for owner approval.
- Whether owner leaving closes the room or transfers ownership.
- Whether room IDs should be purely random or human-readable.
- Whether MVP frontend should be plain HTML/CSS/JS or a small frontend framework.

Recommended MVP decisions:

- New members default to `can_speak = true`.
- Owner leaving closes the room.
- Room IDs are random 6-character uppercase alphanumeric codes.
- Frontend starts as plain HTML/CSS/JS to minimize dependencies.
