use crate::{
    Error, Result,
    auth::CurrentUser,
    domain::room::{ChatMention, ChatMessage, MediaRoute, MediaRouteReason, Room},
    media::{IceCandidate, MediaEvent},
    service::media_route::P2pForwardKind,
    state::AppState,
};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, future::pending, sync::RwLock};
use tokio::sync::mpsc;
use tracing::error;

const SIGNAL_QUEUE_CAPACITY: usize = 256;

type RoomSignalSenders = HashMap<String, HashMap<String, mpsc::Sender<ServerSignal>>>;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientSignal {
    CreateRoom {
        request_id: String,
        nickname: String,
    },
    JoinRoom {
        request_id: String,
        room_id: String,
        nickname: String,
    },
    ResumeRoom {
        request_id: String,
        room_id: String,
        member_id: String,
        resume_token: String,
    },
    LeaveRoom {
        request_id: Option<String>,
    },
    SetSelfMuted {
        request_id: Option<String>,
        self_muted: bool,
    },
    SetMemberCanSpeak {
        request_id: Option<String>,
        member_id: String,
        can_speak: bool,
    },
    SetMemberListening {
        request_id: Option<String>,
        member_id: String,
        listening: bool,
    },
    SetMemberSpeaking {
        request_id: Option<String>,
        speaking: bool,
    },
    SetMemberLatency {
        request_id: Option<String>,
        server_ms: f64,
    },
    SetScreenViewing {
        request_id: Option<String>,
        viewing: bool,
    },
    SendChatMessage {
        request_id: Option<String>,
        content: String,
        #[serde(default)]
        mentions: Vec<ChatMention>,
    },
    StartScreenShare {
        request_id: Option<String>,
    },
    StopScreenShare {
        request_id: Option<String>,
    },
    StartVideoCall {
        request_id: Option<String>,
    },
    StopVideoCall {
        request_id: Option<String>,
    },
    // 浏览器发给后端 PeerConnection 的 offer；不再携带目标成员，也不会被转发给其他成员。
    WebrtcOffer {
        request_id: Option<String>,
        sdp: String,
    },
    // 当前 SFU MVP 中后端不主动发起 offer，因此客户端 answer 会被拒绝。
    WebrtcAnswer {
        request_id: Option<String>,
        sdp: String,
    },
    // 浏览器 trickle ICE 的原始结构，保留 sdpMid/sdpMLineIndex 等字段给 webrtc-rs。
    IceCandidate {
        request_id: Option<String>,
        candidate: IceCandidate,
    },
    // 成员间 P2P offer 只由后端校验并定向转发，不复用 SFU 的 webrtc_offer 语义。
    P2pOffer {
        request_id: String,
        target_member_id: String,
        sdp: String,
    },
    // 成员间 P2P answer 只发给目标成员，发送者身份由当前 WebSocket 会话决定。
    P2pAnswer {
        request_id: String,
        target_member_id: String,
        sdp: String,
    },
    // 成员间 P2P ICE candidate 保持浏览器结构，但只转发给指定目标成员。
    P2pIceCandidate {
        request_id: String,
        target_member_id: String,
        candidate: IceCandidate,
    },
    // P2P 建连失败只把这一对成员切回 SFU，不能影响房间里的其他成员对。
    P2pConnectionFailed {
        request_id: String,
        target_member_id: String,
        reason: String,
    },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerSignal {
    JoinedRoom {
        request_id: String,
        room: Room,
        member_id: String,
        resume_token: String,
        not_listening_member_ids: Vec<String>,
        chat_messages: Vec<ChatMessage>,
    },
    MemberJoined {
        room: Room,
        member_id: String,
    },
    MemberLeft {
        room: Room,
        member_id: String,
    },
    RoomClosed {
        room_id: String,
    },
    MemberUpdated {
        room: Room,
        member_id: String,
    },
    MemberListeningUpdated {
        request_id: Option<String>,
        not_listening_member_ids: Vec<String>,
    },
    MemberSpeakingUpdated {
        member_id: String,
        speaking: bool,
    },
    MemberLatencyUpdated {
        member_id: String,
        server_ms: f64,
    },
    ChatMessageSent {
        request_id: Option<String>,
        message: ChatMessage,
    },
    ChatMessage {
        message: ChatMessage,
    },
    ScreenShareStarted {
        member_id: String,
        nickname: String,
    },
    ScreenShareStopped {
        member_id: String,
    },
    ScreenShareViewerCountUpdated {
        member_id: String,
        viewer_count: usize,
    },
    VideoCallStarted {
        member_id: String,
        nickname: String,
    },
    VideoCallStopped {
        member_id: String,
    },
    VideoCallPublisherCountUpdated {
        publisher_count: usize,
    },
    WebrtcAnswer {
        request_id: Option<String>,
        sdp: String,
    },
    // 有新的服务端下行 track 可订阅时，客户端需要重新发 offer 让 answer 带上新 m-line。
    RenegotiationNeeded {
        member_id: String,
    },
    // 服务端 PeerConnection 产出的本地 candidate，只回给当前协商的 WebSocket。
    IceCandidate {
        candidate: IceCandidate,
    },
    // 成员间 P2P offer 下行只暴露真实发送者，避免客户端伪造来源。
    P2pOffer {
        from_member_id: String,
        sdp: String,
    },
    // 成员间 P2P answer 下行只发给目标成员，不广播给整个房间。
    P2pAnswer {
        from_member_id: String,
        sdp: String,
    },
    // 成员间 P2P ICE candidate 下行保留浏览器 candidate 字段。
    P2pIceCandidate {
        from_member_id: String,
        candidate: IceCandidate,
    },
    // 媒体路由变化按规范化成员对广播，前端据此关闭对应 P2P 链路。
    MediaRouteUpdated {
        member_ids: Vec<String>,
        route: MediaRoute,
        reason: MediaRouteReason,
    },
    Error {
        request_id: Option<String>,
        code: &'static str,
        message: String,
    },
}

#[derive(Debug)]
pub struct SignalHub {
    // 这里只保存房间事件通道，不承载媒体包；P2P 也只转发信令，不接触媒体帧。
    rooms: RwLock<RoomSignalSenders>,
}

impl SignalHub {
    pub fn new() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, room_id: &str, member_id: &str) -> Result<mpsc::Receiver<ServerSignal>> {
        let (sender, receiver) = mpsc::channel(SIGNAL_QUEUE_CAPACITY);
        let mut rooms = self.write_rooms()?;
        let members = rooms.entry(room_id.to_string()).or_default();

        if let Some(existing_sender) = members.get(member_id) {
            if !existing_sender.is_closed() {
                return Err(Error::InvalidMessage("成员已连接".to_string()));
            }
        }

        members.insert(member_id.to_string(), sender);

        Ok(receiver)
    }

    pub fn unregister(&self, room_id: &str, member_id: &str) -> Result<()> {
        let mut rooms = self.write_rooms()?;
        let should_remove_room = match rooms.get_mut(room_id) {
            Some(members) => {
                members.remove(member_id);
                members.is_empty()
            }
            None => false,
        };

        if should_remove_room {
            rooms.remove(room_id);
        }

        Ok(())
    }

    pub fn clear_room(&self, room_id: &str) -> Result<()> {
        self.write_rooms()?.remove(room_id);
        Ok(())
    }

    /// 确认目标成员仍有活跃信令连接，避免 P2P 失败上报写入不可投递的成员对。
    pub fn ensure_member_registered(&self, room_id: &str, member_id: &str) -> Result<()> {
        let mut rooms = self.write_rooms()?;
        let Some(members) = rooms.get_mut(room_id) else {
            return Err(Error::InvalidMessage("目标成员信令连接不可用".to_string()));
        };

        let registered = members
            .get(member_id)
            .is_some_and(|sender| !sender.is_closed());
        if registered {
            return Ok(());
        }

        members.remove(member_id);
        if members.is_empty() {
            rooms.remove(room_id);
        }
        Err(Error::InvalidMessage("目标成员信令连接不可用".to_string()))
    }

    /// 向房间内指定成员定向发送信令，用于 P2P offer/answer/ICE 这类非广播消息。
    pub fn send_to_member(
        &self,
        room_id: &str,
        member_id: &str,
        signal: ServerSignal,
    ) -> Result<()> {
        let mut rooms = self.write_rooms()?;
        let Some(members) = rooms.get_mut(room_id) else {
            return Err(Error::InvalidMessage("目标成员信令连接不可用".to_string()));
        };

        let Some(sender) = members.get(member_id) else {
            return Err(Error::InvalidMessage("目标成员信令连接不可用".to_string()));
        };

        if sender.try_send(signal).is_ok() {
            return Ok(());
        }

        members.remove(member_id);
        if members.is_empty() {
            rooms.remove(room_id);
        }
        Err(Error::InvalidMessage("目标成员信令连接不可用".to_string()))
    }

    pub fn broadcast(
        &self,
        room_id: &str,
        signal: ServerSignal,
        excluded_member_id: Option<&str>,
    ) -> Result<()> {
        let mut rooms = self.write_rooms()?;
        let Some(members) = rooms.get_mut(room_id) else {
            return Ok(());
        };

        members.retain(|member_id, sender| {
            if excluded_member_id == Some(member_id.as_str()) {
                return true;
            }

            sender.try_send(signal.clone()).is_ok()
        });

        if members.is_empty() {
            rooms.remove(room_id);
        }

        Ok(())
    }

    fn write_rooms(&self) -> Result<std::sync::RwLockWriteGuard<'_, RoomSignalSenders>> {
        self.rooms
            .write()
            .map_err(|_| Error::Internal("信令房间写锁已损坏".to_string()))
    }
}

pub async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let current_user = if state.auth.is_enabled() {
        match super::auth::current_user_from_headers(&state, &headers) {
            Ok(user) => Some(user),
            Err(_) => return Error::Unauthenticated.into_response(),
        }
    } else {
        None
    };

    upgrade
        .on_upgrade(move |socket| handle_socket(state, socket, current_user))
        .into_response()
}

async fn handle_socket(state: AppState, socket: WebSocket, current_user: Option<CurrentUser>) {
    let (mut sender, mut receiver) = socket.split();
    let mut joined_room_id: Option<String> = None;
    let mut joined_member_id: Option<String> = None;
    let mut outbound: Option<mpsc::Receiver<ServerSignal>> = None;
    let mut local_ice_candidates: Option<mpsc::Receiver<IceCandidate>> = None;
    let mut media_events = state.media.subscribe_events();
    let mut explicit_leave = false;

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else {
                    break;
                };

                let Message::Text(text) = message else {
                    if matches!(message, Message::Close(_)) {
                        break;
                    }
                    continue;
                };

                // `deny_unknown_fields` 继续保护 SFU webrtc_* 消息；成员间协商只能走新的 p2p_* 类型。
                let signal = match serde_json::from_str::<ClientSignal>(&text) {
                    Ok(signal) => signal,
                    Err(error) => {
                        let _ = send_json(&mut sender, &ServerSignal::Error {
                            request_id: request_id_from_text(&text),
                            code: "invalid_message",
                            message: format!("消息格式无效: {error}"),
                        }).await;
                        continue;
                    }
                };

                match signal {
                    ClientSignal::CreateRoom { request_id, nickname } => {
                        if joined_room_id.is_some() {
                            let _ = send_json(&mut sender, &ServerSignal::Error {
                                request_id: Some(request_id),
                                code: "invalid_message",
                                message: "已经加入房间".to_string(),
                            }).await;
                            continue;
                        }

                        let join = match state
                            .services
                            .room_lifecycle
                            .create_room(nickname, current_user.as_ref())
                        {
                            Ok(join) => join,
                            Err(error) => {
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                                continue;
                            }
                        };

                        let room_id = join.room.id.clone();
                        let member_id = join.member.id.clone();
                        let room_receiver = match state.signals.register(&room_id, &member_id) {
                            Ok(receiver) => receiver,
                            Err(error) => {
                                state
                                    .services
                                    .room_lifecycle
                                    .rollback_join_after_register_failure(&room_id, &member_id);
                                close_persistent_room_for_owner_if_enabled(
                                    &state,
                                    &room_id,
                                    current_user.as_ref(),
                                );
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                                continue;
                            }
                        };

                        joined_room_id = Some(room_id);
                        joined_member_id = Some(member_id.clone());
                        outbound = Some(room_receiver);

                        let _ = send_json(&mut sender, &ServerSignal::JoinedRoom {
                            request_id,
                            room: join.room,
                            member_id,
                            resume_token: join.resume_token,
                            not_listening_member_ids: join.member.not_listening_member_ids(),
                            chat_messages: Vec::new(),
                        }).await;
                    }
                    ClientSignal::JoinRoom { request_id, room_id, nickname } => {
                        if joined_room_id.is_some() {
                            let _ = send_json(&mut sender, &ServerSignal::Error {
                                request_id: Some(request_id),
                                code: "invalid_message",
                                message: "已经加入房间".to_string(),
                            }).await;
                            continue;
                        }

                        let join = match state
                            .services
                            .room_lifecycle
                            .join_room(&room_id, nickname, current_user.as_ref())
                        {
                            Ok(join) => join,
                            Err(error) => {
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                                continue;
                            }
                        };

                        let member_id = join.member.id.clone();
                        let room_receiver = match state.signals.register(&room_id, &member_id) {
                            Ok(receiver) => receiver,
                            Err(error) => {
                                state
                                    .services
                                    .room_lifecycle
                                    .rollback_join_after_register_failure(&room_id, &member_id);
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                                continue;
                            }
                        };

                        joined_room_id = Some(room_id.clone());
                        joined_member_id = Some(member_id.clone());
                        outbound = Some(room_receiver);

                        let _ = send_json(&mut sender, &ServerSignal::JoinedRoom {
                            request_id,
                            room: join.room.clone(),
                            member_id: member_id.clone(),
                            resume_token: join.resume_token,
                            not_listening_member_ids: join.member.not_listening_member_ids(),
                            chat_messages: state.services.room_lifecycle.chat_history(&room_id),
                        }).await;

                        let _ = state.signals.broadcast(
                            &room_id,
                            ServerSignal::MemberJoined {
                                room: join.room,
                                member_id: member_id.clone(),
                            },
                            Some(&member_id),
                        );
                    }
                    ClientSignal::ResumeRoom { request_id, room_id, member_id, resume_token } => {
                        if joined_room_id.is_some() {
                            let _ = send_json(&mut sender, &ServerSignal::Error {
                                request_id: Some(request_id),
                                code: "invalid_message",
                                message: "已经加入房间".to_string(),
                            }).await;
                            continue;
                        }

                        let join = match state
                            .services
                            .room_lifecycle
                            .resume_room(&room_id, &member_id, &resume_token, current_user.as_ref())
                        {
                            Ok(join) => join,
                            Err(error) => {
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                                continue;
                            }
                        };

                        let room_receiver = match state.signals.register(&room_id, &member_id) {
                            Ok(receiver) => receiver,
                            Err(error) => {
                                let _ = send_error(&mut sender, Some(request_id), error).await;
                                continue;
                            }
                        };
                        if let Err(error) = state
                            .services
                            .member_controls
                            .sync_room_media_policies(&join.room)
                            .await
                        {
                            error!(room_id, member_id, %error, "恢复成员媒体策略同步失败");
                        }

                        joined_room_id = Some(room_id.clone());
                        joined_member_id = Some(member_id.clone());
                        outbound = Some(room_receiver);

                        let _ = send_json(&mut sender, &ServerSignal::JoinedRoom {
                            request_id,
                            room: join.room.clone(),
                            member_id: member_id.clone(),
                            resume_token: join.resume_token,
                            not_listening_member_ids: join.member.not_listening_member_ids(),
                            chat_messages: state.services.room_lifecycle.chat_history(&room_id),
                        }).await;

                        let _ = state.signals.broadcast(
                            &room_id,
                            ServerSignal::MemberUpdated {
                                room: join.room,
                                member_id: member_id.clone(),
                            },
                            Some(&member_id),
                        );
                    }
                    ClientSignal::LeaveRoom { request_id } => {
                        if joined_room_id.is_none() {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        }

                        explicit_leave = true;
                        break;
                    }
                    ClientSignal::SetSelfMuted { request_id, self_muted } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state.services.member_controls.set_self_muted(room_id, member_id, self_muted) {
                            Ok(outcome) => {
                                let _ = state.signals.broadcast(
                                    room_id,
                                    ServerSignal::MemberUpdated {
                                        room: outcome.room,
                                        member_id: outcome.member_id.clone(),
                                    },
                                    None,
                                );
                                if outcome.force_speaking_false {
                                    let _ = state.signals.broadcast(
                                        room_id,
                                        ServerSignal::MemberSpeakingUpdated {
                                            member_id: outcome.member_id,
                                            speaking: false,
                                        },
                                        None,
                                    );
                                }
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::SetMemberCanSpeak { request_id, member_id, can_speak } => {
                        let Some((room_id, actor_member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .member_controls
                            .set_member_can_speak(room_id, actor_member_id, &member_id, can_speak)
                            .await
                        {
                            Ok(outcome) => {
                                let _ = state.signals.broadcast(
                                    room_id,
                                    ServerSignal::MemberUpdated {
                                        room: outcome.room,
                                        member_id: outcome.member_id.clone(),
                                    },
                                    None,
                                );
                                if outcome.force_speaking_false {
                                    let _ = state.signals.broadcast(
                                        room_id,
                                        ServerSignal::MemberSpeakingUpdated {
                                            member_id: outcome.member_id,
                                            speaking: false,
                                        },
                                        None,
                                    );
                                }
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::SetMemberListening { request_id, member_id, listening } => {
                        let Some((room_id, listener_member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .member_controls
                            .set_member_listening(
                                room_id,
                                listener_member_id,
                                &member_id,
                                listening,
                                request_id.clone(),
                            )
                            .await
                        {
                            Ok(outcome) => {
                                let _ = send_json(
                                    &mut sender,
                                    &ServerSignal::MemberListeningUpdated {
                                        request_id: outcome.request_id,
                                        not_listening_member_ids: outcome
                                            .state
                                            .not_listening_member_ids,
                                    },
                                )
                                .await;
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::SetMemberSpeaking { request_id, speaking } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        let speaking = state
                            .services
                            .member_controls
                            .normalized_speaking(room_id, member_id, speaking);
                        let _ = state.signals.broadcast(
                            room_id,
                            ServerSignal::MemberSpeakingUpdated {
                                member_id: member_id.to_string(),
                                speaking,
                            },
                            None,
                        );
                    }
                    ClientSignal::SetMemberLatency { request_id, server_ms } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };
                        let server_ms = match state.services.member_controls.validate_latency(server_ms) {
                            Ok(server_ms) => server_ms,
                            Err(error) => {
                                let _ = send_error(
                                    &mut sender,
                                    request_id,
                                    error,
                                )
                                .await;
                                continue;
                            }
                        };
                        let _ = state.signals.broadcast(
                            room_id,
                            ServerSignal::MemberLatencyUpdated {
                                member_id: member_id.to_string(),
                                server_ms,
                            },
                            None,
                        );
                    }
                    ClientSignal::SetScreenViewing { request_id, viewing } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .media_routes
                            .set_screen_viewing(room_id, member_id, viewing)
                            .await
                        {
                            Ok(viewer_count) => {
                                broadcast_screen_viewer_count(
                                    &state,
                                    room_id,
                                    viewer_count,
                                )
                                .await;
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::SendChatMessage { request_id, content, mentions } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state.services.chat.send_message(
                            room_id,
                            member_id,
                            &content,
                            mentions,
                            request_id.clone(),
                        ) {
                            Ok(outcome) => {
                                let _ = send_json(
                                    &mut sender,
                                    &ServerSignal::ChatMessageSent {
                                        request_id: outcome.request_id,
                                        message: outcome.message.clone(),
                                    },
                                )
                                .await;
                                let _ = state.signals.broadcast(
                                    room_id,
                                    ServerSignal::ChatMessage { message: outcome.message },
                                    Some(member_id),
                                );
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::StartScreenShare { request_id } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .media_routes
                            .start_screen_share(room_id, member_id)
                            .await
                        {
                            Ok(outcome) => {
                                if let Some(screen_share) = outcome.screen_share {
                                    let _ = state.signals.broadcast(
                                        room_id,
                                        ServerSignal::ScreenShareStarted {
                                            member_id: screen_share.member_id,
                                            nickname: screen_share.nickname,
                                        },
                                        None,
                                    );
                                    if outcome.viewer_count > 0 {
                                        broadcast_screen_viewer_count(&state, room_id, outcome.viewer_count).await;
                                    }
                                }
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::StopScreenShare { request_id } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .media_routes
                            .stop_screen_share(room_id, member_id)
                            .await
                        {
                            Ok(outcome) => {
                                if let Some(stopped_member_id) = outcome.stopped_member_id {
                                    let _ = state.signals.broadcast(
                                        room_id,
                                        ServerSignal::ScreenShareStopped {
                                            member_id: stopped_member_id,
                                        },
                                        None,
                                    );
                                    broadcast_screen_viewer_count(&state, room_id, 0).await;
                                }
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::StartVideoCall { request_id } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .media_routes
                            .start_video_call(room_id, member_id)
                            .await
                        {
                            Ok(outcome) => {
                                if let Some(publisher) = outcome.publisher {
                                    let _ = state.signals.broadcast(
                                        room_id,
                                        ServerSignal::VideoCallStarted {
                                            member_id: publisher.member_id,
                                            nickname: publisher.nickname,
                                        },
                                        None,
                                    );
                                }
                                broadcast_video_call_publisher_count(
                                    &state,
                                    room_id,
                                    outcome.publisher_count,
                                )
                                .await;
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::StopVideoCall { request_id } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        match state
                            .services
                            .media_routes
                            .stop_video_call(room_id, member_id)
                            .await
                        {
                            Ok(outcome) => {
                                if outcome.stopped {
                                    let _ = state.signals.broadcast(
                                        room_id,
                                        ServerSignal::VideoCallStopped {
                                            member_id: member_id.to_string(),
                                        },
                                        None,
                                    );
                                    broadcast_video_call_publisher_count(
                                        &state,
                                        room_id,
                                        outcome.publisher_count,
                                    )
                                    .await;
                                }
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::WebrtcOffer { request_id, sdp } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        // SFU 模式下 offer 只交给后端媒体层；answer 和本地 ICE 再回给同一个 socket。
                        match state
                            .services
                            .media_routes
                            .handle_sfu_offer(room_id, member_id, sdp)
                            .await
                        {
                            Ok(answer) => {
                                local_ice_candidates = Some(answer.local_ice_candidates);
                                let _ = send_json(
                                    &mut sender,
                                    &ServerSignal::WebrtcAnswer {
                                        request_id,
                                        sdp: answer.sdp,
                                    },
                                )
                                .await;
                            }
                            Err(error) => {
                                let _ = send_error(&mut sender, request_id, error).await;
                            }
                        }
                    }
                    ClientSignal::WebrtcAnswer { request_id, sdp: _ } => {
                        let _ = send_error(
                            &mut sender,
                            request_id,
                            Error::InvalidMessage("服务端未发起 offer，不能接收 webrtc_answer".to_string()),
                        )
                        .await;
                    }
                    ClientSignal::IceCandidate { request_id, candidate } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        // 这是浏览器发给后端 PeerConnection 的远端 candidate，不会广播给其他成员。
                        if let Err(error) = state
                            .services
                            .media_routes
                            .add_sfu_ice_candidate(room_id, member_id, candidate)
                            .await
                        {
                            let _ = send_error(&mut sender, request_id, error).await;
                        }
                    }
                    ClientSignal::P2pOffer { request_id, target_member_id, sdp } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, Some(request_id)).await;
                            continue;
                        };

                        // P2P offer 只校验并定向转发给目标成员，SFU 媒体层不参与浏览器间协商。
                        let result = state
                            .services
                            .media_routes
                            .forward_p2p_signal(
                                room_id,
                                member_id,
                                &target_member_id,
                                P2pForwardKind::Offer { sdp },
                            )
                            .and_then(|outcome| {
                                state.signals.send_to_member(
                                    room_id,
                                    &outcome.target_member_id,
                                    ServerSignal::P2pOffer {
                                        from_member_id: outcome.from_member_id,
                                        sdp: match outcome.kind {
                                            P2pForwardKind::Offer { sdp } => sdp,
                                            _ => unreachable!("P2P offer outcome kind"),
                                        },
                                    },
                                )
                            });
                        if let Err(error) = result {
                            let _ = send_error(
                                &mut sender,
                                Some(request_id),
                                error,
                            ).await;
                        }
                    }
                    ClientSignal::P2pAnswer { request_id, target_member_id, sdp } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, Some(request_id)).await;
                            continue;
                        };

                        // P2P answer 只回到发起 offer 的成员，避免泄露给房间内其他连接。
                        let result = state
                            .services
                            .media_routes
                            .forward_p2p_signal(
                                room_id,
                                member_id,
                                &target_member_id,
                                P2pForwardKind::Answer { sdp },
                            )
                            .and_then(|outcome| {
                                state.signals.send_to_member(
                                    room_id,
                                    &outcome.target_member_id,
                                    ServerSignal::P2pAnswer {
                                        from_member_id: outcome.from_member_id,
                                        sdp: match outcome.kind {
                                            P2pForwardKind::Answer { sdp } => sdp,
                                            _ => unreachable!("P2P answer outcome kind"),
                                        },
                                    },
                                )
                            });
                        if let Err(error) = result {
                            let _ = send_error(
                                &mut sender,
                                Some(request_id),
                                error,
                            ).await;
                        }
                    }
                    ClientSignal::P2pIceCandidate { request_id, target_member_id, candidate } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, Some(request_id)).await;
                            continue;
                        };

                        // P2P ICE candidate 复用浏览器结构，但只进入目标成员的信令队列。
                        let result = state
                            .services
                            .media_routes
                            .forward_p2p_signal(
                                room_id,
                                member_id,
                                &target_member_id,
                                P2pForwardKind::IceCandidate { candidate },
                            )
                            .and_then(|outcome| {
                                state.signals.send_to_member(
                                    room_id,
                                    &outcome.target_member_id,
                                    ServerSignal::P2pIceCandidate {
                                        from_member_id: outcome.from_member_id,
                                        candidate: match outcome.kind {
                                            P2pForwardKind::IceCandidate { candidate } => candidate,
                                            _ => unreachable!("P2P candidate outcome kind"),
                                        },
                                    },
                                )
                            });
                        if let Err(error) = result {
                            let _ = send_error(
                                &mut sender,
                                Some(request_id),
                                error,
                            ).await;
                        }
                    }
                    ClientSignal::P2pConnectionFailed { request_id, target_member_id, reason: _reason } => {
                        let Some((room_id, member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, Some(request_id)).await;
                            continue;
                        };

                        // P2P 失败只切换这一对成员的路由，并广播规范化成员对给前端清理连接。
                        let result = state
                            .services
                            .media_routes
                            .validate_p2p_target(
                                room_id,
                                member_id,
                                &target_member_id,
                            )
                            .and_then(|()| state.signals.ensure_member_registered(room_id, &target_member_id))
                            .and_then(|()| {
                                state.services.media_routes.report_p2p_failure(
                                    room_id,
                                    member_id,
                                    &target_member_id,
                                )
                            });
                        match result {
                            Ok(update) => {
                                let _ = state.signals.broadcast(
                                    room_id,
                                    ServerSignal::MediaRouteUpdated {
                                        member_ids: update.member_ids,
                                        route: update.route,
                                        reason: update.reason,
                                    },
                                    None,
                                );
                            }
                            Err(error) => {
                                let _ = send_error(
                                    &mut sender,
                                    Some(request_id),
                                    error,
                                ).await;
                            }
                        }
                    }
                }
            }
            outgoing = async {
                match outbound.as_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => pending().await,
                }
            }, if outbound.is_some() => {
                let Some(signal) = outgoing else {
                    break;
                };

                if send_json(&mut sender, &signal).await.is_err() {
                    break;
                }
            }
            candidate = async {
                match local_ice_candidates.as_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => pending().await,
                }
            }, if local_ice_candidates.is_some() => {
                let Some(candidate) = candidate else {
                    local_ice_candidates = None;
                    continue;
                };

                // 服务端本地 candidate 只发给当前协商的浏览器。
                if send_json(&mut sender, &ServerSignal::IceCandidate { candidate }).await.is_err() {
                    break;
                }
            }
            media_event = media_events.recv(), if joined_room_id.is_some() => {
                let Ok(media_event) = media_event else {
                    continue;
                };

                let Some(signal) = renegotiation_signal_for_event(
                    &media_event,
                    joined_room_id.as_deref(),
                    joined_member_id.as_deref(),
                ) else {
                    continue;
                };

                if send_json(&mut sender, &signal).await.is_err() {
                    break;
                }
            }
        }
    }

    if let (Some(room_id), Some(member_id)) = (joined_room_id, joined_member_id) {
        let _ = state.media.close_member(&room_id, &member_id).await;
        let viewer_count = state.media.screen_viewer_count(&room_id).await;
        broadcast_screen_viewer_count(&state, &room_id, viewer_count).await;
        let _ = state.signals.unregister(&room_id, &member_id);

        if explicit_leave {
            broadcast_departure(&state, &room_id, &member_id, current_user.as_ref());
        } else {
            broadcast_disconnect(state, room_id, member_id, current_user);
        }
    }
}

// 将非显式 WebSocket 断开转换为可恢复离线状态，避免刷新或热重载误关房间。
fn broadcast_disconnect(
    state: AppState,
    room_id: String,
    member_id: String,
    current_user: Option<CurrentUser>,
) {
    let was_video_publisher = state
        .rooms
        .get_room(&room_id)
        .ok()
        .is_some_and(|room| room.video_call_publishers.contains_key(&member_id));
    match state.rooms.mark_member_disconnected(&room_id, &member_id) {
        Ok(room) => {
            if was_video_publisher {
                let _ = state.signals.broadcast(
                    &room_id,
                    ServerSignal::VideoCallStopped {
                        member_id: member_id.clone(),
                    },
                    None,
                );
                let publisher_count = room.video_call_publishers.len();
                tokio::spawn({
                    let state = state.clone();
                    let room_id = room_id.clone();
                    async move {
                        broadcast_video_call_publisher_count(&state, &room_id, publisher_count)
                            .await;
                    }
                });
            }
            let _ = state.signals.broadcast(
                &room_id,
                ServerSignal::MemberUpdated {
                    room,
                    member_id: member_id.clone(),
                },
                None,
            );
            schedule_disconnected_cleanup(state, room_id, member_id, current_user);
        }
        Err(error) => {
            error!(room_id, member_id, %error, "标记成员断线失败");
        }
    }
}

// 处理用户明确发送 leave_room 的离开语义；房主显式离开会立即关闭房间。
fn broadcast_departure(
    state: &AppState,
    room_id: &str,
    member_id: &str,
    current_user: Option<&CurrentUser>,
) {
    let was_video_publisher = state
        .rooms
        .get_room(room_id)
        .ok()
        .is_some_and(|room| room.video_call_publishers.contains_key(member_id));
    let Ok(room) = state.rooms.leave_room(room_id, member_id) else {
        return;
    };

    if room.owner_member_id == member_id {
        close_persistent_room_for_owner_if_enabled(state, room_id, current_user);
        let _ = state.signals.broadcast(
            room_id,
            ServerSignal::RoomClosed {
                room_id: room_id.to_string(),
            },
            None,
        );
        let _ = state.signals.clear_room(room_id);
    } else {
        if was_video_publisher {
            let _ = state.signals.broadcast(
                room_id,
                ServerSignal::VideoCallStopped {
                    member_id: member_id.to_string(),
                },
                None,
            );
            let publisher_count = room.video_call_publishers.len();
            let state = state.clone();
            let room_id = room_id.to_string();
            tokio::spawn(async move {
                broadcast_video_call_publisher_count(&state, &room_id, publisher_count).await;
            });
        }
        let _ = state.signals.broadcast(
            room_id,
            ServerSignal::MemberLeft {
                room,
                member_id: member_id.to_string(),
            },
            None,
        );
    }
}

// 在断线宽限期后清理仍未恢复的成员；房主超时才关闭持久房间。
fn schedule_disconnected_cleanup(
    state: AppState,
    room_id: String,
    member_id: String,
    current_user: Option<CurrentUser>,
) {
    tokio::spawn(async move {
        tokio::time::sleep(state.disconnect_grace_period).await;

        let was_video_publisher = state
            .rooms
            .get_room(&room_id)
            .ok()
            .is_some_and(|room| room.video_call_publishers.contains_key(&member_id));
        let Ok(Some(room)) = state.rooms.expire_disconnected_member(&room_id, &member_id) else {
            return;
        };

        if room.owner_member_id == member_id {
            close_persistent_room_for_owner_if_enabled(&state, &room_id, current_user.as_ref());
            let _ = state.signals.broadcast(
                &room_id,
                ServerSignal::RoomClosed {
                    room_id: room_id.clone(),
                },
                None,
            );
            let _ = state.signals.clear_room(&room_id);
        } else {
            if was_video_publisher {
                let _ = state.signals.broadcast(
                    &room_id,
                    ServerSignal::VideoCallStopped {
                        member_id: member_id.clone(),
                    },
                    None,
                );
                broadcast_video_call_publisher_count(
                    &state,
                    &room_id,
                    room.video_call_publishers.len(),
                )
                .await;
            }
            let _ = state.signals.broadcast(
                &room_id,
                ServerSignal::MemberLeft { room, member_id },
                None,
            );
        }
    });
}

fn close_persistent_room_for_owner_if_enabled(
    state: &AppState,
    room_id: &str,
    current_user: Option<&CurrentUser>,
) {
    if state.auth.is_enabled() && current_user.is_none() {
        error!(room_id, "认证开启但房主连接缺少用户身份，跳过持久房间关闭");
        return;
    }

    if let Err(error) = state
        .services
        .authenticated_rooms
        .close_as_owner_if_owned(room_id, current_user)
    {
        error!(room_id, %error, "关闭持久房间失败");
    }
}

async fn broadcast_screen_viewer_count(state: &AppState, room_id: &str, viewer_count: usize) {
    let Some(member_id) = state
        .rooms
        .get_room(room_id)
        .ok()
        .and_then(|room| room.screen_share.map(|screen_share| screen_share.member_id))
    else {
        return;
    };

    let _ = state.signals.broadcast(
        room_id,
        ServerSignal::ScreenShareViewerCountUpdated {
            member_id,
            viewer_count,
        },
        None,
    );
}

async fn broadcast_video_call_publisher_count(
    state: &AppState,
    room_id: &str,
    publisher_count: usize,
) {
    let _ = state.signals.broadcast(
        room_id,
        ServerSignal::VideoCallPublisherCountUpdated { publisher_count },
        None,
    );
}

fn request_id_from_text(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("request_id")?.as_str().map(str::to_string))
}

fn joined_pair<'a>(
    room_id: &'a Option<String>,
    member_id: &'a Option<String>,
) -> Option<(&'a str, &'a str)> {
    Some((room_id.as_deref()?, member_id.as_deref()?))
}

fn renegotiation_signal_for_event(
    event: &MediaEvent,
    joined_room_id: Option<&str>,
    joined_member_id: Option<&str>,
) -> Option<ServerSignal> {
    let joined_member_id = joined_member_id?;
    match event {
        MediaEvent::InboundAudioTrack {
            room_id,
            member_id,
            subscriber_member_ids,
        } if joined_room_id == Some(room_id.as_str())
            && subscriber_member_ids
                .iter()
                .any(|member_id| member_id == joined_member_id) =>
        {
            Some(ServerSignal::RenegotiationNeeded {
                member_id: member_id.clone(),
            })
        }
        MediaEvent::InboundScreenVideoTrack {
            room_id,
            member_id,
            subscriber_member_ids,
        }
        | MediaEvent::InboundCameraVideoTrack {
            room_id,
            member_id,
            subscriber_member_ids,
        } if joined_room_id == Some(room_id.as_str())
            && subscriber_member_ids
                .iter()
                .any(|member_id| member_id == joined_member_id) =>
        {
            Some(ServerSignal::RenegotiationNeeded {
                member_id: member_id.clone(),
            })
        }
        _ => None,
    }
}

async fn send_not_joined(
    sender: &mut SplitSink<WebSocket, Message>,
    request_id: Option<String>,
) -> Result<()> {
    send_json(
        sender,
        &ServerSignal::Error {
            request_id,
            code: "invalid_message",
            message: "加入房间后才能发送该消息".to_string(),
        },
    )
    .await
}

async fn send_error(
    sender: &mut SplitSink<WebSocket, Message>,
    request_id: Option<String>,
    error: Error,
) -> Result<()> {
    send_json(
        sender,
        &ServerSignal::Error {
            request_id,
            code: error.code(),
            message: error.to_string(),
        },
    )
    .await
}

async fn send_json(
    sender: &mut SplitSink<WebSocket, Message>,
    signal: &ServerSignal,
) -> Result<()> {
    let text = serde_json::to_string(signal)
        .map_err(|error| Error::Internal(format!("序列化信令失败: {error}")))?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| Error::Internal(format!("发送信令失败: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ClientSignal, SIGNAL_QUEUE_CAPACITY, ServerSignal, SignalHub,
        renegotiation_signal_for_event,
    };
    use crate::{
        Error,
        domain::room::{MediaRoute, MediaRouteReason},
        media::{IceCandidate, MediaEvent},
    };
    use tokio::sync::mpsc::error::TryRecvError;

    fn test_candidate(value: &str) -> IceCandidate {
        IceCandidate {
            candidate: value.to_string(),
            sdp_mid: Some("0".to_string()),
            sdp_mline_index: Some(0),
            username_fragment: None,
        }
    }

    #[test]
    fn 客户端信令消息按_type_字段解析() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"join_room","request_id":"req-1","room_id":"ABC123","nickname":"小明"}"#,
        )
        .expect("解析 join_room 信令");

        assert!(matches!(
            signal,
            ClientSignal::JoinRoom {
                request_id,
                room_id,
                nickname
            } if request_id == "req-1"
                && room_id == "ABC123"
                && nickname == "小明"
        ));
    }

    #[test]
    fn 服务端错误信令包含请求_id() {
        let json = serde_json::to_value(ServerSignal::Error {
            request_id: Some("req-1".to_string()),
            code: "room_not_found",
            message: "房间不存在".to_string(),
        })
        .expect("序列化 error 信令");

        assert_eq!(json["type"], "error");
        assert_eq!(json["request_id"], "req-1");
        assert_eq!(json["code"], "room_not_found");
    }

    #[test]
    fn 客户端_ice_candidate_按浏览器结构解析() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"ice_candidate","request_id":"ice-1","candidate":{"candidate":"candidate:abc","sdpMid":"0","sdpMLineIndex":0,"usernameFragment":"ufrag"}}"#,
        )
        .expect("解析浏览器 ICE candidate");

        assert!(matches!(
            signal,
            ClientSignal::IceCandidate {
                request_id,
                candidate
            } if request_id.as_deref() == Some("ice-1")
                && candidate.candidate == "candidate:abc"
                && candidate.sdp_mid.as_deref() == Some("0")
                && candidate.sdp_mline_index == Some(0)
                && candidate.username_fragment.as_deref() == Some("ufrag")
        ));
    }

    #[test]
    fn 客户端_p2p_ice_candidate_按浏览器结构解析() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"p2p_ice_candidate","request_id":"p2p-ice-1","target_member_id":"m_target","candidate":{"candidate":"candidate:p2p","sdpMid":"0","sdpMLineIndex":0,"usernameFragment":"ufrag"}}"#,
        )
        .expect("解析 P2P ICE candidate");

        assert!(matches!(
            signal,
            ClientSignal::P2pIceCandidate {
                request_id,
                target_member_id,
                candidate
            } if request_id == "p2p-ice-1"
                && target_member_id == "m_target"
                && candidate.candidate == "candidate:p2p"
                && candidate.sdp_mid.as_deref() == Some("0")
                && candidate.sdp_mline_index == Some(0)
                && candidate.username_fragment.as_deref() == Some("ufrag")
        ));
    }

    #[test]
    fn 客户端_set_member_speaking_按布尔状态解析() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"set_member_speaking","request_id":"speak-1","speaking":true}"#,
        )
        .expect("解析发言状态信令");

        assert!(matches!(
            signal,
            ClientSignal::SetMemberSpeaking {
                request_id,
                speaking
            } if request_id.as_deref() == Some("speak-1") && speaking
        ));
    }

    #[test]
    fn 客户端_set_member_latency_按毫秒数解析() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"set_member_latency","request_id":"latency-1","server_ms":28.4}"#,
        )
        .expect("解析成员延迟信令");

        assert!(matches!(
            signal,
            ClientSignal::SetMemberLatency {
                request_id,
                server_ms
            } if request_id.as_deref() == Some("latency-1") && server_ms == 28.4
        ));
    }

    #[test]
    fn 客户端_send_chat_message_解析_mentions() {
        let signal: ClientSignal = serde_json::from_str(
            r#"{"type":"send_chat_message","request_id":"chat-1","content":"@阿木 晚上打哪张图？","mentions":[{"member_id":"m_member","nickname":"阿木"}]}"#,
        )
        .expect("解析聊天 mention 信令");

        assert!(matches!(
            signal,
            ClientSignal::SendChatMessage {
                request_id,
                content,
                mentions
            } if request_id.as_deref() == Some("chat-1")
                && content == "@阿木 晚上打哪张图？"
                && mentions.len() == 1
                && mentions[0].member_id == "m_member"
                && mentions[0].nickname == "阿木"
        ));
    }

    #[test]
    fn 服务端_ice_candidate_按浏览器结构序列化() {
        let json = serde_json::to_value(ServerSignal::IceCandidate {
            candidate: IceCandidate {
                candidate: "candidate:abc".to_string(),
                sdp_mid: Some("0".to_string()),
                sdp_mline_index: Some(0),
                username_fragment: Some("ufrag".to_string()),
            },
        })
        .expect("序列化服务端 ICE candidate");

        assert_eq!(json["type"], "ice_candidate");
        assert_eq!(json["candidate"]["candidate"], "candidate:abc");
        assert_eq!(json["candidate"]["sdpMid"], "0");
        assert_eq!(json["candidate"]["sdpMLineIndex"], 0);
        assert_eq!(json["candidate"]["usernameFragment"], "ufrag");
    }

    #[test]
    fn 服务端_p2p_offer_序列化为真实发送者() {
        let json = serde_json::to_value(ServerSignal::P2pOffer {
            from_member_id: "m_sender".to_string(),
            sdp: "v=0\r\n".to_string(),
        })
        .expect("序列化 P2P offer");

        assert_eq!(json["type"], "p2p_offer");
        assert_eq!(json["from_member_id"], "m_sender");
        assert_eq!(json["sdp"], "v=0\r\n");
    }

    #[test]
    fn 服务端_media_route_updated_序列化为蛇形状态() {
        let json = serde_json::to_value(ServerSignal::MediaRouteUpdated {
            member_ids: vec!["m_a".to_string(), "m_b".to_string()],
            route: MediaRoute::Sfu,
            reason: MediaRouteReason::P2pFailed,
        })
        .expect("序列化媒体路由更新");

        assert_eq!(json["type"], "media_route_updated");
        assert_eq!(json["member_ids"], serde_json::json!(["m_a", "m_b"]));
        assert_eq!(json["route"], "sfu");
        assert_eq!(json["reason"], "p2p_failed");
    }

    #[test]
    fn 服务端_renegotiation_needed_包含发布者成员_id() {
        let json = serde_json::to_value(ServerSignal::RenegotiationNeeded {
            member_id: "publisher-1".to_string(),
        })
        .expect("序列化重新协商信令");

        assert_eq!(json["type"], "renegotiation_needed");
        assert_eq!(json["member_id"], "publisher-1");
    }

    #[test]
    fn 服务端_member_speaking_updated_包含成员状态() {
        let json = serde_json::to_value(ServerSignal::MemberSpeakingUpdated {
            member_id: "publisher-1".to_string(),
            speaking: true,
        })
        .expect("序列化发言状态信令");

        assert_eq!(json["type"], "member_speaking_updated");
        assert_eq!(json["member_id"], "publisher-1");
        assert_eq!(json["speaking"], true);
    }

    #[test]
    fn 服务端_member_latency_updated_包含成员延迟() {
        let json = serde_json::to_value(ServerSignal::MemberLatencyUpdated {
            member_id: "member-1".to_string(),
            server_ms: 28.4,
        })
        .expect("序列化成员延迟信令");

        assert_eq!(json["type"], "member_latency_updated");
        assert_eq!(json["member_id"], "member-1");
        assert_eq!(json["server_ms"], 28.4);
    }

    #[test]
    fn 媒体事件只通知同房间的其他成员重新协商() {
        let event = MediaEvent::InboundAudioTrack {
            room_id: "room-1".to_string(),
            member_id: "publisher-1".to_string(),
            subscriber_member_ids: vec!["listener-1".to_string()],
        };

        assert!(matches!(
            renegotiation_signal_for_event(&event, Some("room-1"), Some("listener-1")),
            Some(ServerSignal::RenegotiationNeeded { member_id }) if member_id == "publisher-1"
        ));
        assert!(
            renegotiation_signal_for_event(&event, Some("room-1"), Some("publisher-1")).is_none()
        );
        assert!(
            renegotiation_signal_for_event(&event, Some("room-2"), Some("listener-1")).is_none()
        );
        assert!(
            renegotiation_signal_for_event(&event, Some("room-1"), Some("listener-2")).is_none()
        );
        assert!(renegotiation_signal_for_event(&event, None, None).is_none());
    }

    #[test]
    fn 信令中心_broadcast_不会发送给被排除成员() {
        let hub = SignalHub::new();
        let mut member_a = hub.register("room-1", "a").expect("注册成员 A");
        let mut member_b = hub.register("room-1", "b").expect("注册成员 B");
        let mut member_c = hub.register("room-1", "c").expect("注册成员 C");

        hub.broadcast(
            "room-1",
            ServerSignal::RoomClosed {
                room_id: "room-1".to_string(),
            },
            Some("a"),
        )
        .expect("广播房间关闭");

        assert!(matches!(member_a.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(
            member_b.try_recv(),
            Ok(ServerSignal::RoomClosed { room_id }) if room_id == "room-1"
        ));
        assert!(matches!(
            member_c.try_recv(),
            Ok(ServerSignal::RoomClosed { room_id }) if room_id == "room-1"
        ));
    }

    #[test]
    fn 信令中心_send_to_member_只发送给目标成员() {
        let hub = SignalHub::new();
        let mut member_a = hub.register("room-1", "a").expect("注册成员 A");
        let mut member_b = hub.register("room-1", "b").expect("注册成员 B");

        hub.send_to_member(
            "room-1",
            "b",
            ServerSignal::P2pOffer {
                from_member_id: "a".to_string(),
                sdp: "v=0\r\n".to_string(),
            },
        )
        .expect("定向发送 P2P offer");

        assert!(matches!(member_a.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(
            member_b.try_recv(),
            Ok(ServerSignal::P2pOffer { from_member_id, sdp })
                if from_member_id == "a" && sdp == "v=0\r\n"
        ));
    }

    #[test]
    fn 信令中心_send_to_member_目标队列失败后移除成员() {
        let hub = SignalHub::new();
        let _member = hub.register("room-1", "target").expect("注册目标成员");

        for index in 0..SIGNAL_QUEUE_CAPACITY {
            hub.send_to_member(
                "room-1",
                "target",
                ServerSignal::IceCandidate {
                    candidate: test_candidate(&format!("candidate-{index}")),
                },
            )
            .expect("填满目标队列前发送成功");
        }

        let error = hub
            .send_to_member(
                "room-1",
                "target",
                ServerSignal::IceCandidate {
                    candidate: test_candidate("overflow"),
                },
            )
            .expect_err("队列满时定向发送失败");
        assert!(matches!(error, Error::InvalidMessage(_)));

        let mut replacement = hub
            .register("room-1", "target")
            .expect("队列失败后成员已从信令中心移除，可以重新注册");
        hub.send_to_member(
            "room-1",
            "target",
            ServerSignal::IceCandidate {
                candidate: test_candidate("after-remove"),
            },
        )
        .expect("重新注册后可以定向发送");

        assert!(matches!(
            replacement.try_recv(),
            Ok(ServerSignal::IceCandidate { candidate }) if candidate.candidate == "after-remove"
        ));
    }

    #[test]
    fn 信令中心_broadcast_目标队列满后移除成员() {
        let hub = SignalHub::new();
        let _member = hub.register("room-1", "target").expect("注册目标成员");

        for index in 0..SIGNAL_QUEUE_CAPACITY {
            hub.broadcast(
                "room-1",
                ServerSignal::IceCandidate {
                    candidate: test_candidate(&format!("candidate-{index}")),
                },
                None,
            )
            .expect("填满目标队列前发送成功");
        }

        hub.broadcast(
            "room-1",
            ServerSignal::IceCandidate {
                candidate: test_candidate("overflow"),
            },
            None,
        )
        .expect("广播时会移除队列已满的成员");

        let mut replacement = hub
            .register("room-1", "target")
            .expect("队列失败后成员已从信令中心移除，可以重新注册");

        hub.broadcast(
            "room-1",
            ServerSignal::IceCandidate {
                candidate: test_candidate("after-remove"),
            },
            None,
        )
        .expect("重新注册后可以收到广播");

        assert!(matches!(
            replacement.try_recv(),
            Ok(ServerSignal::IceCandidate { candidate }) if candidate.candidate == "after-remove"
        ));
    }

    #[test]
    fn 信令中心_register_拒绝重复活跃成员连接() {
        let hub = SignalHub::new();
        let _first = hub.register("room-1", "member-1").expect("首次注册成功");

        let error = hub
            .register("room-1", "member-1")
            .expect_err("同一成员已有活跃连接时拒绝重复注册");

        assert!(matches!(error, Error::InvalidMessage(_)));
    }
}
