use crate::{Error, Result};
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::Mutex;
use webrtc::{
    api::{
        API, APIBuilder, interceptor_registry::register_default_interceptors,
        media_engine::MediaEngine,
    },
    ice_transport::ice_candidate::RTCIceCandidateInit,
    interceptor::registry::Registry,
    peer_connection::{
        RTCPeerConnection, configuration::RTCConfiguration,
        sdp::session_description::RTCSessionDescription,
    },
};

type SessionKey = (String, String);

pub struct MediaController {
    api: API,
    peer_connections: Mutex<HashMap<SessionKey, Arc<RTCPeerConnection>>>,
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
            peer_connections: Mutex::new(HashMap::new()),
        })
    }

    pub async fn handle_offer(
        &self,
        room_id: &str,
        member_id: &str,
        sdp: String,
    ) -> Result<String> {
        let offer = RTCSessionDescription::offer(sdp)
            .map_err(|err| Error::InvalidMessage(format!("无效 SDP offer: {err}")))?;
        let peer_connection = self
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .map_err(|err| Error::Internal(format!("创建 PeerConnection 失败: {err}")))?;

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
        let peer_connection = Arc::new(peer_connection);

        let key = (room_id.to_string(), member_id.to_string());
        let previous = {
            let mut peer_connections = self.peer_connections.lock().await;
            peer_connections.insert(key, Arc::clone(&peer_connection))
        };
        if let Some(previous) = previous {
            let _ = previous.close().await;
        }

        Ok(answer.sdp)
    }

    pub async fn add_ice_candidate(
        &self,
        room_id: &str,
        member_id: &str,
        candidate: String,
    ) -> Result<()> {
        let key = (room_id.to_string(), member_id.to_string());
        let peer_connection = {
            let peer_connections = self.peer_connections.lock().await;
            peer_connections.get(&key).cloned()
        }
        .ok_or_else(|| Error::InvalidMessage("媒体会话不存在，请先发送 offer".to_string()))?;

        peer_connection
            .add_ice_candidate(RTCIceCandidateInit {
                candidate,
                ..Default::default()
            })
            .await
            .map_err(|err| Error::Internal(format!("添加 ICE candidate 失败: {err}")))
    }

    pub async fn close_member(&self, room_id: &str, member_id: &str) -> Result<()> {
        let key = (room_id.to_string(), member_id.to_string());
        let peer_connection = {
            let mut peer_connections = self.peer_connections.lock().await;
            peer_connections.remove(&key)
        };

        if let Some(peer_connection) = peer_connection {
            peer_connection
                .close()
                .await
                .map_err(|err| Error::Internal(format!("关闭 PeerConnection 失败: {err}")))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MediaController;
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

        assert!(answer.contains("m=audio"));
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
                "candidate:1 1 udp 1 127.0.0.1 1 typ host".to_string(),
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
