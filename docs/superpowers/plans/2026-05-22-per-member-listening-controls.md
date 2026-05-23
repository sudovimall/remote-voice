# Per-Member Listening Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let each room member stop and resume receiving a specific other member's voice, while preserving that private choice across refresh-based room resume.

**Architecture:** `RoomStore` owns the private per-member listening state because it already owns member resume identity and room cleanup. `MediaController` receives the current listener policy from signaling and applies it to SFU downlink fanout, existing-track attachment, detachment, and resume attachment. WebSocket responses expose only the current listener's blocked member IDs, and the browser combines that private state with the existing public room snapshot when it renders member controls.

**Tech Stack:** Rust, Axum WebSocket signaling, `webrtc-rs`, browser ES modules, Node test runner.

---

## File Map

- Modify `src/domain/room.rs`
  - Store private per-member listening state, validate target members, and clean references when members leave.
- Modify `tests/room_permissions.rs`
  - Cover room-domain listening semantics and resume persistence.
- Modify `src/media/mod.rs`
  - Track listener downlink policy and apply it when attaching, removing, and restoring SFU tracks.
- Modify `src/transport/http/signaling.rs`
  - Add listening control messages and private listening-state responses.
- Modify `tests/signaling_ws.rs`
  - Verify join/resume/private update behavior over WebSocket.
- Modify `static/room-controls.mjs`
  - Add DOM-free listening button labels, eligibility checks, and signal construction.
- Modify `tests/frontend/room-controls.test.mjs`
  - Cover the new control helper behavior.
- Modify `static/room.js`
  - Track current listener private state and render per-member buttons.
- Modify `static/styles.css`
  - Keep two member controls readable in desktop and narrow layouts.

### Task 1: Room Domain Private Listening State

**Files:**
- Modify: `src/domain/room.rs`
- Modify: `tests/room_permissions.rs`

- [ ] **Step 1: Write failing room-domain tests**

Add tests beside the existing permission and resume tests in `tests/room_permissions.rs`:

```rust
#[test]
fn 成员可以停止并恢复接收另一成员语音() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    let blocked = store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("成员可以不听另一成员");
    assert_eq!(blocked.not_listening_member_ids, vec![member.member.id.clone()]);

    let listening = store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, true)
        .expect("成员可以恢复接收");
    assert!(listening.not_listening_member_ids.is_empty());
}

#[test]
fn 成员恢复原身份后保留不听名单() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("写入不听名单");
    store
        .mark_member_disconnected(&owner.room.id, &owner.member.id)
        .expect("房主断线");

    let resumed = store
        .resume_room(&owner.room.id, &owner.member.id, &owner.resume_token)
        .expect("恢复房间");
    assert_eq!(
        resumed.member.not_listening_member_ids(),
        vec![member.member.id.clone()]
    );
}

#[test]
fn 成员不能屏蔽自己且目标离开后清理不听引用() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    let error = store
        .set_member_listening(&owner.room.id, &owner.member.id, &owner.member.id, false)
        .expect_err("不能不听自己");
    assert!(matches!(error, Error::InvalidMessage(_)));

    store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("写入不听名单");
    store
        .leave_room(&owner.room.id, &member.member.id)
        .expect("成员离开");

    let state = store
        .member_listening_state(&owner.room.id, &owner.member.id)
        .expect("读取当前名单");
    assert!(state.not_listening_member_ids.is_empty());
}
```

- [ ] **Step 2: Run the new room-domain tests and verify RED**

Run:

```bash
cargo test --test room_permissions 成员
```

Expected: FAIL because `set_member_listening`, `member_listening_state`, and member listening state do not exist yet.

- [ ] **Step 3: Add the minimal domain model and APIs**

Update `src/domain/room.rs` to store private blocked member IDs without exposing them in public `Room` serialization:

```rust
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub nickname: String,
    pub role: MemberRole,
    pub can_speak: bool,
    pub self_muted: bool,
    pub connected: bool,
    #[serde(skip, default)]
    not_listening_member_ids: HashSet<String>,
    #[serde(skip)]
    resume_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberListeningState {
    pub not_listening_member_ids: Vec<String>,
}

impl Member {
    pub fn not_listening_member_ids(&self) -> Vec<String> {
        sorted_member_ids(&self.not_listening_member_ids)
    }
}
```

Add room-store helpers that validate the listener and target, update only the listener, and return a sorted response state:

```rust
pub fn member_listening_state(
    &self,
    room_id: &str,
    member_id: &str,
) -> Result<MemberListeningState> {
    let rooms = self.read_rooms()?;
    let room = rooms.get(room_id).ok_or(Error::RoomNotFound)?;
    let member = room.members.get(member_id).ok_or(Error::MemberNotFound)?;
    Ok(MemberListeningState {
        not_listening_member_ids: member.not_listening_member_ids(),
    })
}

pub fn set_member_listening(
    &self,
    room_id: &str,
    listener_member_id: &str,
    publisher_member_id: &str,
    listening: bool,
) -> Result<MemberListeningState> {
    let mut rooms = self.write_rooms()?;
    let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
    if listener_member_id == publisher_member_id {
        return Err(Error::InvalidMessage("不能屏蔽自己的语音".to_string()));
    }
    if !room.members.contains_key(publisher_member_id) {
        return Err(Error::MemberNotFound);
    }

    let listener = room
        .members
        .get_mut(listener_member_id)
        .ok_or(Error::MemberNotFound)?;
    if listening {
        listener.not_listening_member_ids.remove(publisher_member_id);
    } else {
        listener
            .not_listening_member_ids
            .insert(publisher_member_id.to_string());
    }
    room.last_active_epoch_seconds = now_epoch_seconds();

    Ok(MemberListeningState {
        not_listening_member_ids: listener.not_listening_member_ids(),
    })
}
```

Initialize the set in `new_member`, and make both ordinary leave paths remove the departed member ID from every remaining member before returning the new `Room` snapshot:

```rust
fn remove_listening_references(room: &mut Room, member_id: &str) {
    for member in room.members.values_mut() {
        member.not_listening_member_ids.remove(member_id);
    }
}
```

- [ ] **Step 4: Run room-domain tests and verify GREEN**

Run:

```bash
cargo test --test room_permissions
```

Expected: PASS.

- [ ] **Step 5: Commit the domain state**

```bash
git add src/domain/room.rs tests/room_permissions.rs
git commit -m "feat: store member listening preferences"
```

### Task 2: SFU Downlink Listening Policy

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Write failing media tests**

Add focused tests near the existing downlink fanout tests in `src/media/mod.rs`:

```rust
#[tokio::test]
async fn 发布者音频跳过不听该成员的听众() {
    let media = MediaController::new().expect("创建媒体控制器");
    for member_id in ["publisher-1", "listener-1", "listener-2"] {
        media
            .handle_offer("room-1", member_id, create_audio_offer().await)
            .await
            .expect("建立媒体会话");
    }
    media
        .set_member_listening("room-1", "listener-1", "publisher-1", false)
        .await
        .expect("听众屏蔽发布者");

    media
        .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
        .await
        .expect("挂发布者音轨");

    assert_eq!(
        media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("屏蔽听众存在")
            .outbound_track_count,
        0
    );
    assert_eq!(
        media
            .session_snapshot("room-1", "listener-2")
            .await
            .expect("普通听众存在")
            .outbound_track_count,
        1
    );
}

#[tokio::test]
async fn 听众停止并恢复接收已存在发布者音轨() {
    let media = MediaController::new().expect("创建媒体控制器");
    for member_id in ["publisher-1", "listener-1"] {
        media
            .handle_offer("room-1", member_id, create_audio_offer().await)
            .await
            .expect("建立媒体会话");
    }
    media
        .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
        .await
        .expect("挂发布者音轨");

    media
        .set_member_listening("room-1", "listener-1", "publisher-1", false)
        .await
        .expect("停止接收");
    assert_eq!(
        media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众存在")
            .outbound_track_count,
        0
    );

    media
        .set_member_listening("room-1", "listener-1", "publisher-1", true)
        .await
        .expect("恢复接收");
    assert_eq!(
        media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众存在")
            .outbound_track_count,
        1
    );
}
```

- [ ] **Step 2: Run the media tests and verify RED**

Run:

```bash
cargo test media::tests::发布者音频跳过不听该成员的听众
cargo test media::tests::听众停止并恢复接收已存在发布者音轨
```

Expected: FAIL because `MediaController::set_member_listening` does not exist and fanout ignores listener policy.

- [ ] **Step 3: Add listener policy storage and policy checks**

Extend `MediaController` in `src/media/mod.rs` with a listener policy map keyed by `(room_id, listener_member_id)`:

```rust
member_not_listening: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
```

Add the public policy update method:

```rust
pub async fn set_member_listening(
    &self,
    room_id: &str,
    listener_member_id: &str,
    publisher_member_id: &str,
    listening: bool,
) -> Result<()> {
    let key = (room_id.to_string(), listener_member_id.to_string());
    {
        let mut policies = self.member_not_listening.lock().await;
        let blocked = policies.entry(key.clone()).or_default();
        if listening {
            blocked.remove(publisher_member_id);
        } else {
            blocked.insert(publisher_member_id.to_string());
        }
    }

    if listening {
        attach_existing_publisher_audio_to_subscriber(
            Arc::clone(&self.sessions),
            Arc::clone(&self.member_not_listening),
            room_id,
            listener_member_id,
            publisher_member_id,
        )
        .await?;
    } else {
        detach_publisher_audio_from_subscriber(
            Arc::clone(&self.sessions),
            room_id,
            listener_member_id,
            publisher_member_id,
        )
        .await?;
    }

    Ok(())
}
```

Thread the policy map through `attach_audio_to_subscribers` and `attach_existing_audio_to_subscriber`, and skip blocked `(listener, publisher)` pairs:

```rust
async fn listener_accepts_publisher(
    policies: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
    listener_key: &SessionKey,
    publisher_member_id: &str,
) -> bool {
    !policies
        .lock()
        .await
        .get(listener_key)
        .is_some_and(|blocked| blocked.contains(publisher_member_id))
}
```

Detach all matching `OutboundTrack` entries on block and call `replace_track(None)` on their downlink senders. Reuse the existing inbound-track fanout data when restoring a specific publisher's existing audio.

Keep the restore test on the same path by teaching the test helper to register its synthetic publisher track before it fans out:

```rust
async fn attach_audio_to_subscribers_for_test(
    &self,
    room_id: &str,
    publisher_member_id: &str,
) -> Result<()> {
    let track_id = format!("{publisher_member_id}:test-audio");
    let fanout_track = Arc::new(TrackLocalStaticRTP::new(
        webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability {
            mime_type: "audio/opus".to_string(),
            clock_rate: 48000,
            channels: 2,
            ..Default::default()
        },
        track_id.clone(),
        format!("room-{room_id}"),
    ));
    self.store_test_inbound_track(room_id, publisher_member_id, Arc::clone(&fanout_track))
        .await?;
    attach_audio_to_subscribers(
        Arc::clone(&self.sessions),
        Arc::clone(&self.member_not_listening),
        room_id,
        publisher_member_id,
        track_id,
        fanout_track,
    )
    .await
}

#[cfg(test)]
async fn store_test_inbound_track(
    &self,
    room_id: &str,
    publisher_member_id: &str,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<()> {
    let key = (room_id.to_string(), publisher_member_id.to_string());
    let mut sessions = self.sessions.lock().await;
    let publisher = sessions
        .get_mut(&key)
        .ok_or_else(|| Error::MemberNotFound)?;
    publisher.inbound_tracks.insert(
        usize::MAX,
        InboundTrack {
            id: "test-audio".to_string(),
            stream_id: format!("room-{room_id}"),
            ssrc: 0,
            mime_type: "audio/opus".to_string(),
            packet_count: 0,
            fanout_track,
        },
    );
    Ok(())
}
```

Keep `store_test_inbound_track` under `#[cfg(test)]`; production restores still reuse actual inbound tracks recorded from `TrackRemote`.

- [ ] **Step 4: Run media tests and verify GREEN**

Run:

```bash
cargo test media::tests::发布者音频跳过不听该成员的听众
cargo test media::tests::听众停止并恢复接收已存在发布者音轨
cargo test media::tests::发布者音频会为同房间其他会话挂下行_track
cargo test media::tests::听众重新_offer_后保留已挂载的下行_track
```

Expected: PASS.

- [ ] **Step 5: Commit the SFU policy**

```bash
git add src/media/mod.rs
git commit -m "feat: filter member downlink audio"
```

### Task 3: WebSocket Private Listening Protocol

**Files:**
- Modify: `src/transport/http/signaling.rs`
- Modify: `tests/signaling_ws.rs`

- [ ] **Step 1: Write failing WebSocket tests**

Add WebSocket integration coverage in `tests/signaling_ws.rs`:

```rust
#[tokio::test]
async fn websocket_监听状态只回给当前听众并在恢复后返回() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut owner_ws, room_id, owner_id, owner_resume_token) =
        connect_create_with_resume(&ws_url, "create-owner", "房主").await;
    let (mut member_ws, member_id) =
        connect_join(&ws_url, &room_id, "join-member", "队友").await;
    let _ = read_until_type(&mut owner_ws, "member_joined").await;

    owner_ws
        .send(Message::Text(
            json!({
                "type": "set_member_listening",
                "request_id": "listen-off",
                "member_id": member_id,
                "listening": false
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送监听控制");

    let updated = read_until_type(&mut owner_ws, "member_listening_updated").await;
    assert_eq!(updated["request_id"], "listen-off");
    assert_eq!(updated["not_listening_member_ids"], json!([member_id]));
    assert!(timeout(Duration::from_millis(200), member_ws.next()).await.is_err());

    owner_ws.close(None).await.expect("关闭房主 ws");
    let (mut resumed, _) = connect_async(&ws_url).await.expect("连接恢复 ws");
    resumed
        .send(Message::Text(
            json!({
                "type": "resume_room",
                "request_id": "resume-owner",
                "room_id": room_id,
                "member_id": owner_id,
                "resume_token": owner_resume_token
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("发送 resume_room");
    let joined = read_until_type(&mut resumed, "joined_room").await;
    assert_eq!(joined["not_listening_member_ids"], json!([member_id]));
}

#[tokio::test]
async fn websocket_监听控制拒绝屏蔽自己() {
    let state = AppState::new(8).expect("创建应用状态");
    let ws_url = spawn_app(state).await;
    let (mut ws, _room_id, owner_id) = connect_create(&ws_url, "create-owner", "房主").await;

    ws.send(Message::Text(
        json!({
            "type": "set_member_listening",
            "request_id": "listen-self",
            "member_id": owner_id,
            "listening": false
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("发送监听控制");

    let error = read_until_type(&mut ws, "error").await;
    assert_eq!(error["request_id"], "listen-self");
    assert_eq!(error["code"], "invalid_message");
}
```

- [ ] **Step 2: Run the new WebSocket tests and verify RED**

Run:

```bash
cargo test --test signaling_ws websocket_监听
```

Expected: FAIL because the protocol has no listening control or private listening-state response.

- [ ] **Step 3: Add protocol types and signaling handling**

Extend `ClientSignal`:

```rust
SetMemberListening {
    request_id: Option<String>,
    member_id: String,
    listening: bool,
},
```

Return the current listener state on join and resume by adding the field to `JoinedRoom`:

```rust
JoinedRoom {
    request_id: String,
    room: Room,
    member_id: String,
    resume_token: String,
    not_listening_member_ids: Vec<String>,
},
```

Add a private update response:

```rust
MemberListeningUpdated {
    request_id: Option<String>,
    not_listening_member_ids: Vec<String>,
},
```

Handle the new control in the WebSocket loop after `SetMemberCanSpeak`:

```rust
ClientSignal::SetMemberListening { request_id, member_id, listening } => {
    let Some((room_id, listener_member_id)) =
        joined_pair(&joined_room_id, &joined_member_id)
    else {
        let _ = send_not_joined(&mut sender, request_id).await;
        continue;
    };

    match state
        .rooms
        .set_member_listening(room_id, listener_member_id, &member_id, listening)
    {
        Ok(listening_state) => {
            match state
                .media
                .set_member_listening(room_id, listener_member_id, &member_id, listening)
                .await
            {
                Ok(()) => {
                    let _ = send_json(
                        &mut sender,
                        &ServerSignal::MemberListeningUpdated {
                            request_id,
                            not_listening_member_ids: listening_state.not_listening_member_ids,
                        },
                    )
                    .await;
                }
                Err(error) => {
                    let _ = send_error(&mut sender, request_id, error).await;
                }
            }
        }
        Err(error) => {
            let _ = send_error(&mut sender, request_id, error).await;
        }
    }
}
```

Build `JoinedRoom` responses with `join.member.not_listening_member_ids()` so create, join, and resume all expose the caller's private state without broadcasting it.

- [ ] **Step 4: Run WebSocket tests and verify GREEN**

Run:

```bash
cargo test --test signaling_ws websocket_监听
cargo test --test signaling_ws
```

Expected: PASS.

- [ ] **Step 5: Commit the protocol**

```bash
git add src/transport/http/signaling.rs tests/signaling_ws.rs
git commit -m "feat: add private listening signaling"
```

### Task 4: Browser Member Listening Controls

**Files:**
- Modify: `static/room-controls.mjs`
- Modify: `tests/frontend/room-controls.test.mjs`
- Modify: `static/room.js`
- Modify: `static/styles.css`

- [ ] **Step 1: Write failing browser control tests**

Extend `tests/frontend/room-controls.test.mjs`:

```javascript
import {
  canToggleMemberListening,
  memberListeningLabel,
  memberListeningSignal,
} from "../../static/room-controls.mjs";

test("member listening controls describe current private receive choice", () => {
  assert.deepEqual(memberListeningSignal("m_member", false), {
    type: "set_member_listening",
    member_id: "m_member",
    listening: false,
  });
  assert.equal(canToggleMemberListening("m_owner", { id: "m_member" }), true);
  assert.equal(canToggleMemberListening("m_owner", { id: "m_owner" }), false);
  assert.equal(memberListeningLabel(false), "不听");
  assert.equal(memberListeningLabel(true), "接收");
});
```

- [ ] **Step 2: Run browser control tests and verify RED**

Run:

```bash
node --test tests/frontend/room-controls.test.mjs
```

Expected: FAIL because the listening helpers do not exist yet.

- [ ] **Step 3: Add DOM-free listening helpers**

Update `static/room-controls.mjs`:

```javascript
export function memberListeningSignal(memberId, listening) {
  return {
    type: "set_member_listening",
    member_id: memberId,
    listening,
  };
}

export function canToggleMemberListening(ownMemberId, member) {
  return Boolean(ownMemberId && member?.id && member.id !== ownMemberId);
}

export function memberListeningLabel(notListening) {
  return notListening ? "接收" : "不听";
}
```

- [ ] **Step 4: Render the private listening state in the room page**

Keep the blocked member IDs in `static/room.js` as a current-connection private state:

```javascript
let notListeningMemberIds = new Set();

function rememberListeningState(memberIds = []) {
  notListeningMemberIds = new Set(memberIds);
}
```

Initialize it from the successful `joined_room` response:

```javascript
rememberListeningState(joined.not_listening_member_ids);
```

Handle private update messages before public room snapshot handling:

```javascript
if (signal.type === "member_listening_updated") {
  rememberListeningState(signal.not_listening_member_ids);
  if (currentRoom) {
    renderRoom(currentRoom);
  }
  return;
}
```

Append a listener toggle beside existing member row controls:

```javascript
const canToggleListening = canToggleMemberListening(ownMemberId, member);
if (canToggleListening) {
  const notListening = notListeningMemberIds.has(member.id);
  const listening = textNode(
    "button",
    "member-toggle member-listening-toggle",
    memberListeningLabel(notListening),
  );
  listening.type = "button";
  listening.addEventListener("click", () => {
    sendRoomControl(memberListeningSignal(member.id, notListening));
  });
  signals.append(listening);
}
```

Import the new helpers at the top of `room.js`. Reset `notListeningMemberIds` when the room closes or a join fails after clearing the stored room session.

Update `static/styles.css` only as needed so `.member-signals` can wrap both controls and narrow buttons keep readable minimum widths:

```css
.member-listening-toggle {
  min-width: 52px;
}
```

- [ ] **Step 5: Run frontend tests and verify GREEN**

Run:

```bash
node --test tests/frontend/room-controls.test.mjs tests/frontend/room-state.test.mjs
```

Expected: PASS.

- [ ] **Step 6: Commit the browser controls**

```bash
git add static/room-controls.mjs tests/frontend/room-controls.test.mjs static/room.js static/styles.css
git commit -m "feat: add member listening controls"
```

### Task 5: End-to-End Verification

**Files:**
- Verify existing changed files from Tasks 1-4.

- [ ] **Step 1: Run the full Rust suite**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 2: Run all frontend module tests**

Run:

```bash
node --test tests/frontend/*.test.mjs
```

Expected: PASS.

- [ ] **Step 3: Inspect the final worktree diff**

Run:

```bash
git status --short
git diff --stat HEAD
```

Expected: only the listening-control implementation commits and any pre-existing unrelated untracked `.idea/` or archive files remain outside the diff.

- [ ] **Step 4: Manual browser smoke check**

Run the local service:

```bash
cargo run
```

Open two room tabs, then verify:

1. Tab A shows `不听` for Tab B and never shows that button for A itself.
2. Clicking `不听` changes the button to `接收`.
3. Refreshing Tab A restores the same member and keeps the button at `接收`.
4. Explicitly leaving Tab A and joining again creates a new member choice with the button back at `不听`.
5. Narrow viewport member controls wrap without overlapping.

Expected: the page remains connected, the public speaking status labels stay unchanged, and the current listener private button state follows the server-confirmed listening state.
