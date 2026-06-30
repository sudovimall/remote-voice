# P2P Protocol Design - Phase 1

Date: 2026-07-01

## Purpose

This document finalizes the Phase 1 protocol and ownership decisions for the
P2P-first media rollout. Runtime behavior is intentionally unchanged in this
phase.

The design keeps the existing SFU protocol intact and adds separate member to
member P2P signaling messages. The backend remains the authority for room
membership, online state, and per-member-pair media route state.

## Non-Negotiable Compatibility Rules

- Existing `webrtc_offer`, `webrtc_answer`, and `ice_candidate` messages keep
  their current browser-to-server SFU meaning.
- P2P signaling uses only the new `p2p_*` message types.
- Screen share video and camera video remain independent media sources.
- Route fallback is per member pair, never per whole room.
- Backend service splitting starts only after P2P behavior is complete and
  verified.

## Current SFU Boundary

The current SFU path is a single browser-to-backend `RTCPeerConnection` per
member:

- Client `MediaSession.negotiate()` sends `webrtc_offer` to the backend.
- Backend `ClientSignal::WebrtcOffer` calls `state.media.handle_offer(...)`.
- Backend returns `ServerSignal::WebrtcAnswer` only to the same WebSocket.
- Backend ICE candidates are sent only to the same WebSocket as
  `ice_candidate`.
- Client `ice_candidate` is added to the backend PeerConnection and is not
  forwarded to other members.
- New backend media tracks trigger `renegotiation_needed`, which asks other
  clients to renegotiate with the SFU.

Phase 2 must preserve this path exactly while adding P2P signaling alongside it.

## Client Signals

P2P client messages should use required `request_id` values for error
correlation. They are still fire-and-forget messages: the server forwards valid
signals and sends an `error` only when validation fails. Frontend code should
therefore call `send()` with a generated `request_id`, not `request()`, unless a
future phase adds a dedicated success acknowledgement.

```json
{
  "type": "p2p_offer",
  "request_id": "p2p-offer-...",
  "target_member_id": "m_target",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_answer",
  "request_id": "p2p-answer-...",
  "target_member_id": "m_target",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_ice_candidate",
  "request_id": "p2p-ice-...",
  "target_member_id": "m_target",
  "candidate": {
    "candidate": "candidate:...",
    "sdpMid": "0",
    "sdpMLineIndex": 0,
    "usernameFragment": "..."
  }
}
```

```json
{
  "type": "p2p_connection_failed",
  "request_id": "p2p-failed-...",
  "target_member_id": "m_target",
  "reason": "ice_failed"
}
```

Recommended client failure reasons:

- `ice_failed`
- `connection_failed`
- `timeout`
- `unsupported`

Phase 2 can store all of these as a single route-update reason
`p2p_failed` while preserving the raw failure reason for logs if useful.

## Server Signals

P2P offer, answer, and ICE signals are delivered only to the target member.
The server replaces `target_member_id` with `from_member_id` so the recipient
cannot trust a client-supplied sender identity.

```json
{
  "type": "p2p_offer",
  "from_member_id": "m_sender",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_answer",
  "from_member_id": "m_sender",
  "sdp": "..."
}
```

```json
{
  "type": "p2p_ice_candidate",
  "from_member_id": "m_sender",
  "candidate": {
    "candidate": "candidate:...",
    "sdpMid": "0",
    "sdpMLineIndex": 0,
    "usernameFragment": "..."
  }
}
```

Route changes are broadcast to connected members in the room. Clients that are
not part of the pair may keep the route in a diagnostic map or ignore it for
media switching.

```json
{
  "type": "media_route_updated",
  "member_ids": ["m_a", "m_b"],
  "route": "sfu",
  "reason": "p2p_failed"
}
```

`member_ids` must be sorted by the same normalization rule used by the backend
pair key. This keeps A-B and B-A route updates stable.

## Server Validation Rules

All P2P client messages must be rejected when:

- the sender has not joined a room;
- `target_member_id` is the sender's own member id;
- the target member does not exist in the sender's room;
- the target member exists but is offline (`connected == false`);
- the message contains unknown fields;
- the target has no registered WebSocket sender at delivery time.

The existing `not joined` error text should be reused for unjoined sockets.
Other P2P validation failures can use `invalid_message` with a clear Chinese
message. Unknown JSON fields continue to be rejected by `deny_unknown_fields`.

## Media Route Ownership

Route state should live in `src/domain/room.rs`, owned by `RoomStore`.

Recommended types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRoute {
    P2p,
    Sfu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRouteReason {
    P2pFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MemberPairKey {
    first_member_id: String,
    second_member_id: String,
}
```

`MemberPairKey::new(a, b)` must sort the member ids and reject self-pairs.

Recommended `Room` storage:

```rust
#[serde(skip, default)]
media_routes: HashMap<MemberPairKey, MediaRouteState>,
```

This keeps route state private to the backend and prevents room snapshots from
changing shape before the frontend intentionally consumes route updates.

Recommended `RoomStore` methods:

- `validate_p2p_target(room_id, sender_member_id, target_member_id)`
- `media_route(room_id, first_member_id, second_member_id) -> MediaRoute`
- `set_media_route(room_id, first_member_id, second_member_id, route, reason)`
- private cleanup for routes containing a removed or expired member

Route lifecycle:

- Missing route means `p2p`.
- `p2p_connection_failed` changes only that normalized pair to `sfu`.
- Explicit leave removes all routes involving the leaving member.
- Disconnected-member expiration removes all routes involving the expired member.
- Room close removes all routes with the room.
- Recoverable WebSocket disconnect should not immediately delete routes; the
  member may resume during the grace period.

## SignalHub Responsibility

`SignalHub` should stay a WebSocket delivery registry. It should not own route
state.

Phase 2 should add a targeted send method similar to:

```rust
pub fn send_to_member(
    &self,
    room_id: &str,
    member_id: &str,
    signal: ServerSignal,
) -> Result<()>
```

P2P offer, answer, and ICE use `send_to_member`. Route updates can use the
existing room broadcast.

## Backend Flow

P2P offer:

1. Parse `ClientSignal::P2pOffer`.
2. Require joined socket state.
3. Validate target in `RoomStore`.
4. Send `ServerSignal::P2pOffer { from_member_id, sdp }` only to target.

P2P answer and ICE follow the same flow with their corresponding payloads.

P2P failure:

1. Parse `ClientSignal::P2pConnectionFailed`.
2. Require joined socket state.
3. Validate target in `RoomStore`.
4. Set the normalized pair route to `sfu`.
5. Broadcast `media_route_updated` for that pair.

## Frontend Integration Direction

Phase 3 should add an independent P2P manager, for example
`frontend/src/lib/p2p-media-session.js`, instead of folding P2P behavior into
the current SFU `MediaSession`.

Recommended integration points:

- `useRoomSession` owns a `p2pSessionRef` beside `mediaSessionRef`.
- `handleRoomSignal()` dispatches `p2p_offer`, `p2p_answer`,
  `p2p_ice_candidate`, and `media_route_updated` before room snapshot updates.
- `MediaSession` should expose controlled local-track access or callbacks so
  P2P can publish microphone, camera, and screen tracks without reading private
  fields directly.
- `useRoomMediaSession` remains responsible for microphone and camera browser
  permission flows.
- `useRoomScreenShareSession` remains responsible for screen-share permission
  and room state flows.
- A route update to `sfu` closes only the P2P connection for that member pair
  and keeps other P2P pairs alive.

## Phase 2 Implementation Checklist

- Add P2P client/server signal variants without changing existing SFU variants.
- Add route types and normalized member-pair key.
- Add room-store validation and route update methods.
- Add targeted WebSocket delivery in `SignalHub`.
- Wire P2P offer, answer, ICE, and failure handling in `handle_socket`.
- Clean route state on explicit leave, disconnected-member expiration, and room
  close.
- Add Chinese comments for new Rust public behavior entries and any touched
  public methods lacking comments.
