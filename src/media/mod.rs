use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, broadcast, mpsc};
use webrtc::{
    api::{
        API, APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_OPUS, MIME_TYPE_VP8, MediaEngine},
        setting_engine::SettingEngine,
    },
    ice::udp_network::{EphemeralUDP, UDPNetwork},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_candidate_type::RTCIceCandidateType,
    },
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        sdp::session_description::RTCSessionDescription,
    },
    rtcp::{
        packet::Packet as RtcpPacket,
        payload_feedbacks::{
            full_intra_request::FullIntraRequest, picture_loss_indication::PictureLossIndication,
        },
        transport_feedbacks::transport_layer_nack::TransportLayerNack,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
        rtp_sender::RTCRtpSender,
    },
    track::{
        track_local::{TrackLocal, track_local_static_rtp::TrackLocalStaticRTP},
        track_remote::TrackRemote,
    },
};

type SessionKey = (String, String);
type SessionMap = HashMap<SessionKey, MediaSession>;
const LOCAL_ICE_QUEUE_CAPACITY: usize = 64;
const MEDIA_EVENT_QUEUE_CAPACITY: usize = 256;
const DEFAULT_DOWNLINK_SLOT_COUNT: usize = 7;
const VIDEO_KEYFRAME_REQUEST_DELAYS_MS: [u64; 3] = [0, 500, 1500];

/// 管理服务端 SFU PeerConnection、音视频轨道转发和成员级媒体策略。
pub struct MediaController {
    api: API,
    downlink_slot_count: usize,
    // 每个成员只维护一条到后端的 PeerConnection；上行轨道也挂在同一个会话里。
    sessions: Arc<Mutex<SessionMap>>,
    // 房间权限可能先于媒体 offer 到达，先按成员记住，建会话时再带入 RTP 转发路径。
    member_can_speak: Arc<Mutex<HashMap<SessionKey, bool>>>,
    // 每个听众私有的“不接收哪些发布者”策略，信令层只回传给当前听众。
    member_not_listening: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
    screen_share_owners: Arc<Mutex<HashMap<String, String>>>,
    screen_share_viewers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    video_call_publishers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    #[cfg(test)]
    fail_next_screen_share_owner: Arc<std::sync::atomic::AtomicBool>,
    event_sender: broadcast::Sender<MediaEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvent {
    InboundAudioTrack {
        room_id: String,
        member_id: String,
        subscriber_member_ids: Vec<String>,
    },
    InboundScreenVideoTrack {
        room_id: String,
        member_id: String,
        subscriber_member_ids: Vec<String>,
    },
    InboundCameraVideoTrack {
        room_id: String,
        member_id: String,
        subscriber_member_ids: Vec<String>,
    },
}

struct MediaSession {
    peer_connection: Arc<RTCPeerConnection>,
    downlink_senders: Vec<Arc<RTCRtpSender>>,
    screen_video_downlink_sender: Option<Arc<RTCRtpSender>>,
    camera_video_downlink_senders: Vec<Arc<RTCRtpSender>>,
    can_speak: bool,
    inbound_tracks: HashMap<usize, InboundTrack>,
    outbound_tracks: HashMap<String, OutboundTrack>,
    video_feedback_tasks: HashMap<VideoFeedbackSlot, tokio::task::JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MediaTrackKind {
    Audio,
    ScreenShareVideo,
    CameraVideo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct VideoFeedbackSlot {
    kind: MediaTrackKind,
    slot_index: usize,
}

#[derive(Debug, Clone)]
struct InboundTrack {
    id: String,
    stream_id: String,
    ssrc: u32,
    mime_type: String,
    kind: MediaTrackKind,
    packet_count: u64,
    fanout_track: Arc<TrackLocalStaticRTP>,
}

#[derive(Debug, Clone)]
struct OutboundTrack {
    publisher_member_id: String,
    track_id: String,
    kind: MediaTrackKind,
    downlink_slot_index: usize,
    fanout_track: Arc<TrackLocalStaticRTP>,
}

/// 媒体会话只读快照，供测试和诊断确认轨道数量、转发关系和发言权限。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSessionSnapshot {
    pub can_speak: bool,
    pub not_listening_member_ids: Vec<String>,
    pub inbound_track_count: usize,
    pub audio_track_count: usize,
    pub video_track_count: usize,
    pub inbound_packet_count: u64,
    pub outbound_track_count: usize,
    pub outbound_video_track_count: usize,
    pub video_feedback_task_count: usize,
    pub tracks: Vec<InboundTrackSnapshot>,
    pub outbound_tracks: Vec<OutboundTrackSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundTrackSnapshot {
    pub id: String,
    pub stream_id: String,
    pub ssrc: u32,
    pub mime_type: String,
    pub kind: String,
    pub packet_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundTrackSnapshot {
    pub publisher_member_id: String,
    pub track_id: String,
    pub kind: String,
}

#[derive(Debug)]
pub struct MediaAnswer {
    pub sdp: String,
    // handle_offer 返回后 ICE 仍会继续收集；信令层从这个队列流式发送给同一个客户端。
    pub local_ice_candidates: mpsc::Receiver<IceCandidate>,
}

/// WebSocket 信令层传输的 ICE candidate 结构，保持和浏览器 RTCIceCandidateInit 对齐。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IceCandidate {
    // candidate 行本体，例如 "candidate:..."。其他字段用于定位具体的 m-line。
    pub candidate: String,
    #[serde(default)]
    pub sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex", default)]
    pub sdp_mline_index: Option<u16>,
    #[serde(default)]
    pub username_fragment: Option<String>,
}

impl From<RTCIceCandidateInit> for IceCandidate {
    fn from(candidate: RTCIceCandidateInit) -> Self {
        Self {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdp_mid,
            sdp_mline_index: candidate.sdp_mline_index,
            username_fragment: candidate.username_fragment,
        }
    }
}

impl From<IceCandidate> for RTCIceCandidateInit {
    fn from(candidate: IceCandidate) -> Self {
        Self {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdp_mid,
            sdp_mline_index: candidate.sdp_mline_index,
            username_fragment: candidate.username_fragment,
        }
    }
}

impl fmt::Debug for MediaController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaController").finish_non_exhaustive()
    }
}

impl MediaController {
    pub fn new() -> Result<Self> {
        Self::with_api_builder(APIBuilder::new(), DEFAULT_DOWNLINK_SLOT_COUNT)
    }

    pub fn new_with_udp_port_range(
        udp_port_min: u16,
        udp_port_max: u16,
        public_ip: Option<String>,
    ) -> Result<Self> {
        let udp_ports = EphemeralUDP::new(udp_port_min, udp_port_max).map_err(|err| {
            Error::Internal(format!(
                "配置 WebRTC UDP 端口范围 {udp_port_min}-{udp_port_max} 失败: {err}"
            ))
        })?;
        let mut setting_engine = SettingEngine::default();
        setting_engine.set_udp_network(UDPNetwork::Ephemeral(udp_ports));
        if let Some(public_ip) = public_ip {
            setting_engine.set_nat_1to1_ips(vec![public_ip], RTCIceCandidateType::Host);
        }
        Self::with_api_builder(
            APIBuilder::new().with_setting_engine(setting_engine),
            DEFAULT_DOWNLINK_SLOT_COUNT,
        )
    }

    pub fn new_with_downlink_slot_count(downlink_slot_count: usize) -> Result<Self> {
        Self::with_api_builder(APIBuilder::new(), downlink_slot_count)
    }

    fn with_api_builder(api_builder: APIBuilder, downlink_slot_count: usize) -> Result<Self> {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|err| Error::Internal(format!("注册默认 codecs 失败: {err}")))?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(|err| Error::Internal(format!("注册默认 interceptors 失败: {err}")))?;
        let api = api_builder
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        Ok(Self {
            api,
            downlink_slot_count: downlink_slot_count.max(1),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            member_can_speak: Arc::new(Mutex::new(HashMap::new())),
            member_not_listening: Arc::new(Mutex::new(HashMap::new())),
            screen_share_owners: Arc::new(Mutex::new(HashMap::new())),
            screen_share_viewers: Arc::new(Mutex::new(HashMap::new())),
            video_call_publishers: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            fail_next_screen_share_owner: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            event_sender: broadcast::channel(MEDIA_EVENT_QUEUE_CAPACITY).0,
        })
    }

    #[cfg(test)]
    fn new_with_vnet_for_test(vnet: Arc<webrtc::util::vnet::net::Net>) -> Result<Self> {
        let mut setting_engine = SettingEngine::default();
        setting_engine.set_vnet(Some(vnet));
        Self::with_api_builder(
            APIBuilder::new().with_setting_engine(setting_engine),
            DEFAULT_DOWNLINK_SLOT_COUNT,
        )
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<MediaEvent> {
        self.event_sender.subscribe()
    }

    /// 创建或替换某个成员和后端之间的 PeerConnection。
    ///
    /// 这里不是成员之间的 P2P 协商：浏览器的 offer 只交给后端，
    /// 后端生成 answer，并把本地 ICE candidate 通过信令层发回同一个浏览器。
    pub async fn handle_offer(
        &self,
        room_id: &str,
        member_id: &str,
        sdp: String,
    ) -> Result<MediaAnswer> {
        let wants_video = sdp_has_video(&sdp);
        let offer = RTCSessionDescription::offer(sdp)
            .map_err(|err| Error::InvalidMessage(format!("无效 SDP offer: {err}")))?;
        let key = (room_id.to_string(), member_id.to_string());
        let (local_ice_sender, local_ice_candidates) =
            mpsc::channel::<IceCandidate>(LOCAL_ICE_QUEUE_CAPACITY);
        let existing_peer_connection = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(&key)
                .map(|session| Arc::clone(&session.peer_connection))
        };

        if let Some(peer_connection) = existing_peer_connection {
            if wants_video {
                ensure_video_downlink_slots(
                    Arc::clone(&self.sessions),
                    &key,
                    &peer_connection,
                    room_id,
                    member_id,
                    self.downlink_slot_count,
                )
                .await?;
            }
            // 重新协商要沿用已建立的 ICE/DTLS 会话；替换 PeerConnection 会让客户端仍连着旧会话。
            forward_local_ice_candidates(&peer_connection, local_ice_sender);
            let answer = create_answer(&peer_connection, offer).await?;
            if wants_video {
                attach_existing_screen_video_to_subscriber(
                    Arc::clone(&self.sessions),
                    Arc::clone(&self.screen_share_owners),
                    Arc::clone(&self.screen_share_viewers),
                    room_id,
                    member_id,
                )
                .await?;
                attach_existing_camera_videos_to_subscriber(
                    Arc::clone(&self.sessions),
                    Arc::clone(&self.video_call_publishers),
                    room_id,
                    member_id,
                )
                .await?;
            }
            return Ok(MediaAnswer {
                sdp: answer.sdp,
                local_ice_candidates,
            });
        }

        let peer_connection = self
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|err| Error::Internal(format!("创建 PeerConnection 失败: {err}")))?;
        let peer_connection = Arc::new(peer_connection);
        forward_local_ice_candidates(&peer_connection, local_ice_sender);
        let downlink_senders = add_downlink_slots(
            &peer_connection,
            room_id,
            member_id,
            self.downlink_slot_count,
        )
        .await?;
        let screen_video_downlink_sender = if wants_video {
            Some(add_screen_video_downlink_slot(&peer_connection, room_id, member_id).await?)
        } else {
            None
        };
        let camera_video_downlink_senders = if wants_video {
            add_camera_video_downlink_slots(
                &peer_connection,
                room_id,
                member_id,
                self.downlink_slot_count,
            )
            .await?
        } else {
            Vec::new()
        };

        let sessions = Arc::clone(&self.sessions);
        let member_not_listening = Arc::clone(&self.member_not_listening);
        let screen_share_owners = Arc::clone(&self.screen_share_owners);
        let screen_share_viewers = Arc::clone(&self.screen_share_viewers);
        let video_call_publishers = Arc::clone(&self.video_call_publishers);
        let session_key = (room_id.to_string(), member_id.to_string());
        let event_sender = self.event_sender.clone();
        let track_peer_connection = Arc::clone(&peer_connection);
        // 收到上行 TrackRemote 后先登记元数据；RTP 转发会在下一阶段消费这些 track。
        peer_connection.on_track(Box::new(move |track, _, _| {
            let sessions = Arc::clone(&sessions);
            let session_key = session_key.clone();
            let event_sender = event_sender.clone();
            let track_peer_connection = Arc::clone(&track_peer_connection);
            let member_not_listening = Arc::clone(&member_not_listening);
            let screen_share_owners = Arc::clone(&screen_share_owners);
            let screen_share_viewers = Arc::clone(&screen_share_viewers);
            let video_call_publishers = Arc::clone(&video_call_publishers);

            Box::pin(async move {
                let kind = match track.kind() {
                    RTPCodecType::Audio => MediaTrackKind::Audio,
                    RTPCodecType::Video => {
                        let Some(kind) = classify_inbound_video_track(
                            Arc::clone(&sessions),
                            Arc::clone(&screen_share_owners),
                            Arc::clone(&video_call_publishers),
                            &session_key,
                            &track,
                        )
                        .await
                        else {
                            return;
                        };
                        kind
                    }
                    _ => return,
                };

                let inbound_track =
                    InboundTrack::from_remote_track(&track, &session_key.0, &session_key.1, kind);
                let fanout_track = Arc::clone(&inbound_track.fanout_track);
                let outbound_track_id =
                    fanout_track_id(&session_key.1, &inbound_track.id, inbound_track.kind);
                let sessions_for_reader = Arc::clone(&sessions);
                let track_id = track.tid();
                let should_attach = {
                    let mut session_map = sessions.lock().await;
                    let Some(session) = session_map.get_mut(&session_key) else {
                        return;
                    };

                    if !Arc::ptr_eq(&session.peer_connection, &track_peer_connection) {
                        return;
                    }

                    session.inbound_tracks.insert(track_id, inbound_track);
                    true
                };

                if should_attach {
                    tokio::spawn(read_inbound_rtp(
                        Arc::clone(&track),
                        Arc::clone(&fanout_track),
                        sessions_for_reader,
                        session_key.clone(),
                        Arc::clone(&track_peer_connection),
                        track_id,
                        kind,
                        Arc::clone(&screen_share_owners),
                        Arc::clone(&video_call_publishers),
                    ));

                    match kind {
                        MediaTrackKind::Audio => {
                            if let Ok(subscriber_member_ids) = attach_audio_to_subscribers(
                                Arc::clone(&sessions),
                                Arc::clone(&member_not_listening),
                                &session_key.0,
                                &session_key.1,
                                outbound_track_id,
                                fanout_track,
                            )
                            .await
                            {
                                if !subscriber_member_ids.is_empty() {
                                    let _ = event_sender.send(MediaEvent::InboundAudioTrack {
                                        room_id: session_key.0,
                                        member_id: session_key.1,
                                        subscriber_member_ids,
                                    });
                                }
                            }
                        }
                        MediaTrackKind::ScreenShareVideo => {
                            if let Ok(subscriber_member_ids) = attach_screen_video_to_subscribers(
                                Arc::clone(&sessions),
                                Arc::clone(&screen_share_viewers),
                                &session_key.0,
                                &session_key.1,
                                outbound_track_id,
                                fanout_track,
                            )
                            .await
                            {
                                if !subscriber_member_ids.is_empty() {
                                    let _ =
                                        event_sender.send(MediaEvent::InboundScreenVideoTrack {
                                            room_id: session_key.0,
                                            member_id: session_key.1,
                                            subscriber_member_ids,
                                        });
                                }
                            }
                        }
                        MediaTrackKind::CameraVideo => {
                            if let Ok(subscriber_member_ids) = attach_camera_video_to_subscribers(
                                Arc::clone(&sessions),
                                &session_key.0,
                                &session_key.1,
                                outbound_track_id,
                                fanout_track,
                            )
                            .await
                            {
                                if !subscriber_member_ids.is_empty() {
                                    let _ =
                                        event_sender.send(MediaEvent::InboundCameraVideoTrack {
                                            room_id: session_key.0,
                                            member_id: session_key.1,
                                            subscriber_member_ids,
                                        });
                                }
                            }
                        }
                    }
                }
            })
        }));

        let previous = {
            let can_speak = self
                .member_can_speak
                .lock()
                .await
                .get(&key)
                .copied()
                .unwrap_or(true);
            let mut sessions = self.sessions.lock().await;
            sessions.insert(
                key.clone(),
                MediaSession {
                    peer_connection: Arc::clone(&peer_connection),
                    downlink_senders,
                    screen_video_downlink_sender,
                    camera_video_downlink_senders,
                    can_speak,
                    inbound_tracks: HashMap::new(),
                    outbound_tracks: HashMap::new(),
                    video_feedback_tasks: HashMap::new(),
                },
            )
        };

        if let Some(previous) = &previous {
            for outbound_track in previous.outbound_tracks.values() {
                let downlink_sender = {
                    let sessions = self.sessions.lock().await;
                    sessions
                        .get(&key)
                        .and_then(|session| sender_for_outbound_track(session, outbound_track))
                };
                let Some(downlink_sender) = downlink_sender else {
                    continue;
                };

                downlink_sender
                    .replace_track(Some(Arc::clone(&outbound_track.fanout_track)
                        as Arc<dyn TrackLocal + Send + Sync>))
                    .await
                    .map_err(|err| Error::Internal(format!("恢复下行 track 失败: {err}")))?;
                if outbound_track.kind != MediaTrackKind::Audio {
                    replace_subscriber_video_rtcp_feedback_task(
                        Arc::clone(&self.sessions),
                        key.clone(),
                        outbound_track.publisher_member_id.clone(),
                        outbound_track.kind,
                        outbound_track.downlink_slot_index,
                        Arc::clone(&downlink_sender),
                    )
                    .await;
                }
            }

            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(&key) {
                session.outbound_tracks = previous.outbound_tracks.clone();
            }
        }

        let answer = match create_answer(&peer_connection, offer).await {
            Ok(answer) => answer,
            Err(error) => {
                let failed_session = {
                    let mut sessions = self.sessions.lock().await;
                    match sessions.get(&key) {
                        Some(session)
                            if Arc::ptr_eq(&session.peer_connection, &peer_connection) =>
                        {
                            sessions.remove(&key)
                        }
                        _ => None,
                    }
                };

                if let Some(failed_session) = failed_session {
                    let _ = failed_session.peer_connection.close().await;
                }

                if let Some(previous) = previous {
                    let mut sessions = self.sessions.lock().await;
                    sessions.insert(key, previous);
                }

                return Err(error);
            }
        };
        attach_existing_audio_to_subscriber(
            Arc::clone(&self.sessions),
            Arc::clone(&self.member_not_listening),
            room_id,
            member_id,
        )
        .await?;
        if wants_video {
            attach_existing_screen_video_to_subscriber(
                Arc::clone(&self.sessions),
                Arc::clone(&self.screen_share_owners),
                Arc::clone(&self.screen_share_viewers),
                room_id,
                member_id,
            )
            .await?;
            attach_existing_camera_videos_to_subscriber(
                Arc::clone(&self.sessions),
                Arc::clone(&self.video_call_publishers),
                room_id,
                member_id,
            )
            .await?;
        }

        if let Some(previous) = previous {
            // 新连接已经保存成功，旧连接关闭失败不应让本次 answer 回滚。
            let _ = previous.peer_connection.close().await;
        }

        Ok(MediaAnswer {
            sdp: answer.sdp,
            local_ice_candidates,
        })
    }

    /// 添加浏览器通过 WebSocket 信令发来的远端 ICE candidate。
    ///
    /// RTP/SRTP 媒体包不会进入 WebSocket；它们仍由 PeerConnection 管理。
    pub async fn add_ice_candidate(
        &self,
        room_id: &str,
        member_id: &str,
        candidate: IceCandidate,
    ) -> Result<()> {
        let key = (room_id.to_string(), member_id.to_string());
        let peer_connection = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(&key)
                .map(|session| Arc::clone(&session.peer_connection))
        }
        .ok_or_else(|| Error::InvalidMessage("媒体会话不存在，请先发送 offer".to_string()))?;

        peer_connection
            .add_ice_candidate(candidate.into())
            .await
            .map_err(|err| Error::Internal(format!("添加 ICE candidate 失败: {err}")))
    }

    /// 关闭成员的媒体会话。房间状态清理由领域层负责。
    pub async fn close_member(&self, room_id: &str, member_id: &str) -> Result<()> {
        let key = (room_id.to_string(), member_id.to_string());
        self.member_can_speak.lock().await.remove(&key);
        {
            let mut policies = self.member_not_listening.lock().await;
            policies.remove(&key);
            for blocked in policies.values_mut() {
                blocked.remove(member_id);
            }
        }
        {
            let mut viewers = self.screen_share_viewers.lock().await;
            if let Some(room_viewers) = viewers.get_mut(room_id) {
                room_viewers.remove(member_id);
                if room_viewers.is_empty() {
                    viewers.remove(room_id);
                }
            }
        }
        self.clear_screen_share_owner_if_matches(room_id, member_id)
            .await?;
        self.clear_video_call_publisher_if_matches(room_id, member_id)
            .await?;
        detach_publisher_audio_from_subscribers(Arc::clone(&self.sessions), room_id, member_id)
            .await?;
        let session = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(&key)
        };

        if let Some(session) = session {
            session
                .peer_connection
                .close()
                .await
                .map_err(|err| Error::Internal(format!("关闭 PeerConnection 失败: {err}")))?;
        }

        Ok(())
    }

    pub async fn set_screen_share_owner(
        &self,
        room_id: &str,
        member_id: Option<&str>,
    ) -> Result<()> {
        match member_id {
            Some(member_id) => {
                #[cfg(test)]
                if self
                    .fail_next_screen_share_owner
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    return Err(Error::Internal(
                        "测试注入屏幕共享 owner 同步失败".to_string(),
                    ));
                }
                let previous_owner = self
                    .screen_share_owners
                    .lock()
                    .await
                    .insert(room_id.to_string(), member_id.to_string());
                let result = attach_existing_screen_video_to_subscribers_for_publisher(
                    Arc::clone(&self.sessions),
                    Arc::clone(&self.screen_share_viewers),
                    room_id,
                    member_id,
                )
                .await;
                if result.is_err() {
                    let mut owners = self.screen_share_owners.lock().await;
                    match previous_owner {
                        Some(previous_owner) => {
                            owners.insert(room_id.to_string(), previous_owner);
                        }
                        None => {
                            owners.remove(room_id);
                        }
                    }
                }
                result
            }
            None => {
                let previous = self.screen_share_owners.lock().await.remove(room_id);
                if let Some(previous_member_id) = previous {
                    detach_publisher_video_from_subscribers(
                        Arc::clone(&self.sessions),
                        room_id,
                        &previous_member_id,
                        MediaTrackKind::ScreenShareVideo,
                    )
                    .await?;
                    remove_inbound_video_tracks(
                        Arc::clone(&self.sessions),
                        room_id,
                        &previous_member_id,
                        MediaTrackKind::ScreenShareVideo,
                    )
                    .await;
                }
                Ok(())
            }
        }
    }

    async fn clear_screen_share_owner_if_matches(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<()> {
        let should_clear = self
            .screen_share_owners
            .lock()
            .await
            .get(room_id)
            .is_some_and(|owner| owner == member_id);
        if should_clear {
            self.set_screen_share_owner(room_id, None).await?;
        }
        Ok(())
    }

    /// 同步房间层的摄像头发布状态，并在停止发布时释放对应下行槽位。
    pub async fn set_video_call_publisher(
        &self,
        room_id: &str,
        member_id: &str,
        publishing: bool,
    ) -> Result<usize> {
        if publishing {
            self.video_call_publishers
                .lock()
                .await
                .entry(room_id.to_string())
                .or_default()
                .insert(member_id.to_string());
            attach_existing_camera_video_to_subscribers_for_publisher(
                Arc::clone(&self.sessions),
                room_id,
                member_id,
            )
            .await?;
        } else {
            self.clear_video_call_publisher_if_matches(room_id, member_id)
                .await?;
        }

        Ok(self.video_call_publisher_count(room_id).await)
    }

    async fn clear_video_call_publisher_if_matches(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<()> {
        let removed = {
            let mut publishers = self.video_call_publishers.lock().await;
            let Some(room_publishers) = publishers.get_mut(room_id) else {
                return Ok(());
            };
            let removed = room_publishers.remove(member_id);
            if room_publishers.is_empty() {
                publishers.remove(room_id);
            }
            removed
        };
        if removed {
            detach_publisher_video_from_subscribers(
                Arc::clone(&self.sessions),
                room_id,
                member_id,
                MediaTrackKind::CameraVideo,
            )
            .await?;
            remove_inbound_video_tracks(
                Arc::clone(&self.sessions),
                room_id,
                member_id,
                MediaTrackKind::CameraVideo,
            )
            .await;
        }
        Ok(())
    }

    /// 返回当前房间摄像头发布人数，供信令层广播给前端做码率降级。
    pub async fn video_call_publisher_count(&self, room_id: &str) -> usize {
        self.video_call_publishers
            .lock()
            .await
            .get(room_id)
            .map(HashSet::len)
            .unwrap_or(0)
    }

    pub async fn set_screen_viewing(
        &self,
        room_id: &str,
        member_id: &str,
        viewing: bool,
    ) -> Result<usize> {
        {
            let mut viewers = self.screen_share_viewers.lock().await;
            let room_viewers = viewers.entry(room_id.to_string()).or_default();
            if viewing {
                room_viewers.insert(member_id.to_string());
            } else {
                room_viewers.remove(member_id);
                if room_viewers.is_empty() {
                    viewers.remove(room_id);
                }
            }
        }

        if viewing {
            attach_existing_screen_video_to_subscriber(
                Arc::clone(&self.sessions),
                Arc::clone(&self.screen_share_owners),
                Arc::clone(&self.screen_share_viewers),
                room_id,
                member_id,
            )
            .await?;
        } else {
            detach_current_video_from_subscriber(Arc::clone(&self.sessions), room_id, member_id)
                .await?;
        }

        Ok(self.screen_viewer_count(room_id).await)
    }

    pub async fn screen_viewer_count(&self, room_id: &str) -> usize {
        let owner = self.screen_share_owners.lock().await.get(room_id).cloned();
        let viewers = self.screen_share_viewers.lock().await;
        viewers
            .get(room_id)
            .map(|room_viewers| {
                room_viewers
                    .iter()
                    .filter(|member_id| Some(member_id.as_str()) != owner.as_deref())
                    .count()
            })
            .unwrap_or(0)
    }

    /// 注入下一次屏幕共享 owner 同步失败，供服务层回滚测试稳定覆盖。
    #[cfg(test)]
    pub(crate) fn fail_next_screen_share_owner_for_test(&self) {
        self.fail_next_screen_share_owner
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// 同步当前听众对某个发布者的下行接收偏好。
    pub async fn set_member_listening(
        &self,
        room_id: &str,
        listener_member_id: &str,
        publisher_member_id: &str,
        listening: bool,
    ) -> Result<()> {
        let listener_key = (room_id.to_string(), listener_member_id.to_string());
        {
            let mut policies = self.member_not_listening.lock().await;
            let blocked = policies.entry(listener_key.clone()).or_default();
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
            .await
        } else {
            detach_publisher_audio_from_subscriber(
                Arc::clone(&self.sessions),
                room_id,
                listener_member_id,
                publisher_member_id,
            )
            .await
        }
    }

    /// 从房间快照替换式恢复成员音频策略，断线重连后重建媒体层缓存。
    pub async fn sync_member_audio_policy(
        &self,
        room_id: &str,
        member_id: &str,
        can_speak: bool,
        not_listening_member_ids: &[String],
    ) -> Result<()> {
        self.set_member_can_speak(room_id, member_id, can_speak)
            .await?;

        let listener_key = (room_id.to_string(), member_id.to_string());
        let next_blocked = not_listening_member_ids
            .iter()
            .filter(|publisher_member_id| {
                !publisher_member_id.is_empty() && publisher_member_id.as_str() != member_id
            })
            .cloned()
            .collect::<HashSet<_>>();
        let previous_blocked = {
            let mut policies = self.member_not_listening.lock().await;
            if next_blocked.is_empty() {
                policies.remove(&listener_key).unwrap_or_default()
            } else {
                policies
                    .insert(listener_key.clone(), next_blocked.clone())
                    .unwrap_or_default()
            }
        };

        for publisher_member_id in previous_blocked.difference(&next_blocked) {
            attach_existing_publisher_audio_to_subscriber(
                Arc::clone(&self.sessions),
                Arc::clone(&self.member_not_listening),
                room_id,
                member_id,
                publisher_member_id,
            )
            .await?;
        }
        for publisher_member_id in &next_blocked {
            detach_publisher_audio_from_subscriber(
                Arc::clone(&self.sessions),
                room_id,
                member_id,
                publisher_member_id,
            )
            .await?;
        }

        Ok(())
    }

    /// 同步房间层的发言权限；没有媒体会话时缓存到该成员后续的 offer。
    pub async fn set_member_can_speak(
        &self,
        room_id: &str,
        member_id: &str,
        can_speak: bool,
    ) -> Result<()> {
        let key = (room_id.to_string(), member_id.to_string());
        self.member_can_speak
            .lock()
            .await
            .insert(key.clone(), can_speak);
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(&key) {
            session.can_speak = can_speak;
        }

        Ok(())
    }

    /// 返回当前媒体会话的只读快照，主要供状态检查和测试使用。
    pub async fn session_snapshot(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Option<MediaSessionSnapshot> {
        let key = (room_id.to_string(), member_id.to_string());
        let mut not_listening_member_ids = self
            .member_not_listening
            .lock()
            .await
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        not_listening_member_ids.sort();
        let sessions = self.sessions.lock().await;
        sessions
            .get(&key)
            .map(|session| session.snapshot(not_listening_member_ids))
    }

    #[cfg(test)]
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
        .map(|_| ())
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
        let publisher = sessions.get_mut(&key).ok_or(Error::MemberNotFound)?;
        publisher.inbound_tracks.insert(
            usize::MAX,
            InboundTrack {
                id: "test-audio".to_string(),
                stream_id: format!("room-{room_id}"),
                ssrc: 0,
                mime_type: "audio/opus".to_string(),
                kind: MediaTrackKind::Audio,
                packet_count: 0,
                fanout_track,
            },
        );
        Ok(())
    }

    #[cfg(test)]
    async fn store_test_video_inbound_track(
        &self,
        room_id: &str,
        publisher_member_id: &str,
        kind: MediaTrackKind,
    ) -> Result<String> {
        let source_id = match kind {
            MediaTrackKind::ScreenShareVideo => "test-screen",
            MediaTrackKind::CameraVideo => "test-camera",
            MediaTrackKind::Audio => "test-audio",
        };
        let track_id = fanout_track_id(publisher_member_id, source_id, kind);
        let fanout_track = Arc::new(TrackLocalStaticRTP::new(
            webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability {
                mime_type: "video/vp8".to_string(),
                clock_rate: 90000,
                ..Default::default()
            },
            track_id.clone(),
            format!("room-{room_id}"),
        ));
        let key = (room_id.to_string(), publisher_member_id.to_string());
        let mut sessions = self.sessions.lock().await;
        let publisher = sessions.get_mut(&key).ok_or(Error::MemberNotFound)?;
        publisher.inbound_tracks.insert(
            kind as usize,
            InboundTrack {
                id: source_id.to_string(),
                stream_id: format!("room-{room_id}"),
                ssrc: 0,
                mime_type: "video/vp8".to_string(),
                kind,
                packet_count: 0,
                fanout_track,
            },
        );
        Ok(track_id)
    }
}

async fn create_answer(
    peer_connection: &RTCPeerConnection,
    offer: RTCSessionDescription,
) -> Result<RTCSessionDescription> {
    peer_connection
        .set_remote_description(offer)
        .await
        .map_err(|err| Error::Internal(format!("设置 remote description 失败: {err}")))?;
    let answer = peer_connection
        .create_answer(None)
        .await
        .map_err(|err| Error::Internal(format!("创建 SDP answer 失败: {err}")))?;
    peer_connection
        .set_local_description(answer.clone())
        .await
        .map_err(|err| Error::Internal(format!("设置 local description 失败: {err}")))?;
    Ok(answer)
}

fn forward_local_ice_candidates(
    peer_connection: &RTCPeerConnection,
    local_ice_sender: mpsc::Sender<IceCandidate>,
) {
    // webrtc-rs 通过回调异步产出本地 candidate；信令层负责把它们发回当前浏览器。
    peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
        let local_ice_sender = local_ice_sender.clone();
        Box::pin(async move {
            let Some(candidate) = candidate else {
                return;
            };
            let Ok(candidate) = candidate.to_json() else {
                return;
            };

            let _ = local_ice_sender.try_send(candidate.into());
        })
    }));
}

fn sdp_has_video(sdp: &str) -> bool {
    sdp.lines().any(|line| line.starts_with("m=video "))
}

async fn add_downlink_slots(
    peer_connection: &RTCPeerConnection,
    room_id: &str,
    member_id: &str,
    slot_count: usize,
) -> Result<Vec<Arc<RTCRtpSender>>> {
    // 客户端发 offer 时预留多个音频 m-line；服务端把每个发布者放到独立 sender 槽位。
    let mut downlink_senders = Vec::with_capacity(slot_count);
    for slot_index in 0..slot_count {
        let downlink_sender = peer_connection
            .add_track(new_downlink_slot_track(room_id, member_id, slot_index)
                as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|err| Error::Internal(format!("创建下行音频槽位失败: {err}")))?;
        downlink_senders.push(downlink_sender);
    }

    Ok(downlink_senders)
}

async fn add_screen_video_downlink_slot(
    peer_connection: &RTCPeerConnection,
    room_id: &str,
    member_id: &str,
) -> Result<Arc<RTCRtpSender>> {
    peer_connection
        .add_track(new_screen_video_downlink_slot_track(room_id, member_id)
            as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .map_err(|err| Error::Internal(format!("创建下行屏幕共享槽位失败: {err}")))
}

async fn add_camera_video_downlink_slots(
    peer_connection: &RTCPeerConnection,
    room_id: &str,
    member_id: &str,
    slot_count: usize,
) -> Result<Vec<Arc<RTCRtpSender>>> {
    let mut senders = Vec::with_capacity(slot_count);
    for slot_index in 0..slot_count {
        let sender = peer_connection
            .add_track(
                new_camera_video_downlink_slot_track(room_id, member_id, slot_index)
                    as Arc<dyn TrackLocal + Send + Sync>,
            )
            .await
            .map_err(|err| Error::Internal(format!("创建下行摄像头槽位失败: {err}")))?;
        senders.push(sender);
    }
    Ok(senders)
}

async fn ensure_video_downlink_slots(
    sessions: Arc<Mutex<SessionMap>>,
    key: &SessionKey,
    peer_connection: &RTCPeerConnection,
    room_id: &str,
    member_id: &str,
    camera_slot_count: usize,
) -> Result<()> {
    let (needs_screen_sender, needed_camera_slots) = {
        let sessions = sessions.lock().await;
        let Some(session) = sessions.get(key) else {
            return Ok(());
        };
        (
            session.screen_video_downlink_sender.is_none(),
            camera_slot_count.saturating_sub(session.camera_video_downlink_senders.len()),
        )
    };
    let screen_sender = if needs_screen_sender {
        Some(add_screen_video_downlink_slot(peer_connection, room_id, member_id).await?)
    } else {
        None
    };
    let camera_senders = if needed_camera_slots > 0 {
        add_camera_video_downlink_slots(peer_connection, room_id, member_id, needed_camera_slots)
            .await?
    } else {
        Vec::new()
    };
    if screen_sender.is_none() && camera_senders.is_empty() {
        return Ok(());
    }

    let mut sessions = sessions.lock().await;
    if let Some(session) = sessions.get_mut(key) {
        if let Some(sender) = screen_sender {
            session.screen_video_downlink_sender = Some(sender);
        }
        session.camera_video_downlink_senders.extend(camera_senders);
    }
    Ok(())
}

fn new_downlink_slot_track(
    room_id: &str,
    member_id: &str,
    slot_index: usize,
) -> Arc<TrackLocalStaticRTP> {
    Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.to_string(),
            clock_rate: 48000,
            channels: 2,
            ..Default::default()
        },
        format!("{member_id}:downlink-{slot_index}"),
        format!("room-{room_id}"),
    ))
}

fn new_screen_video_downlink_slot_track(
    room_id: &str,
    member_id: &str,
) -> Arc<TrackLocalStaticRTP> {
    Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_VP8.to_string(),
            clock_rate: 90000,
            ..Default::default()
        },
        format!("{member_id}:screen-downlink"),
        format!("room-{room_id}"),
    ))
}

fn new_camera_video_downlink_slot_track(
    room_id: &str,
    member_id: &str,
    slot_index: usize,
) -> Arc<TrackLocalStaticRTP> {
    Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_VP8.to_string(),
            clock_rate: 90000,
            ..Default::default()
        },
        format!("{member_id}:camera-downlink-{slot_index}"),
        format!("room-{room_id}"),
    ))
}

impl MediaSession {
    fn snapshot(&self, not_listening_member_ids: Vec<String>) -> MediaSessionSnapshot {
        let tracks = self
            .inbound_tracks
            .values()
            .map(InboundTrack::snapshot)
            .collect::<Vec<_>>();
        let outbound_tracks = self
            .outbound_tracks
            .values()
            .map(OutboundTrack::snapshot)
            .collect::<Vec<_>>();

        MediaSessionSnapshot {
            can_speak: self.can_speak,
            not_listening_member_ids,
            inbound_track_count: tracks.len(),
            audio_track_count: tracks.iter().filter(|track| track.kind == "audio").count(),
            video_track_count: tracks
                .iter()
                .filter(|track| track.kind.ends_with("video"))
                .count(),
            inbound_packet_count: tracks.iter().map(|track| track.packet_count).sum(),
            outbound_track_count: outbound_tracks.len(),
            outbound_video_track_count: outbound_tracks
                .iter()
                .filter(|track| track.kind.ends_with("video"))
                .count(),
            video_feedback_task_count: self.video_feedback_tasks.len(),
            tracks,
            outbound_tracks,
        }
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        // 媒体会话被替换、关闭或回滚时，立即停止该会话名下的 RTCP 转发任务。
        for (_, task) in self.video_feedback_tasks.drain() {
            task.abort();
        }
    }
}

impl OutboundTrack {
    fn snapshot(&self) -> OutboundTrackSnapshot {
        OutboundTrackSnapshot {
            publisher_member_id: self.publisher_member_id.clone(),
            track_id: self.track_id.clone(),
            kind: self.kind.as_str().to_string(),
        }
    }
}

impl InboundTrack {
    fn from_remote_track(
        track: &TrackRemote,
        room_id: &str,
        member_id: &str,
        kind: MediaTrackKind,
    ) -> Self {
        Self {
            id: track.id(),
            stream_id: track.stream_id(),
            ssrc: track.ssrc(),
            mime_type: track.codec().capability.mime_type,
            kind,
            packet_count: 0,
            fanout_track: Arc::new(TrackLocalStaticRTP::new(
                track.codec().capability,
                fanout_track_id(member_id, &track.id(), kind),
                format!("room-{room_id}"),
            )),
        }
    }

    fn snapshot(&self) -> InboundTrackSnapshot {
        InboundTrackSnapshot {
            id: self.id.clone(),
            stream_id: self.stream_id.clone(),
            ssrc: self.ssrc,
            mime_type: self.mime_type.clone(),
            kind: self.kind.as_str().to_string(),
            packet_count: self.packet_count,
        }
    }
}

impl MediaTrackKind {
    fn as_str(self) -> &'static str {
        match self {
            MediaTrackKind::Audio => "audio",
            MediaTrackKind::ScreenShareVideo => "screen_video",
            MediaTrackKind::CameraVideo => "camera_video",
        }
    }
}

fn fanout_track_id(member_id: &str, track_id: &str, kind: MediaTrackKind) -> String {
    match kind {
        MediaTrackKind::Audio => format!("{member_id}:audio:{track_id}"),
        MediaTrackKind::ScreenShareVideo => format!("{member_id}:screen:{track_id}"),
        MediaTrackKind::CameraVideo => format!("{member_id}:camera:{track_id}"),
    }
}

async fn classify_inbound_video_track(
    sessions: Arc<Mutex<SessionMap>>,
    screen_share_owners: Arc<Mutex<HashMap<String, String>>>,
    video_call_publishers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    session_key: &SessionKey,
    track: &TrackRemote,
) -> Option<MediaTrackKind> {
    let is_screen_owner = screen_share_owners
        .lock()
        .await
        .get(&session_key.0)
        .is_some_and(|owner| owner == &session_key.1);
    let is_camera_publisher = video_call_publishers
        .lock()
        .await
        .get(&session_key.0)
        .is_some_and(|publishers| publishers.contains(&session_key.1));
    let marker = format!(
        "{} {} {}",
        track.id().to_ascii_lowercase(),
        track.stream_id().to_ascii_lowercase(),
        track.rid().to_ascii_lowercase()
    );

    if marker.contains("screen") || marker.contains("display") {
        return is_screen_owner.then_some(MediaTrackKind::ScreenShareVideo);
    }
    if marker.contains("camera") || marker.contains("cam") {
        return is_camera_publisher.then_some(MediaTrackKind::CameraVideo);
    }

    match (is_screen_owner, is_camera_publisher) {
        (true, false) => Some(MediaTrackKind::ScreenShareVideo),
        (false, true) => Some(MediaTrackKind::CameraVideo),
        (false, false) => None,
        (true, true) => {
            let sessions = sessions.lock().await;
            let session = sessions.get(session_key)?;
            let has_screen = session
                .inbound_tracks
                .values()
                .any(|track| track.kind == MediaTrackKind::ScreenShareVideo);
            let has_camera = session
                .inbound_tracks
                .values()
                .any(|track| track.kind == MediaTrackKind::CameraVideo);
            if !has_screen {
                Some(MediaTrackKind::ScreenShareVideo)
            } else if !has_camera {
                Some(MediaTrackKind::CameraVideo)
            } else {
                None
            }
        }
    }
}

fn sender_for_outbound_track(
    session: &MediaSession,
    outbound_track: &OutboundTrack,
) -> Option<Arc<RTCRtpSender>> {
    match outbound_track.kind {
        MediaTrackKind::Audio => session
            .downlink_senders
            .get(outbound_track.downlink_slot_index)
            .cloned(),
        MediaTrackKind::ScreenShareVideo => session.screen_video_downlink_sender.clone(),
        MediaTrackKind::CameraVideo => session
            .camera_video_downlink_senders
            .get(outbound_track.downlink_slot_index)
            .cloned(),
    }
}

async fn read_inbound_rtp(
    track: Arc<TrackRemote>,
    fanout_track: Arc<TrackLocalStaticRTP>,
    sessions: Arc<Mutex<SessionMap>>,
    session_key: SessionKey,
    peer_connection: Arc<RTCPeerConnection>,
    track_id: usize,
    kind: MediaTrackKind,
    screen_share_owners: Arc<Mutex<HashMap<String, String>>>,
    video_call_publishers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
) {
    loop {
        let packet = match track.read_rtp().await {
            Ok((packet, _)) => packet,
            Err(_) => break,
        };

        let should_forward = {
            let mut sessions = sessions.lock().await;
            let Some(session) = sessions.get_mut(&session_key) else {
                break;
            };

            if !Arc::ptr_eq(&session.peer_connection, &peer_connection) {
                break;
            }

            if let Some(track) = session.inbound_tracks.get_mut(&track_id) {
                track.packet_count = track.packet_count.saturating_add(1);
            }

            match kind {
                // 服务端在 RTP 边界执行房间发言权限，避免被禁言客户端继续推音频。
                MediaTrackKind::Audio => session.can_speak,
                MediaTrackKind::ScreenShareVideo | MediaTrackKind::CameraVideo => true,
            }
        };

        let should_forward = should_forward
            && match kind {
                MediaTrackKind::Audio => true,
                MediaTrackKind::ScreenShareVideo => screen_share_owners
                    .lock()
                    .await
                    .get(&session_key.0)
                    .is_some_and(|owner| owner == &session_key.1),
                MediaTrackKind::CameraVideo => video_call_publishers
                    .lock()
                    .await
                    .get(&session_key.0)
                    .is_some_and(|publishers| publishers.contains(&session_key.1)),
            };

        if should_forward {
            let _ = fanout_track.write_rtp_with_extensions(&packet, &[]).await;
        }
    }
}

async fn attach_audio_to_subscribers(
    sessions: Arc<Mutex<SessionMap>>,
    member_not_listening: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
    room_id: &str,
    publisher_member_id: &str,
    track_id: String,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<Vec<String>> {
    let subscriber_keys = {
        let sessions = sessions.lock().await;
        sessions
            .iter()
            .filter_map(|((session_room_id, member_id), _)| {
                if session_room_id == room_id && member_id != publisher_member_id {
                    Some((session_room_id.clone(), member_id.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    let mut subscriber_member_ids = Vec::new();
    for subscriber_key in subscriber_keys {
        if !listener_accepts_publisher(
            Arc::clone(&member_not_listening),
            &subscriber_key,
            publisher_member_id,
        )
        .await
        {
            continue;
        }

        let subscriber_member_id = subscriber_key.1.clone();
        if attach_audio_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key,
            publisher_member_id,
            &track_id,
            Arc::clone(&fanout_track),
        )
        .await?
        {
            subscriber_member_ids.push(subscriber_member_id);
        }
    }

    subscriber_member_ids.sort();
    Ok(subscriber_member_ids)
}

async fn listener_accepts_publisher(
    policies: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
    listener_key: &SessionKey,
    publisher_member_id: &str,
) -> bool {
    let policies = policies.lock().await;
    !policies
        .get(listener_key)
        .is_some_and(|blocked| blocked.contains(publisher_member_id))
}

async fn attach_audio_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    subscriber_key: SessionKey,
    publisher_member_id: &str,
    track_id: &str,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<bool> {
    let downlink_sender = {
        let mut sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get_mut(&subscriber_key) else {
            return Ok(false);
        };

        let downlink_slot_index = match subscriber.outbound_tracks.get(track_id) {
            Some(outbound_track) => outbound_track.downlink_slot_index,
            None => {
                let occupied_slots = subscriber
                    .outbound_tracks
                    .values()
                    .filter(|track| track.kind == MediaTrackKind::Audio)
                    .map(|track| track.downlink_slot_index)
                    .collect::<std::collections::HashSet<_>>();
                (0..subscriber.downlink_senders.len())
                    .find(|slot_index| !occupied_slots.contains(slot_index))
                    .ok_or_else(|| {
                        Error::Internal(format!("成员 {} 的下行音频槽位不足", subscriber_key.1))
                    })?
            }
        };

        let downlink_sender = subscriber
            .downlink_senders
            .get(downlink_slot_index)
            .cloned()
            .ok_or_else(|| Error::Internal("下行音频槽位不存在".to_string()))?;

        subscriber.outbound_tracks.insert(
            track_id.to_string(),
            OutboundTrack {
                publisher_member_id: publisher_member_id.to_string(),
                track_id: track_id.to_string(),
                kind: MediaTrackKind::Audio,
                downlink_slot_index,
                fanout_track: Arc::clone(&fanout_track),
            },
        );
        downlink_sender
    };

    downlink_sender
        .replace_track(Some(fanout_track as Arc<dyn TrackLocal + Send + Sync>))
        .await
        .map_err(|err| Error::Internal(format!("替换下行音频槽位失败: {err}")))?;
    Ok(true)
}

async fn detach_publisher_audio_from_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    listener_member_id: &str,
    publisher_member_id: &str,
) -> Result<()> {
    let subscriber_key = (room_id.to_string(), listener_member_id.to_string());
    let downlink_slots = {
        let mut sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get_mut(&subscriber_key) else {
            return Ok(());
        };

        let track_ids = subscriber
            .outbound_tracks
            .iter()
            .filter_map(|(track_id, outbound_track)| {
                if outbound_track.kind == MediaTrackKind::Audio
                    && outbound_track.publisher_member_id == publisher_member_id
                {
                    Some(track_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        track_ids
            .into_iter()
            .filter_map(|track_id| {
                let outbound_track = subscriber.outbound_tracks.remove(&track_id)?;
                let downlink_sender = subscriber
                    .downlink_senders
                    .get(outbound_track.downlink_slot_index)
                    .cloned()?;
                Some((outbound_track.downlink_slot_index, downlink_sender))
            })
            .collect::<Vec<_>>()
    };

    for (slot_index, downlink_sender) in downlink_slots {
        let empty_slot = new_downlink_slot_track(room_id, listener_member_id, slot_index);
        downlink_sender
            .replace_track(Some(empty_slot as Arc<dyn TrackLocal + Send + Sync>))
            .await
            .map_err(|err| Error::Internal(format!("移除下行音频槽位失败: {err}")))?;
    }

    Ok(())
}

// 发布者关闭媒体会话时，从所有听众下行槽移除该发布者音频，避免槽位被旧 fanout 占住。
async fn detach_publisher_audio_from_subscribers(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
) -> Result<()> {
    let listener_member_ids = {
        let sessions = sessions.lock().await;
        sessions
            .keys()
            .filter_map(|(session_room_id, member_id)| {
                if session_room_id == room_id && member_id != publisher_member_id {
                    Some(member_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    for listener_member_id in listener_member_ids {
        detach_publisher_audio_from_subscriber(
            Arc::clone(&sessions),
            room_id,
            &listener_member_id,
            publisher_member_id,
        )
        .await?;
    }

    Ok(())
}

async fn attach_screen_video_to_subscribers(
    sessions: Arc<Mutex<SessionMap>>,
    screen_share_viewers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    room_id: &str,
    publisher_member_id: &str,
    track_id: String,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<Vec<String>> {
    let subscriber_keys = {
        let sessions = sessions.lock().await;
        let viewers = screen_share_viewers.lock().await;
        sessions
            .iter()
            .filter_map(|((session_room_id, member_id), session)| {
                if session_room_id == room_id
                    && member_id != publisher_member_id
                    && session.screen_video_downlink_sender.is_some()
                    && viewers
                        .get(room_id)
                        .is_some_and(|room_viewers| room_viewers.contains(member_id))
                {
                    Some((session_room_id.clone(), member_id.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    let mut subscriber_member_ids = Vec::new();
    for subscriber_key in subscriber_keys {
        let subscriber_member_id = subscriber_key.1.clone();
        if attach_screen_video_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key,
            publisher_member_id,
            &track_id,
            Arc::clone(&fanout_track),
        )
        .await?
        {
            subscriber_member_ids.push(subscriber_member_id);
        }
    }

    subscriber_member_ids.sort();
    Ok(subscriber_member_ids)
}

async fn attach_screen_video_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    subscriber_key: SessionKey,
    publisher_member_id: &str,
    track_id: &str,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<bool> {
    let downlink_sender = {
        let mut sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get_mut(&subscriber_key) else {
            return Ok(false);
        };
        let Some(downlink_sender) = subscriber.screen_video_downlink_sender.clone() else {
            return Ok(false);
        };

        subscriber.outbound_tracks.insert(
            track_id.to_string(),
            OutboundTrack {
                publisher_member_id: publisher_member_id.to_string(),
                track_id: track_id.to_string(),
                kind: MediaTrackKind::ScreenShareVideo,
                downlink_slot_index: 0,
                fanout_track: Arc::clone(&fanout_track),
            },
        );
        downlink_sender
    };

    downlink_sender
        .replace_track(Some(fanout_track as Arc<dyn TrackLocal + Send + Sync>))
        .await
        .map_err(|err| Error::Internal(format!("替换下行屏幕共享槽位失败: {err}")))?;
    replace_subscriber_video_rtcp_feedback_task(
        Arc::clone(&sessions),
        subscriber_key.clone(),
        publisher_member_id.to_string(),
        MediaTrackKind::ScreenShareVideo,
        0,
        Arc::clone(&downlink_sender),
    )
    .await;
    schedule_publisher_video_keyframes(
        Arc::clone(&sessions),
        subscriber_key.0.clone(),
        publisher_member_id.to_string(),
        MediaTrackKind::ScreenShareVideo,
    );
    Ok(true)
}

async fn attach_camera_video_to_subscribers(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
    track_id: String,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<Vec<String>> {
    let subscriber_keys = {
        let sessions = sessions.lock().await;
        sessions
            .iter()
            .filter_map(|((session_room_id, member_id), session)| {
                if session_room_id == room_id
                    && member_id != publisher_member_id
                    && !session.camera_video_downlink_senders.is_empty()
                {
                    Some((session_room_id.clone(), member_id.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    let mut subscriber_member_ids = Vec::new();
    for subscriber_key in subscriber_keys {
        let subscriber_member_id = subscriber_key.1.clone();
        if attach_camera_video_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key,
            publisher_member_id,
            &track_id,
            Arc::clone(&fanout_track),
        )
        .await?
        {
            subscriber_member_ids.push(subscriber_member_id);
        }
    }

    subscriber_member_ids.sort();
    Ok(subscriber_member_ids)
}

async fn attach_camera_video_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    subscriber_key: SessionKey,
    publisher_member_id: &str,
    track_id: &str,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<bool> {
    let (downlink_sender, downlink_slot_index) = {
        let mut sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get_mut(&subscriber_key) else {
            return Ok(false);
        };
        if subscriber.camera_video_downlink_senders.is_empty() {
            return Ok(false);
        }

        let existing = subscriber
            .outbound_tracks
            .iter()
            .find_map(|(track_id, outbound_track)| {
                if outbound_track.kind == MediaTrackKind::CameraVideo
                    && outbound_track.publisher_member_id == publisher_member_id
                {
                    Some((track_id.clone(), outbound_track.downlink_slot_index))
                } else {
                    None
                }
            });
        let downlink_slot_index = match existing {
            Some((existing_track_id, slot_index)) => {
                if existing_track_id != track_id {
                    subscriber.outbound_tracks.remove(&existing_track_id);
                }
                slot_index
            }
            None => {
                let occupied_slots = subscriber
                    .outbound_tracks
                    .values()
                    .filter(|track| track.kind == MediaTrackKind::CameraVideo)
                    .map(|track| track.downlink_slot_index)
                    .collect::<HashSet<_>>();
                (0..subscriber.camera_video_downlink_senders.len())
                    .find(|slot_index| !occupied_slots.contains(slot_index))
                    .ok_or_else(|| {
                        Error::Internal(format!(
                            "成员 {} 订阅发布者 {} 时下行摄像头槽位不足",
                            subscriber_key.1, publisher_member_id
                        ))
                    })?
            }
        };

        let downlink_sender = subscriber
            .camera_video_downlink_senders
            .get(downlink_slot_index)
            .cloned()
            .ok_or_else(|| Error::Internal("下行摄像头槽位不存在".to_string()))?;

        subscriber.outbound_tracks.insert(
            track_id.to_string(),
            OutboundTrack {
                publisher_member_id: publisher_member_id.to_string(),
                track_id: track_id.to_string(),
                kind: MediaTrackKind::CameraVideo,
                downlink_slot_index,
                fanout_track: Arc::clone(&fanout_track),
            },
        );
        (downlink_sender, downlink_slot_index)
    };

    downlink_sender
        .replace_track(Some(fanout_track as Arc<dyn TrackLocal + Send + Sync>))
        .await
        .map_err(|err| Error::Internal(format!("替换下行摄像头槽位失败: {err}")))?;
    replace_subscriber_video_rtcp_feedback_task(
        Arc::clone(&sessions),
        subscriber_key.clone(),
        publisher_member_id.to_string(),
        MediaTrackKind::CameraVideo,
        downlink_slot_index,
        Arc::clone(&downlink_sender),
    )
    .await;
    schedule_publisher_video_keyframes(
        Arc::clone(&sessions),
        subscriber_key.0.clone(),
        publisher_member_id.to_string(),
        MediaTrackKind::CameraVideo,
    );
    Ok(true)
}

async fn replace_subscriber_video_rtcp_feedback_task(
    sessions: Arc<Mutex<SessionMap>>,
    subscriber_key: SessionKey,
    publisher_member_id: String,
    kind: MediaTrackKind,
    slot_index: usize,
    downlink_sender: Arc<RTCRtpSender>,
) {
    let task = spawn_subscriber_video_rtcp_feedback(
        Arc::clone(&sessions),
        subscriber_key.0.clone(),
        publisher_member_id,
        kind,
        downlink_sender,
    );
    let previous = {
        let mut sessions = sessions.lock().await;
        let Some(session) = sessions.get_mut(&subscriber_key) else {
            task.abort();
            return;
        };
        session
            .video_feedback_tasks
            .insert(VideoFeedbackSlot { kind, slot_index }, task)
    };
    if let Some(previous) = previous {
        previous.abort();
    }
}

fn spawn_subscriber_video_rtcp_feedback(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: String,
    publisher_member_id: String,
    kind: MediaTrackKind,
    downlink_sender: Arc<RTCRtpSender>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok((packets, _)) = downlink_sender.read_rtcp().await else {
                break;
            };

            let publisher = {
                let sessions = sessions.lock().await;
                let publisher_key = (room_id.clone(), publisher_member_id.clone());
                let Some(publisher) = sessions.get(&publisher_key) else {
                    break;
                };
                let Some(media_ssrc) = publisher
                    .inbound_tracks
                    .values()
                    .find(|track| track.kind == kind)
                    .map(|track| track.ssrc)
                else {
                    break;
                };

                (Arc::clone(&publisher.peer_connection), media_ssrc)
            };

            let forwarded = packets
                .iter()
                .filter_map(|packet| {
                    rewrite_video_feedback_for_publisher(packet.as_ref(), publisher.1)
                })
                .collect::<Vec<_>>();
            if !forwarded.is_empty() {
                let _ = publisher.0.write_rtcp(&forwarded).await;
            }
        }
    })
}

fn rewrite_video_feedback_for_publisher(
    packet: &(dyn RtcpPacket + Send + Sync),
    publisher_media_ssrc: u32,
) -> Option<Box<dyn RtcpPacket + Send + Sync>> {
    if let Some(pli) = packet.as_any().downcast_ref::<PictureLossIndication>() {
        let mut forwarded = pli.clone();
        forwarded.media_ssrc = publisher_media_ssrc;
        return Some(Box::new(forwarded));
    }
    if let Some(nack) = packet.as_any().downcast_ref::<TransportLayerNack>() {
        let mut forwarded = nack.clone();
        forwarded.media_ssrc = publisher_media_ssrc;
        return Some(Box::new(forwarded));
    }
    if let Some(fir) = packet.as_any().downcast_ref::<FullIntraRequest>() {
        let mut forwarded = fir.clone();
        forwarded.media_ssrc = publisher_media_ssrc;
        for entry in &mut forwarded.fir {
            entry.ssrc = publisher_media_ssrc;
        }
        return Some(Box::new(forwarded));
    }

    None
}

fn schedule_publisher_video_keyframes(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: String,
    publisher_member_id: String,
    kind: MediaTrackKind,
) {
    tokio::spawn(async move {
        for delay_ms in VIDEO_KEYFRAME_REQUEST_DELAYS_MS {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            let _ = request_publisher_video_keyframe(
                Arc::clone(&sessions),
                &room_id,
                &publisher_member_id,
                kind,
            )
            .await;
        }
    });
}

async fn request_publisher_video_keyframe(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
    kind: MediaTrackKind,
) -> Result<()> {
    let request = {
        let sessions = sessions.lock().await;
        let publisher_key = (room_id.to_string(), publisher_member_id.to_string());
        let Some(publisher) = sessions.get(&publisher_key) else {
            return Ok(());
        };
        let Some(media_ssrc) = publisher
            .inbound_tracks
            .values()
            .find(|track| track.kind == kind)
            .map(|track| track.ssrc)
        else {
            return Ok(());
        };

        (Arc::clone(&publisher.peer_connection), media_ssrc)
    };

    request
        .0
        .write_rtcp(&[Box::new(PictureLossIndication {
            sender_ssrc: 0,
            media_ssrc: request.1,
        })])
        .await
        .map_err(|err| Error::Internal(format!("请求视频关键帧失败: {err}")))?;
    Ok(())
}

async fn detach_publisher_video_from_subscribers(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
    kind: MediaTrackKind,
) -> Result<()> {
    let downlink_senders = {
        let mut sessions = sessions.lock().await;
        sessions
            .iter_mut()
            .filter_map(|((session_room_id, listener_member_id), session)| {
                if session_room_id != room_id || listener_member_id == publisher_member_id {
                    return None;
                }

                let track_ids = session
                    .outbound_tracks
                    .iter()
                    .filter_map(|(track_id, outbound_track)| {
                        if outbound_track.kind == kind
                            && outbound_track.publisher_member_id == publisher_member_id
                        {
                            Some(track_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if track_ids.is_empty() {
                    return None;
                }
                let slot_index = track_ids
                    .iter()
                    .find_map(|track_id| {
                        session
                            .outbound_tracks
                            .get(track_id)
                            .map(|track| track.downlink_slot_index)
                    })
                    .unwrap_or(0);

                for track_id in track_ids {
                    session.outbound_tracks.remove(&track_id);
                }
                if let Some(task) = session
                    .video_feedback_tasks
                    .remove(&VideoFeedbackSlot { kind, slot_index })
                {
                    task.abort();
                }

                match kind {
                    MediaTrackKind::ScreenShareVideo => {
                        session.screen_video_downlink_sender.clone().map(|sender| {
                            (
                                session_room_id.clone(),
                                listener_member_id.clone(),
                                0,
                                sender,
                            )
                        })
                    }
                    MediaTrackKind::CameraVideo => session
                        .camera_video_downlink_senders
                        .get(slot_index)
                        .cloned()
                        .map(|sender| {
                            (
                                session_room_id.clone(),
                                listener_member_id.clone(),
                                slot_index,
                                sender,
                            )
                        }),
                    MediaTrackKind::Audio => None,
                }
            })
            .collect::<Vec<_>>()
    };

    for (session_room_id, listener_member_id, slot_index, downlink_sender) in downlink_senders {
        let empty_slot = match kind {
            MediaTrackKind::ScreenShareVideo => {
                new_screen_video_downlink_slot_track(&session_room_id, &listener_member_id)
            }
            MediaTrackKind::CameraVideo => new_camera_video_downlink_slot_track(
                &session_room_id,
                &listener_member_id,
                slot_index,
            ),
            MediaTrackKind::Audio => continue,
        };
        downlink_sender
            .replace_track(Some(empty_slot as Arc<dyn TrackLocal + Send + Sync>))
            .await
            .map_err(|err| Error::Internal(format!("移除下行视频槽位失败: {err}")))?;
    }

    Ok(())
}

async fn attach_existing_audio_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    member_not_listening: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
    room_id: &str,
    member_id: &str,
) -> Result<()> {
    let subscriber_key = (room_id.to_string(), member_id.to_string());
    let existing_audio = {
        let sessions = sessions.lock().await;
        if !sessions.contains_key(&subscriber_key) {
            return Ok(());
        }

        sessions
            .iter()
            .flat_map(|((session_room_id, publisher_member_id), session)| {
                if session_room_id != room_id || publisher_member_id == member_id {
                    return Vec::new();
                }

                session
                    .inbound_tracks
                    .values()
                    .filter(|track| track.kind == MediaTrackKind::Audio)
                    .map(|track| {
                        (
                            publisher_member_id.clone(),
                            fanout_track_id(publisher_member_id, &track.id, track.kind),
                            Arc::clone(&track.fanout_track),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };

    for (publisher_member_id, track_id, fanout_track) in existing_audio {
        if !listener_accepts_publisher(
            Arc::clone(&member_not_listening),
            &subscriber_key,
            &publisher_member_id,
        )
        .await
        {
            continue;
        }

        attach_audio_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key.clone(),
            &publisher_member_id,
            &track_id,
            fanout_track,
        )
        .await
        .map_err(|err| Error::Internal(format!("晚加入听众接入下行音频失败: {err}")))?;
    }

    Ok(())
}

async fn attach_existing_publisher_audio_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    member_not_listening: Arc<Mutex<HashMap<SessionKey, HashSet<String>>>>,
    room_id: &str,
    listener_member_id: &str,
    publisher_member_id: &str,
) -> Result<()> {
    let subscriber_key = (room_id.to_string(), listener_member_id.to_string());
    if !listener_accepts_publisher(
        Arc::clone(&member_not_listening),
        &subscriber_key,
        publisher_member_id,
    )
    .await
    {
        return Ok(());
    }

    let existing_audio = {
        let sessions = sessions.lock().await;
        if !sessions.contains_key(&subscriber_key) {
            return Ok(());
        }

        let publisher_key = (room_id.to_string(), publisher_member_id.to_string());
        let Some(publisher) = sessions.get(&publisher_key) else {
            return Ok(());
        };

        publisher
            .inbound_tracks
            .values()
            .filter(|track| track.kind == MediaTrackKind::Audio)
            .map(|track| {
                (
                    fanout_track_id(publisher_member_id, &track.id, track.kind),
                    Arc::clone(&track.fanout_track),
                )
            })
            .collect::<Vec<_>>()
    };

    for (track_id, fanout_track) in existing_audio {
        attach_audio_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key.clone(),
            publisher_member_id,
            &track_id,
            fanout_track,
        )
        .await
        .map_err(|err| Error::Internal(format!("恢复下行音频失败: {err}")))?;
    }

    Ok(())
}

async fn attach_existing_screen_video_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    screen_share_owners: Arc<Mutex<HashMap<String, String>>>,
    screen_share_viewers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    room_id: &str,
    member_id: &str,
) -> Result<()> {
    let publisher_member_id = {
        let owners = screen_share_owners.lock().await;
        owners.get(room_id).cloned()
    };
    let Some(publisher_member_id) = publisher_member_id else {
        return Ok(());
    };
    if publisher_member_id == member_id {
        return Ok(());
    }
    let is_viewing = {
        let viewers = screen_share_viewers.lock().await;
        viewers
            .get(room_id)
            .is_some_and(|room_viewers| room_viewers.contains(member_id))
    };
    if !is_viewing {
        return Ok(());
    }

    let subscriber_key = (room_id.to_string(), member_id.to_string());
    let existing_video = {
        let sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get(&subscriber_key) else {
            return Ok(());
        };
        if subscriber.screen_video_downlink_sender.is_none() {
            return Ok(());
        }

        let publisher_key = (room_id.to_string(), publisher_member_id.clone());
        let Some(publisher) = sessions.get(&publisher_key) else {
            return Ok(());
        };

        publisher
            .inbound_tracks
            .values()
            .filter(|track| track.kind == MediaTrackKind::ScreenShareVideo)
            .map(|track| {
                (
                    fanout_track_id(&publisher_member_id, &track.id, track.kind),
                    Arc::clone(&track.fanout_track),
                )
            })
            .collect::<Vec<_>>()
    };

    for (track_id, fanout_track) in existing_video {
        attach_screen_video_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key.clone(),
            &publisher_member_id,
            &track_id,
            fanout_track,
        )
        .await
        .map_err(|err| Error::Internal(format!("恢复下行屏幕共享失败: {err}")))?;
    }

    Ok(())
}

async fn attach_existing_screen_video_to_subscribers_for_publisher(
    sessions: Arc<Mutex<SessionMap>>,
    screen_share_viewers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    room_id: &str,
    publisher_member_id: &str,
) -> Result<()> {
    let existing_video = {
        let sessions = sessions.lock().await;
        let publisher_key = (room_id.to_string(), publisher_member_id.to_string());
        let Some(publisher) = sessions.get(&publisher_key) else {
            return Ok(());
        };

        publisher
            .inbound_tracks
            .values()
            .filter(|track| track.kind == MediaTrackKind::ScreenShareVideo)
            .map(|track| {
                (
                    fanout_track_id(publisher_member_id, &track.id, track.kind),
                    Arc::clone(&track.fanout_track),
                )
            })
            .collect::<Vec<_>>()
    };

    for (track_id, fanout_track) in existing_video {
        attach_screen_video_to_subscribers(
            Arc::clone(&sessions),
            Arc::clone(&screen_share_viewers),
            room_id,
            publisher_member_id,
            track_id,
            fanout_track,
        )
        .await?;
    }

    Ok(())
}

async fn attach_existing_camera_videos_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    video_call_publishers: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    room_id: &str,
    member_id: &str,
) -> Result<()> {
    let room_publishers = {
        let publishers = video_call_publishers.lock().await;
        publishers.get(room_id).cloned().unwrap_or_default()
    };
    if room_publishers.is_empty() {
        return Ok(());
    }

    let subscriber_key = (room_id.to_string(), member_id.to_string());
    let existing_camera = {
        let sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get(&subscriber_key) else {
            return Ok(());
        };
        if subscriber.camera_video_downlink_senders.is_empty() {
            return Ok(());
        }

        sessions
            .iter()
            .flat_map(|((session_room_id, publisher_member_id), session)| {
                if session_room_id != room_id
                    || publisher_member_id == member_id
                    || !room_publishers.contains(publisher_member_id)
                {
                    return Vec::new();
                }

                session
                    .inbound_tracks
                    .values()
                    .filter(|track| track.kind == MediaTrackKind::CameraVideo)
                    .map(|track| {
                        (
                            publisher_member_id.clone(),
                            fanout_track_id(publisher_member_id, &track.id, track.kind),
                            Arc::clone(&track.fanout_track),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };

    for (publisher_member_id, track_id, fanout_track) in existing_camera {
        attach_camera_video_to_subscriber(
            Arc::clone(&sessions),
            subscriber_key.clone(),
            &publisher_member_id,
            &track_id,
            fanout_track,
        )
        .await
        .map_err(|err| Error::Internal(format!("恢复下行摄像头失败: {err}")))?;
    }

    Ok(())
}

async fn attach_existing_camera_video_to_subscribers_for_publisher(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
) -> Result<()> {
    let existing_camera = {
        let sessions = sessions.lock().await;
        let publisher_key = (room_id.to_string(), publisher_member_id.to_string());
        let Some(publisher) = sessions.get(&publisher_key) else {
            return Ok(());
        };

        publisher
            .inbound_tracks
            .values()
            .filter(|track| track.kind == MediaTrackKind::CameraVideo)
            .map(|track| {
                (
                    fanout_track_id(publisher_member_id, &track.id, track.kind),
                    Arc::clone(&track.fanout_track),
                )
            })
            .collect::<Vec<_>>()
    };

    for (track_id, fanout_track) in existing_camera {
        attach_camera_video_to_subscribers(
            Arc::clone(&sessions),
            room_id,
            publisher_member_id,
            track_id,
            fanout_track,
        )
        .await?;
    }

    Ok(())
}

async fn detach_current_video_from_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    listener_member_id: &str,
) -> Result<()> {
    let downlink_sender = {
        let mut sessions = sessions.lock().await;
        let subscriber_key = (room_id.to_string(), listener_member_id.to_string());
        let Some(subscriber) = sessions.get_mut(&subscriber_key) else {
            return Ok(());
        };

        let track_ids = subscriber
            .outbound_tracks
            .iter()
            .filter_map(|(track_id, outbound_track)| {
                if outbound_track.kind == MediaTrackKind::ScreenShareVideo {
                    Some(track_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        for track_id in track_ids {
            subscriber.outbound_tracks.remove(&track_id);
        }
        if let Some(task) = subscriber.video_feedback_tasks.remove(&VideoFeedbackSlot {
            kind: MediaTrackKind::ScreenShareVideo,
            slot_index: 0,
        }) {
            task.abort();
        }

        subscriber.screen_video_downlink_sender.clone()
    };

    let Some(downlink_sender) = downlink_sender else {
        return Ok(());
    };

    let empty_slot = new_screen_video_downlink_slot_track(room_id, listener_member_id);
    downlink_sender
        .replace_track(Some(empty_slot as Arc<dyn TrackLocal + Send + Sync>))
        .await
        .map_err(|err| Error::Internal(format!("停止接收屏幕共享失败: {err}")))
}

async fn remove_inbound_video_tracks(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
    kind: MediaTrackKind,
) {
    let key = (room_id.to_string(), publisher_member_id.to_string());
    let mut sessions = sessions.lock().await;
    let Some(session) = sessions.get_mut(&key) else {
        return;
    };
    session.inbound_tracks.retain(|_, track| track.kind != kind);
}

#[cfg(test)]
mod tests {
    use super::{IceCandidate, MediaController, MediaTrackKind};
    use crate::Error;
    use std::{sync::Arc, time::Duration};
    use tokio::{
        sync::{Mutex, broadcast, mpsc},
        time::{sleep, timeout},
    };
    use webrtc::{
        api::{
            APIBuilder,
            interceptor_registry::register_default_interceptors,
            media_engine::{MIME_TYPE_OPUS, MIME_TYPE_VP8, MediaEngine},
            setting_engine::SettingEngine,
        },
        interceptor::registry::Registry,
        peer_connection::{
            RTCPeerConnection, configuration::RTCConfiguration,
            peer_connection_state::RTCPeerConnectionState,
            sdp::session_description::RTCSessionDescription,
        },
        rtcp::{
            payload_feedbacks::picture_loss_indication::PictureLossIndication,
            transport_feedbacks::transport_layer_nack::{NackPair, TransportLayerNack},
        },
        rtp_transceiver::{
            rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
            rtp_sender::RTCRtpSender,
        },
        track::track_local::{
            TrackLocal, TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP,
        },
        util::vnet::{
            net::{Net, NetConfig},
            router::{Router, RouterConfig},
        },
    };

    #[tokio::test]
    async fn 媒体控制器可以根据_offer_生成_answer() {
        let media = MediaController::new().expect("创建媒体控制器");
        let offer_sdp = create_audio_offer().await;

        let answer = media
            .handle_offer("room-1", "member-1", offer_sdp)
            .await
            .expect("根据 offer 生成 answer");

        assert!(answer.sdp.contains("m=audio"));
    }

    #[tokio::test]
    async fn udp_端口范围成为服务端_ice_candidate_端口() {
        let udp_port = free_udp_port().await;
        let media = MediaController::new_with_udp_port_range(udp_port, udp_port, None)
            .expect("创建固定 UDP 端口范围媒体控制器");

        let mut answer = media
            .handle_offer("room-1", "member-1", create_audio_offer().await)
            .await
            .expect("根据 offer 生成 answer");
        let candidate = timeout(Duration::from_secs(2), answer.local_ice_candidates.recv())
            .await
            .expect("服务端 ICE candidate 未超时")
            .expect("服务端 ICE candidate 存在");
        let candidate_port = candidate
            .candidate
            .split_whitespace()
            .nth(5)
            .expect("candidate 带端口")
            .parse::<u16>()
            .expect("candidate 端口可解析");

        assert_eq!(candidate_port, udp_port);
    }

    #[tokio::test]
    async fn 公网_ip_成为_nat_后的服务端_host_candidate() {
        let udp_port = free_udp_port().await;
        let media = MediaController::new_with_udp_port_range(
            udp_port,
            udp_port,
            Some("203.0.113.10".to_string()),
        )
        .expect("创建公网 ICE 媒体控制器");

        let mut answer = media
            .handle_offer("room-1", "member-1", create_audio_offer().await)
            .await
            .expect("根据 offer 生成 answer");
        let mut candidates = Vec::new();
        let mut saw_public_ip = false;
        for _ in 0..8 {
            let candidate = timeout(Duration::from_secs(2), answer.local_ice_candidates.recv())
                .await
                .expect("服务端 ICE candidate 未超时")
                .expect("服务端 ICE candidate 存在");
            saw_public_ip |= candidate.candidate.contains("203.0.113.10");
            candidates.push(candidate.candidate);
            if saw_public_ip {
                break;
            }
        }

        assert!(
            saw_public_ip,
            "配置的公网 IP 应进入 candidate: {}",
            candidates.join(" | ")
        );
    }

    async fn free_udp_port() -> u16 {
        tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("绑定测试 UDP 端口")
            .local_addr()
            .expect("读取测试 UDP 端口")
            .port()
    }

    #[tokio::test]
    async fn offer_建立媒体会话_关闭成员后清理会话() {
        let media = MediaController::new().expect("创建媒体控制器");
        let offer_sdp = create_audio_offer().await;

        assert!(media.session_snapshot("room-1", "member-1").await.is_none());

        media
            .handle_offer("room-1", "member-1", offer_sdp)
            .await
            .expect("根据 offer 建立媒体会话");

        let snapshot = media
            .session_snapshot("room-1", "member-1")
            .await
            .expect("offer 后存在媒体会话");
        assert_eq!(snapshot.inbound_track_count, 0);
        assert_eq!(snapshot.audio_track_count, 0);
        assert_eq!(snapshot.inbound_packet_count, 0);
        assert_eq!(snapshot.outbound_track_count, 0);

        media
            .close_member("room-1", "member-1")
            .await
            .expect("关闭媒体会话");

        assert!(media.session_snapshot("room-1", "member-1").await.is_none());
    }

    #[tokio::test]
    async fn 发布者音频会为同房间其他会话挂下行_track() {
        let media = MediaController::new().expect("创建媒体控制器");

        media
            .handle_offer("room-1", "publisher-1", create_audio_offer().await)
            .await
            .expect("建立发布者会话");
        media
            .handle_offer("room-1", "listener-1", create_audio_offer().await)
            .await
            .expect("建立听众会话");
        media
            .handle_offer("room-2", "listener-2", create_audio_offer().await)
            .await
            .expect("建立其他房间会话");

        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
            .await
            .expect("为同房间听众挂下行 track");

        assert_eq!(
            media
                .session_snapshot("room-1", "publisher-1")
                .await
                .expect("发布者会话存在")
                .outbound_track_count,
            0
        );
        let listener_snapshot = media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(listener_snapshot.outbound_track_count, 1);
        assert_eq!(
            listener_snapshot.outbound_tracks[0].publisher_member_id,
            "publisher-1"
        );
        assert_eq!(
            media
                .session_snapshot("room-2", "listener-2")
                .await
                .expect("其他房间会话存在")
                .outbound_track_count,
            0
        );
    }

    #[tokio::test]
    async fn 三人房听众保留多个发布者的下行_track() {
        let media = MediaController::new().expect("创建媒体控制器");

        for member_id in ["publisher-1", "publisher-2", "listener-1"] {
            media
                .handle_offer("room-1", member_id, create_audio_offer().await)
                .await
                .expect("建立媒体会话");
        }

        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
            .await
            .expect("挂发布者 1 下行 track");
        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-2")
            .await
            .expect("挂发布者 2 下行 track");

        let listener_snapshot = media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(listener_snapshot.outbound_track_count, 2);
        assert!(
            listener_snapshot
                .outbound_tracks
                .iter()
                .any(|track| track.publisher_member_id == "publisher-1")
        );
        assert!(
            listener_snapshot
                .outbound_tracks
                .iter()
                .any(|track| track.publisher_member_id == "publisher-2")
        );
    }

    #[tokio::test]
    async fn 视频下行不会占用音频槽位() {
        let media = MediaController::new_with_downlink_slot_count(1).expect("创建媒体控制器");
        for member_id in ["screen-publisher", "audio-publisher", "listener-1"] {
            media
                .handle_offer("room-1", member_id, create_audio_video_offer(1).await)
                .await
                .expect("建立带视频槽位的媒体会话");
        }
        media
            .set_screen_viewing("room-1", "listener-1", true)
            .await
            .expect("听众观看屏幕");
        media
            .store_test_video_inbound_track(
                "room-1",
                "screen-publisher",
                MediaTrackKind::ScreenShareVideo,
            )
            .await
            .expect("登记屏幕共享上行");
        media
            .set_screen_share_owner("room-1", Some("screen-publisher"))
            .await
            .expect("开启屏幕共享");

        media
            .attach_audio_to_subscribers_for_test("room-1", "audio-publisher")
            .await
            .expect("视频下行不应占用唯一音频槽");

        let snapshot = media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(snapshot.outbound_track_count, 2);
        assert_eq!(
            snapshot
                .outbound_tracks
                .iter()
                .filter(|track| track.kind == "audio")
                .count(),
            1
        );
        assert_eq!(snapshot.outbound_video_track_count, 1);
    }

    #[tokio::test]
    async fn 发布者关闭后释放所有听众的音频槽位() {
        let media = MediaController::new_with_downlink_slot_count(1).expect("创建媒体控制器");
        for member_id in ["publisher-1", "publisher-2", "listener-1"] {
            media
                .handle_offer("room-1", member_id, create_audio_offer().await)
                .await
                .expect("建立媒体会话");
        }
        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
            .await
            .expect("发布者 1 占用唯一音频槽");

        media
            .close_member("room-1", "publisher-1")
            .await
            .expect("关闭发布者 1");
        assert_eq!(
            media
                .session_snapshot("room-1", "listener-1")
                .await
                .expect("听众会话存在")
                .outbound_track_count,
            0
        );

        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-2")
            .await
            .expect("发布者 2 复用释放后的音频槽");
        let snapshot = media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(snapshot.outbound_track_count, 1);
        assert_eq!(
            snapshot.outbound_tracks[0].publisher_member_id,
            "publisher-2"
        );
    }

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
    async fn 恢复音频策略会替换不听名单并同步已有下行_track() {
        let media = MediaController::new().expect("创建媒体控制器");
        for member_id in ["publisher-1", "publisher-2", "listener-1"] {
            media
                .handle_offer("room-1", member_id, create_audio_offer().await)
                .await
                .expect("建立媒体会话");
        }
        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
            .await
            .expect("挂发布者 1 音轨");
        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-2")
            .await
            .expect("挂发布者 2 音轨");

        media
            .sync_member_audio_policy("room-1", "listener-1", true, &["publisher-1".to_string()])
            .await
            .expect("恢复不听发布者 1");
        let mut publishers = media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众会话存在")
            .outbound_tracks
            .iter()
            .map(|track| track.publisher_member_id.clone())
            .collect::<Vec<_>>();
        publishers.sort_unstable();
        assert_eq!(publishers, vec!["publisher-2".to_string()]);

        media
            .sync_member_audio_policy("room-1", "listener-1", true, &["publisher-2".to_string()])
            .await
            .expect("替换为不听发布者 2");
        let mut publishers = media
            .session_snapshot("room-1", "listener-1")
            .await
            .expect("听众会话存在")
            .outbound_tracks
            .iter()
            .map(|track| track.publisher_member_id.clone())
            .collect::<Vec<_>>();
        publishers.sort_unstable();
        assert_eq!(publishers, vec!["publisher-1".to_string()]);

        media
            .sync_member_audio_policy("room-1", "listener-1", true, &[])
            .await
            .expect("清空不听名单");
        assert_eq!(
            media
                .session_snapshot("room-1", "listener-1")
                .await
                .expect("听众会话存在")
                .outbound_track_count,
            2
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

    #[tokio::test]
    async fn 听众重新_offer_后保留已挂载的下行_track() {
        let media = MediaController::new().expect("创建媒体控制器");

        media
            .handle_offer("room-1", "publisher-1", create_audio_offer().await)
            .await
            .expect("建立发布者会话");
        media
            .handle_offer("room-1", "listener-1", create_audio_offer().await)
            .await
            .expect("建立听众会话");
        media
            .attach_audio_to_subscribers_for_test("room-1", "publisher-1")
            .await
            .expect("为听众挂下行 track");

        media
            .handle_offer("room-1", "listener-1", create_audio_offer().await)
            .await
            .expect("听众重新 offer");

        assert_eq!(
            media
                .session_snapshot("room-1", "listener-1")
                .await
                .expect("听众会话存在")
                .outbound_track_count,
            1
        );
    }

    #[tokio::test]
    async fn 上行音频_rtp_经后端转发给同房间听众() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .expect("听众声明音频 transceiver");
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_track();
        publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者添加音频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;

        send_until_publisher_audio_arrives(&publisher_track, &mut media_events, "publisher-1")
            .await;
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众媒体会话存在")
                .outbound_track_count,
            1
        );
        negotiate_client(&media, room_id, "listener-1", &listener).await;

        let received_payload = send_until_listener_receives(&publisher_track, &mut packet_receiver)
            .await
            .expect("听众收到转发 RTP");
        assert_eq!(received_payload, vec![0xA5]);

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 三人房听众接收两个发布者的上行音频() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        for _ in 0..2 {
            listener
                .add_transceiver_from_kind(RTPCodecType::Audio, None)
                .await
                .expect("听众声明音频 transceiver");
        }
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        let publisher_one = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_one_track = new_publisher_track();
        publisher_one
            .add_track(Arc::clone(&publisher_one_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者 1 添加音频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher_one).await;
        wait_for_connected(&publisher_one).await;
        send_until_publisher_audio_arrives(&publisher_one_track, &mut media_events, "publisher-1")
            .await;

        let publisher_two = new_test_peer_connection(Arc::clone(&test_network.publisher_two)).await;
        let publisher_two_track = new_publisher_track();
        publisher_two
            .add_track(Arc::clone(&publisher_two_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者 2 添加音频 track");
        negotiate_client(&media, room_id, "publisher-2", &publisher_two).await;
        wait_for_connected(&publisher_two).await;
        send_until_publisher_audio_arrives(&publisher_two_track, &mut media_events, "publisher-2")
            .await;

        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众媒体会话存在")
                .outbound_track_count,
            2
        );
        negotiate_client(&media, room_id, "listener-1", &listener).await;

        assert_eq!(
            send_until_listener_receives_payload(
                &publisher_one_track,
                &mut packet_receiver,
                vec![0xA1],
            )
            .await,
            Some(vec![0xA1])
        );
        assert_eq!(
            send_until_listener_receives_payload(
                &publisher_two_track,
                &mut packet_receiver,
                vec![0xB2],
            )
            .await,
            Some(vec![0xB2])
        );

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher_one
            .close()
            .await
            .expect("关闭发布者 1 PeerConnection");
        publisher_two
            .close()
            .await
            .expect("关闭发布者 2 PeerConnection");
    }

    #[tokio::test]
    async fn 已有发布者时新听众加入后接收上行音频() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_track();
        publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者添加音频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;
        send_until_publisher_audio_registered(&media, room_id, "publisher-1", &publisher_track)
            .await;
        assert!(
            timeout(Duration::from_millis(200), media_events.recv())
                .await
                .is_err(),
            "没有听众时不应产生重新协商媒体事件"
        );

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .expect("听众声明音频 transceiver");
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        let received_payload = send_until_listener_receives(&publisher_track, &mut packet_receiver)
            .await
            .expect("新听众收到发布者 RTP");
        assert_eq!(received_payload, vec![0xA5]);

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 非共享者视频_track_不会转发() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Video, None)
            .await
            .expect("听众声明视频 transceiver");
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_video_track();
        publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者添加视频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;

        for sequence_number in 0..10 {
            publisher_track
                .write_rtp_with_extensions(&test_video_rtp_packet(sequence_number), &[])
                .await
                .expect("发布测试视频 RTP");
            sleep(Duration::from_millis(10)).await;
        }

        assert!(
            timeout(Duration::from_millis(200), media_events.recv())
                .await
                .is_err(),
            "非共享者视频 track 不应产生媒体事件"
        );
        assert_eq!(
            media
                .session_snapshot(room_id, "publisher-1")
                .await
                .expect("发布者媒体会话存在")
                .video_track_count,
            0
        );
        assert!(
            timeout(Duration::from_millis(200), packet_receiver.recv())
                .await
                .is_err(),
            "非共享者视频不应转发给听众"
        );

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 共享者视频_track_只会转发给正在观看的听众() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();
        media
            .set_screen_share_owner(room_id, Some("publisher-1"))
            .await
            .expect("设置屏幕共享者");

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Video, None)
            .await
            .expect("听众声明视频 transceiver");
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;
        let viewer_count = media
            .set_screen_viewing(room_id, "listener-1", true)
            .await
            .expect("听众开始观看屏幕");
        assert_eq!(viewer_count, 1);

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_video_track();
        let publisher_sender = publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("共享者添加视频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;

        let media_event =
            send_until_publisher_video_event(&publisher_track, &mut media_events, "publisher-1")
                .await;
        match media_event {
            super::MediaEvent::InboundScreenVideoTrack {
                subscriber_member_ids,
                ..
            } => assert_eq!(subscriber_member_ids, vec!["listener-1".to_string()]),
            other => panic!("收到非预期媒体事件: {other:?}"),
        }
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众媒体会话存在")
                .outbound_video_track_count,
            1
        );
        let publisher_video_ssrc = media
            .session_snapshot(room_id, "publisher-1")
            .await
            .expect("共享者媒体会话存在")
            .tracks
            .into_iter()
            .find(|track| track.kind == "screen_video")
            .expect("共享者视频 track 存在")
            .ssrc;
        assert!(
            publisher_receives_pli_count(&publisher_sender, publisher_video_ssrc, 2).await >= 2,
            "听众接入屏幕共享后服务端应在协商窗口内重复请求关键帧"
        );

        let received_payload =
            send_until_listener_receives_video(&publisher_track, &mut packet_receiver)
                .await
                .expect("听众收到屏幕共享 RTP");
        assert_eq!(received_payload, vec![0xC7]);

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 听众视频_rtcp_反馈会转发给共享者() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();
        media
            .set_screen_share_owner(room_id, Some("publisher-1"))
            .await
            .expect("设置屏幕共享者");

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Video, None)
            .await
            .expect("听众声明视频 transceiver");
        let mut packet_receiver = receive_first_track_packet_info(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;
        media
            .set_screen_viewing(room_id, "listener-1", true)
            .await
            .expect("听众开始观看屏幕");

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_video_track();
        let publisher_sender = publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("共享者添加视频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;

        send_until_publisher_video_arrives(&publisher_track, &mut media_events, "publisher-1")
            .await;
        let publisher_video_ssrc = media
            .session_snapshot(room_id, "publisher-1")
            .await
            .expect("共享者媒体会话存在")
            .tracks
            .into_iter()
            .find(|track| track.kind == "screen_video")
            .expect("共享者视频 track 存在")
            .ssrc;
        publisher_track
            .write_rtp_with_extensions(&test_video_rtp_packet(900), &[])
            .await
            .expect("发布待转发视频 RTP");
        let received = timeout(Duration::from_secs(2), packet_receiver.recv())
            .await
            .expect("等待听众视频 RTP 未超时")
            .expect("听众收到视频 RTP");

        listener
            .write_rtcp(&[Box::new(TransportLayerNack {
                sender_ssrc: 1,
                media_ssrc: received.ssrc,
                nacks: vec![NackPair::new(received.sequence_number)],
            })])
            .await
            .expect("听众发送 NACK");

        assert!(
            publisher_receives_nack_for_ssrc(&publisher_sender, publisher_video_ssrc).await,
            "听众对下行视频的 NACK 应改写 SSRC 后转发给共享者"
        );

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 共享者视频_track_不会转发给未观看的听众() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();
        media
            .set_screen_share_owner(room_id, Some("publisher-1"))
            .await
            .expect("设置屏幕共享者");

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Video, None)
            .await
            .expect("听众声明视频 transceiver");
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_video_track();
        publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("共享者添加视频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;

        for sequence_number in 0..10 {
            publisher_track
                .write_rtp_with_extensions(&test_video_rtp_packet(sequence_number), &[])
                .await
                .expect("发布测试屏幕共享 RTP");
            sleep(Duration::from_millis(10)).await;
        }
        assert!(
            timeout(Duration::from_millis(200), media_events.recv())
                .await
                .is_err(),
            "没有观看者时不应产生重新协商媒体事件"
        );
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众媒体会话存在")
                .outbound_video_track_count,
            0
        );

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 两个摄像头发布者会占用听众的独立下行槽位() {
        let media = MediaController::new().expect("创建媒体控制器");
        let room_id = "room-1";

        for member_id in ["publisher-1", "publisher-2", "listener-1"] {
            media
                .handle_offer(room_id, member_id, create_audio_video_offer(3).await)
                .await
                .expect("建立带视频槽位的媒体会话");
        }
        for publisher_member_id in ["publisher-1", "publisher-2"] {
            media
                .store_test_video_inbound_track(
                    room_id,
                    publisher_member_id,
                    super::MediaTrackKind::CameraVideo,
                )
                .await
                .expect("登记测试摄像头上行");
            media
                .set_video_call_publisher(room_id, publisher_member_id, true)
                .await
                .expect("开启摄像头发布状态");
        }

        let listener = media
            .session_snapshot(room_id, "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(listener.outbound_video_track_count, 2);
        assert!(
            listener
                .outbound_tracks
                .iter()
                .any(|track| track.publisher_member_id == "publisher-1"
                    && track.kind == "camera_video")
        );
        assert!(
            listener
                .outbound_tracks
                .iter()
                .any(|track| track.publisher_member_id == "publisher-2"
                    && track.kind == "camera_video")
        );
    }

    #[tokio::test]
    async fn 屏幕共享重复观看不会累积_rtcp_feedback_任务() {
        let media = MediaController::new().expect("创建媒体控制器");
        let room_id = "room-1";

        for member_id in ["publisher-1", "listener-1"] {
            media
                .handle_offer(room_id, member_id, create_audio_video_offer(1).await)
                .await
                .expect("建立带视频槽位的媒体会话");
        }
        media
            .store_test_video_inbound_track(
                room_id,
                "publisher-1",
                super::MediaTrackKind::ScreenShareVideo,
            )
            .await
            .expect("登记测试屏幕共享上行");
        media
            .set_screen_share_owner(room_id, Some("publisher-1"))
            .await
            .expect("开启屏幕共享发布状态");

        media
            .set_screen_viewing(room_id, "listener-1", true)
            .await
            .expect("听众开始观看屏幕");
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众会话存在")
                .video_feedback_task_count,
            1
        );

        media
            .set_screen_viewing(room_id, "listener-1", true)
            .await
            .expect("重复观看屏幕保持幂等");
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众会话存在")
                .video_feedback_task_count,
            1
        );

        media
            .set_screen_viewing(room_id, "listener-1", false)
            .await
            .expect("停止观看屏幕");
        let listener = media
            .session_snapshot(room_id, "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(listener.outbound_video_track_count, 0);
        assert_eq!(listener.video_feedback_task_count, 0);
    }

    #[tokio::test]
    async fn 摄像头重复发布不会累积_rtcp_feedback_任务() {
        let media = MediaController::new().expect("创建媒体控制器");
        let room_id = "room-1";

        for member_id in ["publisher-1", "listener-1"] {
            media
                .handle_offer(room_id, member_id, create_audio_video_offer(1).await)
                .await
                .expect("建立带视频槽位的媒体会话");
        }
        media
            .store_test_video_inbound_track(
                room_id,
                "publisher-1",
                super::MediaTrackKind::CameraVideo,
            )
            .await
            .expect("登记测试摄像头上行");

        media
            .set_video_call_publisher(room_id, "publisher-1", true)
            .await
            .expect("开启摄像头发布状态");
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众会话存在")
                .video_feedback_task_count,
            1
        );

        media
            .set_video_call_publisher(room_id, "publisher-1", true)
            .await
            .expect("重复发布摄像头保持幂等");
        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众会话存在")
                .video_feedback_task_count,
            1
        );

        media
            .set_video_call_publisher(room_id, "publisher-1", false)
            .await
            .expect("停止摄像头发布");
        let listener = media
            .session_snapshot(room_id, "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(listener.outbound_video_track_count, 0);
        assert_eq!(listener.video_feedback_task_count, 0);
    }

    #[tokio::test]
    async fn 停止屏幕共享不会移除摄像头下行槽位() {
        let media = MediaController::new().expect("创建媒体控制器");
        let room_id = "room-1";

        for member_id in ["publisher-1", "listener-1"] {
            media
                .handle_offer(room_id, member_id, create_audio_video_offer(2).await)
                .await
                .expect("建立带视频槽位的媒体会话");
        }
        media
            .set_screen_viewing(room_id, "listener-1", true)
            .await
            .expect("听众开始观看屏幕");
        media
            .store_test_video_inbound_track(
                room_id,
                "publisher-1",
                super::MediaTrackKind::ScreenShareVideo,
            )
            .await
            .expect("登记测试屏幕共享上行");
        media
            .set_screen_share_owner(room_id, Some("publisher-1"))
            .await
            .expect("开启屏幕共享发布状态");
        media
            .store_test_video_inbound_track(
                room_id,
                "publisher-1",
                super::MediaTrackKind::CameraVideo,
            )
            .await
            .expect("登记测试摄像头上行");
        media
            .set_video_call_publisher(room_id, "publisher-1", true)
            .await
            .expect("开启摄像头发布状态");

        assert_eq!(
            media
                .session_snapshot(room_id, "listener-1")
                .await
                .expect("听众会话存在")
                .outbound_video_track_count,
            2
        );

        media
            .set_screen_share_owner(room_id, None)
            .await
            .expect("停止屏幕共享");
        let listener = media
            .session_snapshot(room_id, "listener-1")
            .await
            .expect("听众会话存在");
        assert_eq!(listener.outbound_video_track_count, 1);
        assert_eq!(listener.outbound_tracks[0].kind, "camera_video");
    }

    #[tokio::test]
    async fn 禁止发言成员的上行音频不会转发给听众() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .expect("听众声明音频 transceiver");
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_track();
        publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者添加音频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;
        media
            .set_member_can_speak(room_id, "publisher-1", false)
            .await
            .expect("禁止发布者发言");

        send_until_publisher_audio_arrives(&publisher_track, &mut media_events, "publisher-1")
            .await;
        negotiate_client(&media, room_id, "listener-1", &listener).await;

        for sequence_number in 1..=5 {
            publisher_track
                .write_rtp(&test_rtp_packet(sequence_number))
                .await
                .expect("发布者发送 RTP");
            sleep(Duration::from_millis(20)).await;
        }

        assert!(
            timeout(Duration::from_millis(200), packet_receiver.recv())
                .await
                .is_err(),
            "禁止发言成员的上行 RTP 不应转发给听众"
        );

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 禁止发言权限在发布者媒体会话建立前生效() {
        let test_network = new_test_network().await;
        let media = MediaController::new_with_vnet_for_test(Arc::clone(&test_network.server))
            .expect("创建媒体控制器");
        let room_id = "room-1";
        let mut media_events = media.subscribe_events();

        let listener = new_test_peer_connection(Arc::clone(&test_network.listener)).await;
        listener
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .expect("听众声明音频 transceiver");
        let mut packet_receiver = receive_first_track_packet(&listener);
        negotiate_client(&media, room_id, "listener-1", &listener).await;
        wait_for_connected(&listener).await;

        media
            .set_member_can_speak(room_id, "publisher-1", false)
            .await
            .expect("提前禁止发布者发言");

        let publisher = new_test_peer_connection(Arc::clone(&test_network.publisher)).await;
        let publisher_track = new_publisher_track();
        publisher
            .add_track(Arc::clone(&publisher_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .expect("发布者添加音频 track");
        negotiate_client(&media, room_id, "publisher-1", &publisher).await;
        wait_for_connected(&publisher).await;

        send_until_publisher_audio_arrives(&publisher_track, &mut media_events, "publisher-1")
            .await;
        negotiate_client(&media, room_id, "listener-1", &listener).await;

        for sequence_number in 1..=5 {
            publisher_track
                .write_rtp(&test_rtp_packet(sequence_number))
                .await
                .expect("发布者发送 RTP");
            sleep(Duration::from_millis(20)).await;
        }

        assert!(
            timeout(Duration::from_millis(200), packet_receiver.recv())
                .await
                .is_err(),
            "媒体会话建立前禁言也应丢弃发布者 RTP"
        );

        listener.close().await.expect("关闭听众 PeerConnection");
        publisher.close().await.expect("关闭发布者 PeerConnection");
    }

    #[tokio::test]
    async fn 恢复音频策略会缓存后续媒体会话的禁言状态() {
        let media = MediaController::new().expect("创建媒体控制器");

        media
            .sync_member_audio_policy("room-1", "publisher-1", false, &[])
            .await
            .expect("恢复禁言策略");
        media
            .handle_offer("room-1", "publisher-1", create_audio_offer().await)
            .await
            .expect("建立发布者媒体会话");

        assert!(
            !media
                .session_snapshot("room-1", "publisher-1")
                .await
                .expect("发布者会话存在")
                .can_speak
        );

        media
            .sync_member_audio_policy("room-1", "publisher-1", true, &[])
            .await
            .expect("恢复可发言策略");
        assert!(
            media
                .session_snapshot("room-1", "publisher-1")
                .await
                .expect("发布者会话存在")
                .can_speak
        );
    }

    #[tokio::test]
    async fn 无效_offer_返回_invalid_message() {
        let media = MediaController::new().expect("创建媒体控制器");

        let err = media
            .handle_offer("room-1", "member-1", "not sdp".to_string())
            .await
            .expect_err("无效 SDP 应失败");

        assert!(matches!(err, Error::InvalidMessage(_)));
    }

    #[tokio::test]
    async fn 没有_offer_时添加_ice_candidate_返回_invalid_message() {
        let media = MediaController::new().expect("创建媒体控制器");

        let err = media
            .add_ice_candidate(
                "room-1",
                "member-1",
                IceCandidate {
                    candidate: "candidate:1 1 udp 1 127.0.0.1 1 typ host".to_string(),
                    sdp_mid: Some("0".to_string()),
                    sdp_mline_index: Some(0),
                    username_fragment: None,
                },
            )
            .await
            .expect_err("没有 offer 时不能添加候选");

        assert!(matches!(err, Error::InvalidMessage(_)));
    }

    async fn create_audio_offer() -> String {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .expect("注册默认 codecs");
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .expect("注册默认 interceptors");
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();
        let peer_connection = api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("创建测试 PeerConnection");
        peer_connection
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .expect("添加 audio transceiver");
        let offer = peer_connection
            .create_offer(None)
            .await
            .expect("创建测试 offer");
        peer_connection
            .set_local_description(offer.clone())
            .await
            .expect("设置测试 local description");
        peer_connection
            .close()
            .await
            .expect("关闭测试 PeerConnection");
        offer.sdp
    }

    async fn create_audio_video_offer(video_count: usize) -> String {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .expect("注册默认 codecs");
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .expect("注册默认 interceptors");
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();
        let peer_connection = api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("创建测试 PeerConnection");
        peer_connection
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .expect("添加 audio transceiver");
        for _ in 0..video_count {
            peer_connection
                .add_transceiver_from_kind(RTPCodecType::Video, None)
                .await
                .expect("添加 video transceiver");
        }
        let offer = peer_connection
            .create_offer(None)
            .await
            .expect("创建测试 offer");
        peer_connection
            .set_local_description(offer.clone())
            .await
            .expect("设置测试 local description");
        peer_connection
            .close()
            .await
            .expect("关闭测试 PeerConnection");
        offer.sdp
    }

    struct TestNetwork {
        _router: Arc<Mutex<Router>>,
        server: Arc<Net>,
        listener: Arc<Net>,
        publisher: Arc<Net>,
        publisher_two: Arc<Net>,
    }

    async fn new_test_network() -> TestNetwork {
        let router = Arc::new(Mutex::new(
            Router::new(RouterConfig {
                cidr: "1.2.3.0/24".to_string(),
                ..Default::default()
            })
            .expect("创建测试 vnet router"),
        ));
        let server = attach_test_net(&router, "1.2.3.10").await;
        let listener = attach_test_net(&router, "1.2.3.11").await;
        let publisher = attach_test_net(&router, "1.2.3.12").await;
        let publisher_two = attach_test_net(&router, "1.2.3.13").await;

        router.lock().await.start().await.expect("启动测试 vnet");

        TestNetwork {
            _router: router,
            server,
            listener,
            publisher,
            publisher_two,
        }
    }

    async fn attach_test_net(router: &Arc<Mutex<Router>>, ip: &str) -> Arc<Net> {
        let vnet = Arc::new(Net::new(Some(NetConfig {
            static_ips: vec![ip.to_string()],
            ..Default::default()
        })));
        let nic = vnet.get_nic().expect("读取测试 vnet nic");

        router
            .lock()
            .await
            .add_net(Arc::clone(&nic))
            .await
            .expect("向测试 router 添加 nic");
        nic.lock()
            .await
            .set_router(Arc::clone(router))
            .await
            .expect("测试 nic 绑定 router");
        vnet
    }

    async fn new_test_peer_connection(vnet: Arc<Net>) -> Arc<RTCPeerConnection> {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .expect("注册默认 codecs");
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .expect("注册默认 interceptors");
        let mut setting_engine = SettingEngine::default();
        setting_engine.set_vnet(Some(vnet));
        let api = APIBuilder::new()
            .with_setting_engine(setting_engine)
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .expect("创建客户端 PeerConnection"),
        )
    }

    fn new_publisher_track() -> Arc<TrackLocalStaticRTP> {
        Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                clock_rate: 48000,
                channels: 2,
                ..Default::default()
            },
            "publisher-audio".to_string(),
            "publisher-stream".to_string(),
        ))
    }

    fn new_publisher_video_track() -> Arc<TrackLocalStaticRTP> {
        Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP8.to_string(),
                clock_rate: 90000,
                ..Default::default()
            },
            "publisher-screen".to_string(),
            "publisher-screen-stream".to_string(),
        ))
    }

    #[derive(Debug)]
    struct ReceivedTrackPacket {
        ssrc: u32,
        sequence_number: u16,
        payload: Vec<u8>,
    }

    fn receive_first_track_packet(peer_connection: &RTCPeerConnection) -> mpsc::Receiver<Vec<u8>> {
        let mut packet_info_receiver = receive_first_track_packet_info(peer_connection);
        let (packet_sender, packet_receiver) = mpsc::channel(8);
        tokio::spawn(async move {
            while let Some(packet) = packet_info_receiver.recv().await {
                if packet_sender.send(packet.payload).await.is_err() {
                    break;
                }
            }
        });
        packet_receiver
    }

    fn receive_first_track_packet_info(
        peer_connection: &RTCPeerConnection,
    ) -> mpsc::Receiver<ReceivedTrackPacket> {
        let (packet_sender, packet_receiver) = mpsc::channel(8);
        peer_connection.on_track(Box::new(move |track, _, _| {
            let packet_sender = packet_sender.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    while let Ok((packet, _)) = track.read_rtp().await {
                        let packet = ReceivedTrackPacket {
                            ssrc: packet.header.ssrc,
                            sequence_number: packet.header.sequence_number,
                            payload: packet.payload.to_vec(),
                        };
                        if packet_sender.send(packet).await.is_err() {
                            break;
                        }
                    }
                });
            })
        }));
        packet_receiver
    }

    async fn negotiate_client(
        media: &MediaController,
        room_id: &str,
        member_id: &str,
        peer_connection: &RTCPeerConnection,
    ) {
        let (client_candidate_sender, mut client_candidates) = mpsc::channel(16);
        peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let client_candidate_sender = client_candidate_sender.clone();
            Box::pin(async move {
                let Some(candidate) = candidate else {
                    return;
                };
                let Ok(candidate) = candidate.to_json() else {
                    return;
                };

                let _ = client_candidate_sender.try_send(IceCandidate::from(candidate));
            })
        }));
        let offer = peer_connection
            .create_offer(None)
            .await
            .expect("客户端创建 offer");
        let mut gathering_complete = peer_connection.gathering_complete_promise().await;
        peer_connection
            .set_local_description(offer)
            .await
            .expect("客户端设置 local offer");
        timeout(Duration::from_secs(2), gathering_complete.recv())
            .await
            .expect("客户端 ICE gathering 完成");
        let offer_sdp = peer_connection
            .local_description()
            .await
            .expect("客户端 local description 存在")
            .sdp;

        let mut answer = media
            .handle_offer(room_id, member_id, offer_sdp)
            .await
            .expect("后端处理 offer");
        let answer_has_candidates = answer.sdp.contains("a=candidate:");
        let answer_description =
            RTCSessionDescription::answer(answer.sdp).expect("构造后端 SDP answer");
        peer_connection
            .set_remote_description(answer_description)
            .await
            .expect("客户端设置后端 answer");

        trickle_candidates_until_connected(
            media,
            room_id,
            member_id,
            peer_connection,
            &mut client_candidates,
            &mut answer.local_ice_candidates,
            answer_has_candidates,
        )
        .await;
    }

    async fn trickle_candidates_until_connected(
        media: &MediaController,
        room_id: &str,
        member_id: &str,
        peer_connection: &RTCPeerConnection,
        client_candidates: &mut mpsc::Receiver<IceCandidate>,
        server_candidates: &mut mpsc::Receiver<IceCandidate>,
        answer_has_candidates: bool,
    ) {
        timeout(Duration::from_secs(3), async {
            let mut saw_server_candidate = answer_has_candidates;

            loop {
                if saw_server_candidate
                    && peer_connection.connection_state() == RTCPeerConnectionState::Connected
                {
                    return;
                }

                tokio::select! {
                    Some(candidate) = client_candidates.recv() => {
                        media
                            .add_ice_candidate(room_id, member_id, candidate)
                            .await
                            .expect("后端添加客户端 ICE candidate");
                    }
                    Some(candidate) = server_candidates.recv() => {
                        saw_server_candidate = true;
                        peer_connection
                            .add_ice_candidate(
                                webrtc::ice_transport::ice_candidate::RTCIceCandidateInit::from(candidate),
                            )
                            .await
                            .expect("客户端添加服务端 ICE candidate");
                    }
                    _ = sleep(Duration::from_millis(10)) => {}
                }
            }
        })
        .await
        .expect("客户端和后端 ICE 连通");
    }

    async fn wait_for_connected(peer_connection: &RTCPeerConnection) {
        timeout(Duration::from_secs(3), async {
            while peer_connection.connection_state() != RTCPeerConnectionState::Connected {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("客户端 PeerConnection 连接成功");
    }

    async fn send_until_publisher_audio_arrives(
        publisher_track: &TrackLocalStaticRTP,
        media_events: &mut broadcast::Receiver<super::MediaEvent>,
        publisher_member_id: &str,
    ) {
        let _ =
            send_until_publisher_audio_event(publisher_track, media_events, publisher_member_id)
                .await;
    }

    async fn send_until_publisher_audio_event(
        publisher_track: &TrackLocalStaticRTP,
        media_events: &mut broadcast::Receiver<super::MediaEvent>,
        publisher_member_id: &str,
    ) -> super::MediaEvent {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 0..200 {
                publisher_track
                    .write_rtp_with_extensions(&test_rtp_packet(sequence_number), &[])
                    .await
                    .expect("发布测试上行 RTP");

                while let Ok(Ok(event)) =
                    timeout(Duration::from_millis(20), media_events.recv()).await
                {
                    if matches!(
                        event,
                        super::MediaEvent::InboundAudioTrack { ref member_id, .. }
                            if member_id == publisher_member_id
                    ) {
                        return event;
                    }
                }

                sleep(Duration::from_millis(5)).await;
            }

            panic!("后端未收到发布者上行音频");
        })
        .await
        .expect("等待发布者上行音频未超时")
    }

    async fn send_until_publisher_audio_registered(
        media: &MediaController,
        room_id: &str,
        publisher_member_id: &str,
        publisher_track: &TrackLocalStaticRTP,
    ) {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 0..200 {
                publisher_track
                    .write_rtp_with_extensions(&test_rtp_packet(sequence_number), &[])
                    .await
                    .expect("发布测试上行 RTP");
                if media
                    .session_snapshot(room_id, publisher_member_id)
                    .await
                    .is_some_and(|snapshot| snapshot.audio_track_count > 0)
                {
                    return;
                }
                sleep(Duration::from_millis(5)).await;
            }

            panic!("后端未登记发布者上行音频");
        })
        .await
        .expect("等待发布者上行音频登记未超时");
    }

    async fn send_until_publisher_video_arrives(
        publisher_track: &TrackLocalStaticRTP,
        media_events: &mut broadcast::Receiver<super::MediaEvent>,
        publisher_member_id: &str,
    ) {
        let _ =
            send_until_publisher_video_event(publisher_track, media_events, publisher_member_id)
                .await;
    }

    async fn send_until_publisher_video_event(
        publisher_track: &TrackLocalStaticRTP,
        media_events: &mut broadcast::Receiver<super::MediaEvent>,
        publisher_member_id: &str,
    ) -> super::MediaEvent {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 0..200 {
                publisher_track
                    .write_rtp_with_extensions(&test_video_rtp_packet(sequence_number), &[])
                    .await
                    .expect("发布测试上行视频 RTP");

                while let Ok(Ok(event)) =
                    timeout(Duration::from_millis(20), media_events.recv()).await
                {
                    if matches!(
                        event,
                        super::MediaEvent::InboundScreenVideoTrack { ref member_id, .. }
                            if member_id == publisher_member_id
                    ) {
                        return event;
                    }
                }

                sleep(Duration::from_millis(5)).await;
            }

            panic!("后端未收到共享者上行视频");
        })
        .await
        .expect("等待共享者上行视频未超时")
    }

    async fn send_until_listener_receives(
        publisher_track: &TrackLocalStaticRTP,
        packet_receiver: &mut mpsc::Receiver<Vec<u8>>,
    ) -> Option<Vec<u8>> {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 200..500 {
                publisher_track
                    .write_rtp_with_extensions(&test_rtp_packet(sequence_number), &[])
                    .await
                    .expect("发布待转发 RTP");

                if let Ok(Some(payload)) =
                    timeout(Duration::from_millis(20), packet_receiver.recv()).await
                {
                    return Some(payload);
                }

                sleep(Duration::from_millis(5)).await;
            }

            None
        })
        .await
        .expect("等待听众 RTP 未超时")
    }

    async fn send_until_listener_receives_payload(
        publisher_track: &TrackLocalStaticRTP,
        packet_receiver: &mut mpsc::Receiver<Vec<u8>>,
        expected_payload: Vec<u8>,
    ) -> Option<Vec<u8>> {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 500..800 {
                publisher_track
                    .write_rtp_with_extensions(
                        &test_rtp_packet_with_payload(sequence_number, expected_payload.clone()),
                        &[],
                    )
                    .await
                    .expect("发布待转发 RTP");

                if let Ok(Some(payload)) =
                    timeout(Duration::from_millis(20), packet_receiver.recv()).await
                {
                    if payload == expected_payload {
                        return Some(payload);
                    }
                }

                sleep(Duration::from_millis(5)).await;
            }

            None
        })
        .await
        .expect("等待指定听众 RTP 未超时")
    }

    async fn send_until_listener_receives_video(
        publisher_track: &TrackLocalStaticRTP,
        packet_receiver: &mut mpsc::Receiver<Vec<u8>>,
    ) -> Option<Vec<u8>> {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 800..1100 {
                publisher_track
                    .write_rtp_with_extensions(&test_video_rtp_packet(sequence_number), &[])
                    .await
                    .expect("发布待转发视频 RTP");

                if let Ok(Some(payload)) =
                    timeout(Duration::from_millis(20), packet_receiver.recv()).await
                {
                    if payload == vec![0xC7] {
                        return Some(payload);
                    }
                }

                sleep(Duration::from_millis(5)).await;
            }

            None
        })
        .await
        .expect("等待听众视频 RTP 未超时")
    }

    async fn publisher_receives_pli_count(
        publisher_sender: &RTCRtpSender,
        media_ssrc: u32,
        expected_count: usize,
    ) -> usize {
        timeout(Duration::from_secs(2), async {
            let mut pli_count = 0;
            loop {
                let Ok((packets, _)) = publisher_sender.read_rtcp().await else {
                    return pli_count;
                };

                for packet in packets {
                    if packet
                        .as_any()
                        .downcast_ref::<PictureLossIndication>()
                        .is_some_and(|pli| pli.media_ssrc == media_ssrc)
                    {
                        pli_count += 1;
                        if pli_count >= expected_count {
                            return pli_count;
                        }
                    }
                }
            }
        })
        .await
        .unwrap_or(0)
    }

    async fn publisher_receives_nack_for_ssrc(
        publisher_sender: &RTCRtpSender,
        media_ssrc: u32,
    ) -> bool {
        timeout(Duration::from_secs(2), async {
            loop {
                let Ok((packets, _)) = publisher_sender.read_rtcp().await else {
                    return false;
                };

                for packet in packets {
                    if packet
                        .as_any()
                        .downcast_ref::<TransportLayerNack>()
                        .is_some_and(|nack| nack.media_ssrc == media_ssrc)
                    {
                        return true;
                    }
                }
            }
        })
        .await
        .unwrap_or(false)
    }

    fn test_rtp_packet(sequence_number: u16) -> webrtc::rtp::packet::Packet {
        test_rtp_packet_with_payload(sequence_number, vec![0xA5])
    }

    fn test_video_rtp_packet(sequence_number: u16) -> webrtc::rtp::packet::Packet {
        webrtc::rtp::packet::Packet {
            header: webrtc::rtp::header::Header {
                version: 2,
                payload_type: 96,
                sequence_number,
                timestamp: u32::from(sequence_number) * 3000,
                marker: true,
                ..Default::default()
            },
            payload: vec![0xC7].into(),
        }
    }

    fn test_rtp_packet_with_payload(
        sequence_number: u16,
        payload: Vec<u8>,
    ) -> webrtc::rtp::packet::Packet {
        webrtc::rtp::packet::Packet {
            header: webrtc::rtp::header::Header {
                version: 2,
                payload_type: 111,
                sequence_number,
                timestamp: u32::from(sequence_number) * 960,
                ..Default::default()
            },
            payload: payload.into(),
        }
    }
}
