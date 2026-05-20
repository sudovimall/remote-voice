use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::{Mutex, broadcast, mpsc};
use webrtc::{
    api::{
        API, APIBuilder, interceptor_registry::register_default_interceptors,
        media_engine::MediaEngine,
    },
    ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTPCodecType,
    track::track_remote::TrackRemote,
};

type SessionKey = (String, String);
type SessionMap = HashMap<SessionKey, MediaSession>;
const LOCAL_ICE_QUEUE_CAPACITY: usize = 64;
const MEDIA_EVENT_QUEUE_CAPACITY: usize = 256;

pub struct MediaController {
    api: API,
    // 每个成员只维护一条到后端的 PeerConnection；上行轨道也挂在同一个会话里。
    sessions: Arc<Mutex<SessionMap>>,
    event_sender: broadcast::Sender<MediaEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvent {
    InboundAudioTrack { room_id: String, member_id: String },
}

struct MediaSession {
    peer_connection: Arc<RTCPeerConnection>,
    inbound_tracks: HashMap<usize, InboundTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboundTrack {
    id: String,
    stream_id: String,
    ssrc: u32,
    mime_type: String,
    packet_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSessionSnapshot {
    pub inbound_track_count: usize,
    pub audio_track_count: usize,
    pub inbound_packet_count: u64,
    pub tracks: Vec<InboundTrackSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundTrackSnapshot {
    pub id: String,
    pub stream_id: String,
    pub ssrc: u32,
    pub mime_type: String,
    pub packet_count: u64,
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
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|err| Error::Internal(format!("注册默认 codecs 失败: {err}")))?;
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .map_err(|err| Error::Internal(format!("注册默认 interceptors 失败: {err}")))?;
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        Ok(Self {
            api,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            event_sender: broadcast::channel(MEDIA_EVENT_QUEUE_CAPACITY).0,
        })
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
        let peer_connection = self
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|err| Error::Internal(format!("创建 PeerConnection 失败: {err}")))?;
        let peer_connection = Arc::new(peer_connection);
        let (local_ice_sender, local_ice_candidates) =
            mpsc::channel::<IceCandidate>(LOCAL_ICE_QUEUE_CAPACITY);

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

                let inbound_track = InboundTrack::from_remote_track(&track);
                let sessions_for_reader = Arc::clone(&sessions);
                let mut session_map = sessions.lock().await;
                let Some(session) = session_map.get_mut(&session_key) else {
                    return;
                };

                if Arc::ptr_eq(&session.peer_connection, &track_peer_connection) {
                    let track_id = track.tid();
                    session.inbound_tracks.insert(track_id, inbound_track);
                    tokio::spawn(read_inbound_rtp(
                        Arc::clone(&track),
                        sessions_for_reader,
                        session_key.clone(),
                        track_peer_connection,
                        track_id,
                    ));
                    let _ = event_sender.send(MediaEvent::InboundAudioTrack {
                        room_id: session_key.0,
                        member_id: session_key.1,
                    });
                }
            })
        }));

        let key = (room_id.to_string(), member_id.to_string());
        let previous = {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(
                key.clone(),
                MediaSession {
                    peer_connection: Arc::clone(&peer_connection),
                    inbound_tracks: HashMap::new(),
                },
            )
        };

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

impl MediaSession {
    fn snapshot(&self) -> MediaSessionSnapshot {
        let tracks = self
            .inbound_tracks
            .values()
            .map(InboundTrack::snapshot)
            .collect::<Vec<_>>();

        MediaSessionSnapshot {
            inbound_track_count: tracks.len(),
            audio_track_count: tracks.len(),
            inbound_packet_count: tracks.iter().map(|track| track.packet_count).sum(),
            tracks,
        }
    }
}

impl InboundTrack {
    fn from_remote_track(track: &TrackRemote) -> Self {
        Self {
            id: track.id(),
            stream_id: track.stream_id(),
            ssrc: track.ssrc(),
            mime_type: track.codec().capability.mime_type,
            packet_count: 0,
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
    sessions: Arc<Mutex<SessionMap>>,
    session_key: SessionKey,
    peer_connection: Arc<RTCPeerConnection>,
    track_id: usize,
) {
    loop {
        if track.read_rtp().await.is_err() {
            break;
        }

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
    }
}

#[cfg(test)]
mod tests {
    use super::{IceCandidate, MediaController};
    use crate::Error;
    use webrtc::{
        api::{
            APIBuilder, interceptor_registry::register_default_interceptors,
            media_engine::MediaEngine,
        },
        interceptor::registry::Registry,
        peer_connection::configuration::RTCConfiguration,
        rtp_transceiver::rtp_codec::RTPCodecType,
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

        media
            .close_member("room-1", "member-1")
            .await
            .expect("关闭媒体会话");

        assert!(media.session_snapshot("room-1", "member-1").await.is_none());
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
}
