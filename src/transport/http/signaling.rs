use crate::{
    Error, Result,
    domain::room::Room,
    media::{IceCandidate, MediaEvent},
    state::AppState,
};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, future::pending, sync::RwLock};
use tokio::sync::mpsc;

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
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerSignal {
    JoinedRoom {
        request_id: String,
        room: Room,
        member_id: String,
        resume_token: String,
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
    Error {
        request_id: Option<String>,
        code: &'static str,
        message: String,
    },
}

#[derive(Debug)]
pub struct SignalHub {
    // 这里只保存房间事件通道，不承载媒体包，也不再做成员间 WebRTC 信令转发。
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
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: AppState, socket: WebSocket) {
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

                // `deny_unknown_fields` 让旧的成员间 P2P 信令字段（例如 target_member_id）
                // 直接被拒绝，避免客户端绕过后端媒体层。
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

                        let join = match state.rooms.create_room(nickname) {
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
                                let _ = state.rooms.leave_room(&room_id, &member_id);
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

                        let join = match state.rooms.join_room(&room_id, nickname) {
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
                                let _ = state.rooms.leave_room(&room_id, &member_id);
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

                        let join = match state.rooms.resume_room(&room_id, &member_id, &resume_token) {
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

                        joined_room_id = Some(room_id.clone());
                        joined_member_id = Some(member_id.clone());
                        outbound = Some(room_receiver);

                        let _ = send_json(&mut sender, &ServerSignal::JoinedRoom {
                            request_id,
                            room: join.room.clone(),
                            member_id: member_id.clone(),
                            resume_token: join.resume_token,
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

                        match state.rooms.set_self_muted(room_id, member_id, self_muted) {
                            Ok(room) => {
                                let _ = state.signals.broadcast(
                                    room_id,
                                    ServerSignal::MemberUpdated {
                                        room,
                                        member_id: member_id.to_string(),
                                    },
                                    None,
                                );
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

                        match state.rooms.set_member_can_speak(room_id, actor_member_id, &member_id, can_speak) {
                            Ok(room) => {
                                // 房间层是权限真源；媒体层只消费当前值决定是否转发上行 RTP。
                                let _ = state
                                    .media
                                    .set_member_can_speak(room_id, &member_id, can_speak)
                                    .await;
                                let _ = state.signals.broadcast(
                                    room_id,
                                    ServerSignal::MemberUpdated { room, member_id },
                                    None,
                                );
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
                        match state.media.handle_offer(room_id, member_id, sdp).await {
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
                            .media
                            .add_ice_candidate(room_id, member_id, candidate)
                            .await
                        {
                            let _ = send_error(&mut sender, request_id, error).await;
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
        let _ = state.signals.unregister(&room_id, &member_id);

        if explicit_leave {
            broadcast_explicit_leave(&state, &room_id, &member_id);
        } else if let Ok(room) = state.rooms.mark_member_disconnected(&room_id, &member_id) {
            let _ = state.signals.broadcast(
                &room_id,
                ServerSignal::MemberUpdated {
                    room,
                    member_id: member_id.clone(),
                },
                None,
            );
            schedule_disconnected_cleanup(state, room_id, member_id);
        }
    }
}

fn broadcast_explicit_leave(state: &AppState, room_id: &str, member_id: &str) {
    let Ok(room) = state.rooms.leave_room(room_id, member_id) else {
        return;
    };

    if room.owner_member_id == member_id {
        let _ = state.signals.broadcast(
            room_id,
            ServerSignal::RoomClosed {
                room_id: room_id.to_string(),
            },
            None,
        );
        let _ = state.signals.clear_room(room_id);
    } else {
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

fn schedule_disconnected_cleanup(state: AppState, room_id: String, member_id: String) {
    tokio::spawn(async move {
        tokio::time::sleep(state.disconnect_grace_period).await;

        let Ok(Some(room)) = state.rooms.expire_disconnected_member(&room_id, &member_id) else {
            return;
        };

        if room.owner_member_id == member_id {
            let _ = state.signals.broadcast(
                &room_id,
                ServerSignal::RoomClosed {
                    room_id: room_id.clone(),
                },
                None,
            );
            let _ = state.signals.clear_room(&room_id);
        } else {
            let _ = state.signals.broadcast(
                &room_id,
                ServerSignal::MemberLeft { room, member_id },
                None,
            );
        }
    });
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
    match event {
        MediaEvent::InboundAudioTrack { room_id, member_id }
            if joined_room_id == Some(room_id.as_str())
                && joined_member_id.is_some()
                && joined_member_id != Some(member_id.as_str()) =>
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
    fn 服务端_renegotiation_needed_包含发布者成员_id() {
        let json = serde_json::to_value(ServerSignal::RenegotiationNeeded {
            member_id: "publisher-1".to_string(),
        })
        .expect("序列化重新协商信令");

        assert_eq!(json["type"], "renegotiation_needed");
        assert_eq!(json["member_id"], "publisher-1");
    }

    #[test]
    fn 媒体事件只通知同房间的其他成员重新协商() {
        let event = MediaEvent::InboundAudioTrack {
            room_id: "room-1".to_string(),
            member_id: "publisher-1".to_string(),
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
