use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::{Mutex, broadcast, mpsc};
use webrtc::{
    api::{
        API, APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_OPUS, MediaEngine},
    },
    ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        sdp::session_description::RTCSessionDescription,
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

pub struct MediaController {
    api: API,
    // 每个成员只维护一条到后端的 PeerConnection；上行轨道也挂在同一个会话里。
    sessions: Arc<Mutex<SessionMap>>,
    // 房间权限可能先于媒体 offer 到达，先按成员记住，建会话时再带入 RTP 转发路径。
    member_can_speak: Arc<Mutex<HashMap<SessionKey, bool>>>,
    event_sender: broadcast::Sender<MediaEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvent {
    InboundAudioTrack { room_id: String, member_id: String },
}

struct MediaSession {
    peer_connection: Arc<RTCPeerConnection>,
    downlink_sender: Arc<RTCRtpSender>,
    can_speak: bool,
    inbound_tracks: HashMap<usize, InboundTrack>,
    outbound_tracks: HashMap<String, OutboundTrack>,
}

#[derive(Debug, Clone)]
struct InboundTrack {
    id: String,
    stream_id: String,
    ssrc: u32,
    mime_type: String,
    packet_count: u64,
    fanout_track: Arc<TrackLocalStaticRTP>,
}

#[derive(Debug, Clone)]
struct OutboundTrack {
    publisher_member_id: String,
    track_id: String,
    fanout_track: Arc<TrackLocalStaticRTP>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSessionSnapshot {
    pub inbound_track_count: usize,
    pub audio_track_count: usize,
    pub inbound_packet_count: u64,
    pub outbound_track_count: usize,
    pub tracks: Vec<InboundTrackSnapshot>,
    pub outbound_tracks: Vec<OutboundTrackSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundTrackSnapshot {
    pub id: String,
    pub stream_id: String,
    pub ssrc: u32,
    pub mime_type: String,
    pub packet_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundTrackSnapshot {
    pub publisher_member_id: String,
    pub track_id: String,
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
        Self::with_api_builder(APIBuilder::new())
    }

    fn with_api_builder(api_builder: APIBuilder) -> Result<Self> {
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
            sessions: Arc::new(Mutex::new(HashMap::new())),
            member_can_speak: Arc::new(Mutex::new(HashMap::new())),
            event_sender: broadcast::channel(MEDIA_EVENT_QUEUE_CAPACITY).0,
        })
    }

    #[cfg(test)]
    fn new_with_vnet_for_test(vnet: Arc<webrtc::util::vnet::net::Net>) -> Result<Self> {
        let mut setting_engine = webrtc::api::setting_engine::SettingEngine::default();
        setting_engine.set_vnet(Some(vnet));
        Self::with_api_builder(APIBuilder::new().with_setting_engine(setting_engine))
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
            // 重新协商要沿用已建立的 ICE/DTLS 会话；替换 PeerConnection 会让客户端仍连着旧会话。
            forward_local_ice_candidates(&peer_connection, local_ice_sender);
            let answer = create_answer(&peer_connection, offer).await?;
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
        let downlink_sender = add_downlink_slot(&peer_connection, room_id, member_id).await?;

        let sessions = Arc::clone(&self.sessions);
        let session_key = (room_id.to_string(), member_id.to_string());
        let event_sender = self.event_sender.clone();
        let track_peer_connection = Arc::clone(&peer_connection);
        // 收到上行 TrackRemote 后先登记元数据；RTP 转发会在下一阶段消费这些 track。
        peer_connection.on_track(Box::new(move |track, _, _| {
            let sessions = Arc::clone(&sessions);
            let session_key = session_key.clone();
            let event_sender = event_sender.clone();
            let track_peer_connection = Arc::clone(&track_peer_connection);

            Box::pin(async move {
                if track.kind() != RTPCodecType::Audio {
                    return;
                }

                let inbound_track =
                    InboundTrack::from_remote_track(&track, &session_key.0, &session_key.1);
                let fanout_track = Arc::clone(&inbound_track.fanout_track);
                let outbound_track_id = format!("{}:{}", session_key.1, inbound_track.id);
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
                    ));

                    if attach_audio_to_subscribers(
                        Arc::clone(&sessions),
                        &session_key.0,
                        &session_key.1,
                        outbound_track_id,
                        fanout_track,
                    )
                    .await
                    .is_ok()
                    {
                        let _ = event_sender.send(MediaEvent::InboundAudioTrack {
                            room_id: session_key.0,
                            member_id: session_key.1,
                        });
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
                    downlink_sender,
                    can_speak,
                    inbound_tracks: HashMap::new(),
                    outbound_tracks: HashMap::new(),
                },
            )
        };

        if let Some(previous) = &previous {
            for outbound_track in previous.outbound_tracks.values() {
                peer_connection
                    .add_track(Arc::clone(&outbound_track.fanout_track)
                        as Arc<dyn TrackLocal + Send + Sync>)
                    .await
                    .map_err(|err| Error::Internal(format!("恢复下行音频 track 失败: {err}")))?;
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
        attach_existing_audio_to_subscriber(Arc::clone(&self.sessions), room_id, member_id).await?;

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
        let sessions = self.sessions.lock().await;
        sessions.get(&key).map(MediaSession::snapshot)
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

        attach_audio_to_subscribers(
            Arc::clone(&self.sessions),
            room_id,
            publisher_member_id,
            track_id,
            fanout_track,
        )
        .await
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

async fn add_downlink_slot(
    peer_connection: &RTCPeerConnection,
    room_id: &str,
    member_id: &str,
) -> Result<Arc<RTCRtpSender>> {
    // 当前客户端 offer 流程由客户端发起；先协商一个音频 sender 槽位，
    // 后续同房间发布者出现时才可以不新增 m-line 直接替换真实 track。
    let downlink_slot = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_OPUS.to_string(),
            clock_rate: 48000,
            channels: 2,
            ..Default::default()
        },
        format!("{member_id}:downlink"),
        format!("room-{room_id}"),
    ));

    peer_connection
        .add_track(downlink_slot as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .map_err(|err| Error::Internal(format!("创建下行音频槽位失败: {err}")))
}

impl MediaSession {
    fn snapshot(&self) -> MediaSessionSnapshot {
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
            inbound_track_count: tracks.len(),
            audio_track_count: tracks.len(),
            inbound_packet_count: tracks.iter().map(|track| track.packet_count).sum(),
            outbound_track_count: outbound_tracks.len(),
            tracks,
            outbound_tracks,
        }
    }
}

impl OutboundTrack {
    fn snapshot(&self) -> OutboundTrackSnapshot {
        OutboundTrackSnapshot {
            publisher_member_id: self.publisher_member_id.clone(),
            track_id: self.track_id.clone(),
        }
    }
}

impl InboundTrack {
    fn from_remote_track(track: &TrackRemote, room_id: &str, member_id: &str) -> Self {
        Self {
            id: track.id(),
            stream_id: track.stream_id(),
            ssrc: track.ssrc(),
            mime_type: track.codec().capability.mime_type,
            packet_count: 0,
            fanout_track: Arc::new(TrackLocalStaticRTP::new(
                track.codec().capability,
                format!("{member_id}:{}", track.id()),
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
            packet_count: self.packet_count,
        }
    }
}

async fn read_inbound_rtp(
    track: Arc<TrackRemote>,
    fanout_track: Arc<TrackLocalStaticRTP>,
    sessions: Arc<Mutex<SessionMap>>,
    session_key: SessionKey,
    peer_connection: Arc<RTCPeerConnection>,
    track_id: usize,
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

            // 服务端在 RTP 边界执行房间发言权限，避免被禁言客户端继续推音频。
            session.can_speak
        };

        if should_forward {
            let _ = fanout_track.write_rtp_with_extensions(&packet, &[]).await;
        }
    }
}

async fn attach_audio_to_subscribers(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    publisher_member_id: &str,
    track_id: String,
    fanout_track: Arc<TrackLocalStaticRTP>,
) -> Result<()> {
    let subscribers = {
        let sessions = sessions.lock().await;
        sessions
            .iter()
            .filter_map(|((session_room_id, member_id), session)| {
                if session_room_id == room_id && member_id != publisher_member_id {
                    Some((
                        (session_room_id.clone(), member_id.clone()),
                        Arc::clone(&session.downlink_sender),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    for (subscriber_key, downlink_sender) in subscribers {
        downlink_sender
            .replace_track(Some(
                Arc::clone(&fanout_track) as Arc<dyn TrackLocal + Send + Sync>
            ))
            .await
            .map_err(|err| Error::Internal(format!("替换下行音频槽位失败: {err}")))?;

        let mut sessions = sessions.lock().await;
        if let Some(session) = sessions.get_mut(&subscriber_key) {
            session.outbound_tracks.clear();
            session.outbound_tracks.insert(
                track_id.clone(),
                OutboundTrack {
                    publisher_member_id: publisher_member_id.to_string(),
                    track_id: track_id.clone(),
                    fanout_track: Arc::clone(&fanout_track),
                },
            );
        }
    }

    Ok(())
}

async fn attach_existing_audio_to_subscriber(
    sessions: Arc<Mutex<SessionMap>>,
    room_id: &str,
    member_id: &str,
) -> Result<()> {
    let subscriber_key = (room_id.to_string(), member_id.to_string());
    let existing_audio = {
        let sessions = sessions.lock().await;
        let Some(subscriber) = sessions.get(&subscriber_key) else {
            return Ok(());
        };

        // 当前每个听众只有一个预协商下行槽位，晚加入时先接入房间里已存在的一路音频。
        sessions
            .iter()
            .find_map(|((session_room_id, publisher_member_id), session)| {
                if session_room_id != room_id || publisher_member_id == member_id {
                    return None;
                }

                session.inbound_tracks.values().next().map(|track| {
                    (
                        Arc::clone(&subscriber.downlink_sender),
                        publisher_member_id.clone(),
                        format!("{}:{}", publisher_member_id, track.id),
                        Arc::clone(&track.fanout_track),
                    )
                })
            })
    };

    let Some((downlink_sender, publisher_member_id, track_id, fanout_track)) = existing_audio
    else {
        return Ok(());
    };

    downlink_sender
        .replace_track(Some(
            Arc::clone(&fanout_track) as Arc<dyn TrackLocal + Send + Sync>
        ))
        .await
        .map_err(|err| Error::Internal(format!("晚加入听众接入下行音频失败: {err}")))?;

    let mut sessions = sessions.lock().await;
    if let Some(subscriber) = sessions.get_mut(&subscriber_key) {
        subscriber.outbound_tracks.clear();
        subscriber.outbound_tracks.insert(
            track_id.clone(),
            OutboundTrack {
                publisher_member_id,
                track_id,
                fanout_track,
            },
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{IceCandidate, MediaController};
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
            media_engine::{MIME_TYPE_OPUS, MediaEngine},
            setting_engine::SettingEngine,
        },
        interceptor::registry::Registry,
        peer_connection::{
            RTCPeerConnection, configuration::RTCConfiguration,
            peer_connection_state::RTCPeerConnectionState,
            sdp::session_description::RTCSessionDescription,
        },
        rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType},
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

        send_until_publisher_audio_arrives(&publisher_track, &mut media_events).await;
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
        send_until_publisher_audio_arrives(&publisher_track, &mut media_events).await;

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

        send_until_publisher_audio_arrives(&publisher_track, &mut media_events).await;
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

        send_until_publisher_audio_arrives(&publisher_track, &mut media_events).await;
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

    struct TestNetwork {
        _router: Arc<Mutex<Router>>,
        server: Arc<Net>,
        listener: Arc<Net>,
        publisher: Arc<Net>,
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

        router.lock().await.start().await.expect("启动测试 vnet");

        TestNetwork {
            _router: router,
            server,
            listener,
            publisher,
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

    fn receive_first_track_packet(peer_connection: &RTCPeerConnection) -> mpsc::Receiver<Vec<u8>> {
        let (packet_sender, packet_receiver) = mpsc::channel(1);
        peer_connection.on_track(Box::new(move |track, _, _| {
            let packet_sender = packet_sender.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    if let Ok((packet, _)) = track.read_rtp().await {
                        let _ = packet_sender.send(packet.payload.to_vec()).await;
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
    ) {
        timeout(Duration::from_secs(3), async {
            for sequence_number in 0..200 {
                publisher_track
                    .write_rtp_with_extensions(&test_rtp_packet(sequence_number), &[])
                    .await
                    .expect("发布测试上行 RTP");

                if let Ok(Ok(event)) = timeout(Duration::from_millis(20), media_events.recv()).await
                {
                    assert!(matches!(
                        event,
                        super::MediaEvent::InboundAudioTrack { member_id, .. }
                            if member_id == "publisher-1"
                    ));
                    return;
                }

                sleep(Duration::from_millis(5)).await;
            }

            panic!("后端未收到发布者上行音频");
        })
        .await
        .expect("等待发布者上行音频未超时");
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

    fn test_rtp_packet(sequence_number: u16) -> webrtc::rtp::packet::Packet {
        webrtc::rtp::packet::Packet {
            header: webrtc::rtp::header::Header {
                version: 2,
                payload_type: 111,
                sequence_number,
                timestamp: u32::from(sequence_number) * 960,
                ..Default::default()
            },
            payload: vec![0xA5].into(),
        }
    }
}
