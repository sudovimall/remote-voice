# Backend Service Split Design - Phase 5

Date: 2026-07-01

## Purpose

This document defines the backend service split to implement after the P2P
media rollout. Phase 5 is design-only: it should not move runtime code yet.

The split must preserve the existing HTTP, WebSocket, SFU, P2P, room snapshot,
authentication, persistence, and browser-test behavior. The main implementation
goal for Phase 6 is to move business orchestration out of
`transport/http/signaling.rs` and `transport/http/auth.rs` without changing the
JSON protocol.

## Current Responsibility Map

`src/domain/room.rs`

- Owns in-memory room state through `RoomStore`.
- Enforces room and member invariants: owner checks, resume token checks,
  capacity, connected/offline state, chat validation, screen-share occupancy,
  camera publisher state, and P2P member-pair route state.
- Also generates IDs/tokens and performs cross-cutting cleanup when a member
  leaves or expires.

`src/media/mod.rs`

- Owns the SFU WebRTC engine and RTP forwarding.
- Maintains backend PeerConnections, local ICE queues, publisher/listener
  media policy, screen-share viewers, video publishers, and media events.
- Should remain the low-level media engine rather than absorbing room business
  rules.

`src/transport/http/signaling.rs`

- Owns WebSocket protocol types and socket I/O.
- Also currently performs too much business orchestration: create/join/resume,
  persistent room ownership, room commands, chat, screen-share/video-call
  coordination, SFU offer/ICE, P2P forwarding, disconnect cleanup, and delayed
  expiration.

`src/transport/http/auth.rs` and `src/transport/http/rooms.rs`

- Own HTTP request/response, cookie, redirect, and page concerns.
- Also reach through `AuthService::store()` for persistent-room listing and
  closing.

`src/auth/service.rs`

- Owns authentication, sessions, invites, users, and admin checks.
- Exposes raw storage through `store()`, which has let persistent-room policy
  leak into HTTP and WebSocket handlers.

## Design Principles

- Keep `RoomStore` as the domain authority and public compatibility facade in
  the first implementation pass. Do not split it into multiple independently
  locked stores.
- Add service modules around existing domain/media/auth components before
  moving or renaming domain internals.
- Keep `ClientSignal` and `ServerSignal` in `transport/http/signaling.rs`.
  They are protocol DTOs and should continue to control serde shape.
- Keep the WebSocket handler responsible for socket lifecycle: upgrade,
  receive loop, send loop, request-id extraction, JSON parse errors, local ICE
  stream pumping, and connection close.
- Move command orchestration into services that return domain outcomes and
  neutral realtime effects. The transport layer maps those effects into
  `ServerSignal` and dispatches them.
- Keep all current error codes and Chinese error messages unless a test is
  intentionally updated in the same implementation phase.

## Proposed Module Layout

```text
src/service/
  mod.rs
  authenticated_room.rs
  room_lifecycle.rs
  member_control.rs
  media_route.rs
  chat.rs
  realtime.rs
```

`service::realtime` is the application-level orchestrator used by the WebSocket
handler. It composes the narrower services and returns realtime effects. It is
not a socket abstraction.

`transport/http/signaling.rs` can initially keep `SignalHub`, because it is
coupled to `ServerSignal` and socket queues. If the signal hub is moved later,
move protocol types or introduce neutral outbound events first to avoid a
service-to-transport dependency cycle.

## Service Boundaries

### Authenticated Room Service

Own persistent-room policy when authentication is enabled. It should hide raw
`SqliteStore` access from transport.

Responsibilities:

- Create a persistent room record for the authenticated creator.
- Decide whether an authenticated join should use `MemberRole::Owner` or
  `MemberRole::Member`.
- Reject joins for closed persistent rooms with `Error::RoomClosed`.
- Touch persistent room activity after successful join or restore.
- List open persistent rooms for admin/room-list APIs.
- Close persistent rooms as admin.
- Close persistent rooms as owner only when the current user owns the stored
  room.

Draft methods:

```rust
pub struct AuthenticatedRoomService;

pub enum PersistentJoinDecision {
    NotPersistent,
    JoinAs(MemberRole),
}

impl AuthenticatedRoomService {
    pub fn create_for_owner(
        &self,
        room_id: &str,
        owner: &CurrentUser,
        now_epoch_seconds: i64,
    ) -> Result<()>;

    pub fn join_decision(
        &self,
        room_id: &str,
        user: Option<&CurrentUser>,
    ) -> Result<PersistentJoinDecision>;

    pub fn touch_if_persistent(
        &self,
        room_id: &str,
        now_epoch_seconds: i64,
    ) -> Result<()>;

    pub fn list_open_for_admin(
        &self,
        actor: &CurrentUser,
    ) -> Result<Vec<PersistentRoomView>>;

    pub fn close_as_admin(
        &self,
        actor: &CurrentUser,
        room_id: &str,
        now_epoch_seconds: i64,
    ) -> Result<()>;

    pub fn close_as_owner_if_owned(
        &self,
        actor: Option<&CurrentUser>,
        room_id: &str,
        now_epoch_seconds: i64,
    ) -> Result<bool>;
}
```

Compatibility notes:

- `close_persistent_room` currently treats missing or already closed rows as
  successful. Preserve that behavior for admin close and owner cleanup.
- Owner cleanup must still skip closing when the current user is missing or is
  not the stored persistent owner.

### Room Lifecycle Service

Own room/session lifecycle orchestration around `RoomStore`. It should keep
membership cleanup atomic by using existing `RoomStore` methods.

Responsibilities:

- Create runtime room.
- Join runtime room, including persistent-room role decision.
- Restore missing runtime persistent room with the correct role.
- Resume an existing member.
- Handle explicit member leave.
- Handle ordinary WebSocket disconnect and delayed expiration.
- Close runtime room.
- Prepare joined-room payload data: member id, resume token, listening state,
  and chat history.

Draft methods:

```rust
pub struct RoomLifecycleService;

pub struct JoinOutcome {
    pub join: RoomJoin,
    pub not_listening_member_ids: Vec<String>,
    pub chat_messages: Vec<ChatMessage>,
    pub broadcast: Option<RoomLifecycleEvent>,
}

pub enum RoomLifecycleEvent {
    MemberJoined { room: Room, member_id: String },
    MemberUpdated { room: Room, member_id: String },
    MemberLeft { room: Room, member_id: String },
    RoomClosed { room_id: String },
}

pub struct DepartureOutcome {
    pub event: RoomLifecycleEvent,
    pub stopped_video_call: Option<String>,
    pub publisher_count: Option<usize>,
    pub should_clear_signal_room: bool,
    pub should_close_persistent_room: bool,
}

pub struct DisconnectOutcome {
    pub event: Option<RoomLifecycleEvent>,
    pub stopped_video_call: Option<String>,
    pub publisher_count: Option<usize>,
    pub schedule_expiration: bool,
}

impl RoomLifecycleService {
    pub fn create_room(&self, nickname: String, user: Option<&CurrentUser>) -> Result<JoinOutcome>;
    pub fn join_room(&self, room_id: &str, nickname: String, user: Option<&CurrentUser>) -> Result<JoinOutcome>;
    pub fn resume_room(&self, room_id: &str, member_id: &str, resume_token: &str) -> Result<JoinOutcome>;
    pub async fn explicit_leave(&self, room_id: &str, member_id: &str, user: Option<&CurrentUser>) -> Result<DepartureOutcome>;
    pub async fn disconnect(&self, room_id: &str, member_id: &str) -> Result<DisconnectOutcome>;
    pub async fn expire_disconnected(&self, room_id: &str, member_id: &str, user: Option<&CurrentUser>) -> Result<Option<DepartureOutcome>>;
    pub fn close_room(&self, room_id: &str) -> Result<Room>;
}
```

The exact outcome shape may change during implementation, but the important
point is to stop pre-reading state in `signaling.rs` before mutating room
state. For example, the current handler reads whether a member was a camera
publisher before `leave_room`; that should become part of the lifecycle outcome.

### Member Control Service

Own room member controls and their media-side effects.

Responsibilities:

- Self mute.
- Owner controlled `can_speak`.
- Per-listener receiving preference.
- Speaking state sanitization.
- Latency validation.

Draft methods:

```rust
pub struct MemberControlService;

pub struct MemberControlOutcome {
    pub events: Vec<RealtimeEvent>,
    pub direct: Vec<DirectEvent>,
}

impl MemberControlService {
    pub async fn set_self_muted(
        &self,
        room_id: &str,
        member_id: &str,
        self_muted: bool,
    ) -> Result<MemberControlOutcome>;

    pub async fn set_member_can_speak(
        &self,
        room_id: &str,
        actor_member_id: &str,
        target_member_id: &str,
        can_speak: bool,
    ) -> Result<MemberControlOutcome>;

    pub async fn set_member_listening(
        &self,
        room_id: &str,
        listener_member_id: &str,
        publisher_member_id: &str,
        listening: bool,
        request_id: Option<String>,
    ) -> Result<MemberControlOutcome>;

    pub fn set_member_speaking(
        &self,
        room_id: &str,
        member_id: &str,
        speaking: bool,
    ) -> Result<RealtimeEvent>;

    pub fn set_member_latency(
        &self,
        member_id: &str,
        server_ms: f64,
    ) -> Result<RealtimeEvent>;
}
```

Compatibility notes:

- `set_member_speaking` currently suppresses speaking when the member cannot
  speak or is self-muted. Preserve that sanitization.
- Latency must remain finite and non-negative.
- When `can_speak` is turned off, the service must keep broadcasting
  `member_speaking_updated(false)`.

### Media Route Service

Own application-level media room state and route decisions. The existing
`MediaController` remains the SFU/WebRTC engine.

Responsibilities:

- Screen-share start/stop room occupancy.
- Screen viewer count and media attachment.
- Camera/video-call publisher state and publisher count.
- Rollback when media controller operations fail after room state changes.
- P2P offer/answer/ICE target validation and direct delivery intent.
- P2P connection failure and single-pair SFU fallback route update.
- SFU offer/ICE handoff to `MediaController`.

Draft methods:

```rust
pub struct MediaRouteService;

pub struct ScreenShareOutcome {
    pub events: Vec<RealtimeEvent>,
}

pub struct VideoCallOutcome {
    pub events: Vec<RealtimeEvent>,
}

pub struct SfuOfferOutcome {
    pub sdp: String,
    pub local_ice_candidates: mpsc::Receiver<IceCandidate>,
}

pub enum P2pForwardKind {
    Offer { sdp: String },
    Answer { sdp: String },
    IceCandidate { candidate: IceCandidate },
}

pub struct P2pForwardOutcome {
    pub target_member_id: String,
    pub event: DirectEvent,
}

impl MediaRouteService {
    pub async fn start_screen_share(&self, room_id: &str, member_id: &str) -> Result<ScreenShareOutcome>;
    pub async fn stop_screen_share(&self, room_id: &str, member_id: &str) -> Result<ScreenShareOutcome>;
    pub async fn set_screen_viewing(&self, room_id: &str, member_id: &str, viewing: bool) -> Result<ScreenShareOutcome>;

    pub async fn start_video_call(&self, room_id: &str, member_id: &str) -> Result<VideoCallOutcome>;
    pub async fn stop_video_call(&self, room_id: &str, member_id: &str) -> Result<VideoCallOutcome>;

    pub async fn handle_sfu_offer(&self, room_id: &str, member_id: &str, sdp: String) -> Result<SfuOfferOutcome>;
    pub async fn add_sfu_ice_candidate(&self, room_id: &str, member_id: &str, candidate: IceCandidate) -> Result<()>;

    pub fn forward_p2p_signal(
        &self,
        room_id: &str,
        sender_member_id: &str,
        target_member_id: &str,
        kind: P2pForwardKind,
    ) -> Result<P2pForwardOutcome>;

    pub fn report_p2p_failure(
        &self,
        room_id: &str,
        sender_member_id: &str,
        target_member_id: &str,
    ) -> Result<RealtimeEvent>;
}
```

Compatibility notes:

- `webrtc_offer`, `webrtc_answer`, and `ice_candidate` stay SFU-only.
- `p2p_*` signals stay member-to-member and never enter `MediaController`.
- `p2p_connection_failed` must change only the normalized failed pair to
  `route: sfu`.
- The P2P missing-member error must continue to become `invalid_message` at the
  WebSocket boundary instead of leaking `member_not_found`.

### Chat Service

Own chat message validation and history access around `RoomStore`.

Responsibilities:

- Trim and validate content.
- Validate mentions against room members.
- Normalize mentions to server-known member id and nickname.
- Save history and return sender ack plus broadcast event.

Draft methods:

```rust
pub struct ChatService;

pub struct ChatOutcome {
    pub ack: DirectEvent,
    pub broadcast: RealtimeEvent,
}

impl ChatService {
    pub fn send_message(
        &self,
        room_id: &str,
        member_id: &str,
        content: &str,
        mentions: Vec<ChatMention>,
        request_id: Option<String>,
    ) -> Result<ChatOutcome>;

    pub fn history(&self, room_id: &str) -> Result<Vec<ChatMessage>>;
}
```

## Realtime Effects

Services should avoid calling `SignalHub` directly. They should return effects
that the WebSocket handler dispatches.

Draft neutral effect types:

```rust
pub enum DirectEvent {
    JoinedRoom { request_id: String, join: JoinOutcome },
    MemberListeningUpdated { request_id: Option<String>, not_listening_member_ids: Vec<String> },
    ChatMessageSent { request_id: Option<String>, message: ChatMessage },
    WebrtcAnswer { request_id: Option<String>, sdp: String },
    P2pOffer { target_member_id: String, from_member_id: String, sdp: String },
    P2pAnswer { target_member_id: String, from_member_id: String, sdp: String },
    P2pIceCandidate { target_member_id: String, from_member_id: String, candidate: IceCandidate },
}

pub enum RealtimeEvent {
    MemberJoined { room: Room, member_id: String },
    MemberLeft { room: Room, member_id: String },
    RoomClosed { room_id: String },
    MemberUpdated { room: Room, member_id: String },
    MemberSpeakingUpdated { member_id: String, speaking: bool },
    MemberLatencyUpdated { member_id: String, server_ms: f64 },
    ChatMessage { message: ChatMessage },
    ScreenShareStarted { member_id: String, nickname: String },
    ScreenShareStopped { member_id: String },
    ScreenShareViewerCountUpdated { member_id: String, viewer_count: usize },
    VideoCallStarted { member_id: String, nickname: String },
    VideoCallStopped { member_id: String },
    VideoCallPublisherCountUpdated { publisher_count: usize },
    MediaRouteUpdated { member_ids: Vec<String>, route: MediaRoute, reason: MediaRouteReason },
    RenegotiationNeeded { member_id: String },
}
```

The implementation can refine these names. The constraint is that one place in
transport should convert service effects to `ServerSignal`, preserving the
current protocol and event ordering.

## Error Mapping Rules

Preserve the existing `crate::Error` mapping:

- `RoomNotFound` and `MemberNotFound` stay `404` in HTTP and their current WS
  codes unless a P2P boundary normalizes the error.
- `RoomFull` stays `room_full`.
- `NotRoomOwner` and `Forbidden` stay forbidden-style errors.
- `InvalidResumeToken` stays `invalid_resume_token`.
- `RoomClosed` stays `room_closed`.
- `InvalidMessage` stays `invalid_message`.
- `MediaNotReady` stays `media_not_ready`.
- Database and internal failures stay `internal_error`.

Special P2P rule:

- Missing, cross-room, offline, self, or undeliverable P2P targets must remain
  externally invalid P2P messages. If the domain returns `MemberNotFound`, the
  P2P service or WebSocket boundary must keep converting it to
  `Error::InvalidMessage(...)`.

Special persistent-room rules:

- Missing or already closed persistent rows during close should remain
  effectively idempotent.
- Admin-only persistent-room actions must still return `Forbidden` for ordinary
  users.
- Closed persistent-room join must still return `RoomClosed`.

## Phase 6 Migration Plan

1. Add `src/service/mod.rs` and service shells. Wire them from `AppState`
   without removing existing `rooms`, `media`, `signals`, or `auth` fields.
2. Implement `AuthenticatedRoomService` first and replace raw
   `AuthService::store()` usage in `auth.rs`, `rooms.rs`, and
   `signaling.rs`.
3. Add `ChatService` and migrate `send_chat_message` and `chat_history`
   orchestration. Keep `RoomStore` as the underlying domain facade.
4. Add `MemberControlService` for self mute, can-speak, listening, speaking,
   and latency.
5. Add `MediaRouteService` for P2P forwarding/fallback first, then screen
   share and video-call commands.
6. Add `RoomLifecycleService` for create/join/resume/leave/disconnect/expire.
   Migrate delayed cleanup last because it has timer and resume races.
7. Thin `handle_socket` so each `ClientSignal` branch validates joined state,
   calls a service, and dispatches returned effects.
8. Only after all behavior is protected by tests, consider moving `SignalHub`
   out of `transport/http` or replacing `ServerSignal`-typed hub queues with
   neutral realtime events.

## Test Migration Plan

Keep `tests/signaling_ws.rs` as the end-to-end regression harness throughout
Phase 6. Add service tests before moving each behavior slice, then use the
existing WebSocket tests to prove protocol compatibility.

Recommended order:

1. Authenticated room service tests:
   - creator becomes persistent owner;
   - owner join regains runtime owner role;
   - ordinary user joins as member;
   - closed persistent room returns `room_closed`;
   - admin close is idempotent and requires admin.
2. Room lifecycle service tests:
   - create/join/resume;
   - leave owner closes room;
   - normal disconnect keeps room recoverable;
   - expiration closes owner room or removes ordinary member;
   - route cleanup remains unchanged.
3. Chat service tests:
   - empty/too-long rejection;
   - mention normalization;
   - history trimming and joined-room history.
4. Member control service tests:
   - owner permission changes;
   - self mute;
   - listening preferences and cleanup;
   - speaking/latency event sanitization.
5. P2P media-route service tests:
   - offer/answer/ICE direct event shape;
   - target validation and no sender spoofing;
   - pair fallback route update;
   - SFU `webrtc_*` separation remains intact in WebSocket tests.
6. Screen-share/video-call service tests:
   - single screen-share owner;
   - owner force-stop;
   - screen viewer count;
   - camera publisher idempotency;
   - media rollback on controller failure.
7. SFU and connection lifecycle tests:
   - SFU offer answer and local ICE stream;
   - renegotiation events;
   - disconnect cleanup and resume races.

Acceptance for Phase 6 remains:

```bash
cargo test
```

Browser and frontend tests should also be run after any change that affects
P2P/SFU signaling behavior or WebSocket message ordering.
