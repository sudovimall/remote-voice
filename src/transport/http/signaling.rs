use crate::{Error, Result, domain::room::Room, state::AppState};
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
use tokio::sync::mpsc::{self, error::TrySendError};

const SIGNAL_QUEUE_CAPACITY: usize = 256;

type RoomSignalSenders = HashMap<String, HashMap<String, mpsc::Sender<ServerSignal>>>;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientSignal {
    JoinRoom {
        request_id: String,
        room_id: String,
        nickname: String,
        member_id: Option<String>,
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
    WebrtcOffer {
        request_id: Option<String>,
        target_member_id: String,
        sdp: String,
    },
    WebrtcAnswer {
        request_id: Option<String>,
        target_member_id: String,
        sdp: String,
    },
    IceCandidate {
        request_id: Option<String>,
        target_member_id: String,
        candidate: String,
    },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerSignal {
    JoinedRoom {
        request_id: String,
        room: Room,
        member_id: String,
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
    WebrtcOffer {
        from_member_id: String,
        sdp: String,
    },
    WebrtcAnswer {
        from_member_id: String,
        sdp: String,
    },
    IceCandidate {
        from_member_id: String,
        candidate: String,
    },
    Error {
        request_id: Option<String>,
        code: &'static str,
        message: String,
    },
}

#[derive(Debug)]
pub struct SignalHub {
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

    pub fn send_to(
        &self,
        room_id: &str,
        target_member_id: &str,
        signal: ServerSignal,
    ) -> Result<()> {
        let mut rooms = self.write_rooms()?;
        let Some(members) = rooms.get_mut(room_id) else {
            return Err(Error::MemberNotFound);
        };

        let Some(sender) = members.get(target_member_id) else {
            return Err(Error::MemberNotFound);
        };

        match sender.try_send(signal) {
            Ok(()) => Ok(()),
            Err(TrySendError::Closed(_)) => {
                members.remove(target_member_id);
                if members.is_empty() {
                    rooms.remove(room_id);
                }
                Err(Error::MemberNotFound)
            }
            Err(TrySendError::Full(_)) => {
                members.remove(target_member_id);
                if members.is_empty() {
                    rooms.remove(room_id);
                }
                Err(Error::Internal("目标信令队列已满".to_string()))
            }
        }
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
                    ClientSignal::JoinRoom { request_id, room_id, nickname, member_id } => {
                        if joined_room_id.is_some() {
                            let _ = send_json(&mut sender, &ServerSignal::Error {
                                request_id: Some(request_id),
                                code: "invalid_message",
                                message: "已经加入房间".to_string(),
                            }).await;
                            continue;
                        }

                        if let Some(member_id) = member_id {
                            let room = match state.rooms.get_room(&room_id) {
                                Ok(room) => room,
                                Err(error) => {
                                    let _ = send_error(&mut sender, Some(request_id), error).await;
                                    continue;
                                }
                            };

                            if !room.members.contains_key(&member_id) {
                                let _ = send_error(&mut sender, Some(request_id), Error::MemberNotFound).await;
                                continue;
                            }

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
                                room,
                                member_id,
                            }).await;
                        } else {
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
                    }
                    ClientSignal::LeaveRoom { request_id } => {
                        if joined_room_id.is_none() {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        }

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
                    ClientSignal::WebrtcOffer { request_id, target_member_id, sdp } => {
                        let Some((room_id, from_member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        let signal = ServerSignal::WebrtcOffer {
                            from_member_id: from_member_id.to_string(),
                            sdp,
                        };
                        if let Err(error) = state.signals.send_to(room_id, &target_member_id, signal) {
                            let _ = send_error(&mut sender, request_id, error).await;
                        }
                    }
                    ClientSignal::WebrtcAnswer { request_id, target_member_id, sdp } => {
                        let Some((room_id, from_member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        let signal = ServerSignal::WebrtcAnswer {
                            from_member_id: from_member_id.to_string(),
                            sdp,
                        };
                        if let Err(error) = state.signals.send_to(room_id, &target_member_id, signal) {
                            let _ = send_error(&mut sender, request_id, error).await;
                        }
                    }
                    ClientSignal::IceCandidate { request_id, target_member_id, candidate } => {
                        let Some((room_id, from_member_id)) = joined_pair(&joined_room_id, &joined_member_id) else {
                            let _ = send_not_joined(&mut sender, request_id).await;
                            continue;
                        };

                        let signal = ServerSignal::IceCandidate {
                            from_member_id: from_member_id.to_string(),
                            candidate,
                        };
                        if let Err(error) = state.signals.send_to(room_id, &target_member_id, signal) {
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
        }
    }

    if let (Some(room_id), Some(member_id)) = (joined_room_id, joined_member_id) {
        if let Ok(room) = state.rooms.leave_room(&room_id, &member_id) {
            let _ = state.signals.unregister(&room_id, &member_id);

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
        } else {
            let _ = state.signals.unregister(&room_id, &member_id);
        }
    }
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
    use super::{ClientSignal, SIGNAL_QUEUE_CAPACITY, ServerSignal, SignalHub};
    use crate::Error;
    use tokio::sync::mpsc::error::TryRecvError;

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
                nickname,
                member_id
            } if request_id == "req-1"
                && room_id == "ABC123"
                && nickname == "小明"
                && member_id.is_none()
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
    fn 信令中心_direct_send_只发送给目标成员() {
        let hub = SignalHub::new();
        let mut member_a = hub.register("room-1", "a").expect("注册成员 A");
        let mut member_b = hub.register("room-1", "b").expect("注册成员 B");
        let mut member_c = hub.register("room-1", "c").expect("注册成员 C");

        hub.send_to(
            "room-1",
            "b",
            ServerSignal::WebrtcOffer {
                from_member_id: "a".to_string(),
                sdp: "offer-sdp".to_string(),
            },
        )
        .expect("定向发送给成员 B");

        assert!(matches!(member_a.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(member_c.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(
            member_b.try_recv(),
            Ok(ServerSignal::WebrtcOffer { from_member_id, sdp })
                if from_member_id == "a" && sdp == "offer-sdp"
        ));
    }

    #[test]
    fn 信令中心_direct_send_目标队列满后移除成员() {
        let hub = SignalHub::new();
        let _member = hub.register("room-1", "target").expect("注册目标成员");

        for index in 0..SIGNAL_QUEUE_CAPACITY {
            hub.send_to(
                "room-1",
                "target",
                ServerSignal::IceCandidate {
                    from_member_id: "source".to_string(),
                    candidate: format!("candidate-{index}"),
                },
            )
            .expect("填满目标队列前发送成功");
        }

        let full_error = hub
            .send_to(
                "room-1",
                "target",
                ServerSignal::IceCandidate {
                    from_member_id: "source".to_string(),
                    candidate: "overflow".to_string(),
                },
            )
            .expect_err("目标队列满后发送失败");

        assert!(matches!(full_error, Error::Internal(_)));

        let removed_error = hub
            .send_to(
                "room-1",
                "target",
                ServerSignal::IceCandidate {
                    from_member_id: "source".to_string(),
                    candidate: "after-remove".to_string(),
                },
            )
            .expect_err("队列失败后成员已从信令中心移除");

        assert!(matches!(removed_error, Error::MemberNotFound));
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
