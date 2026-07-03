use crate::{
    Error, Result,
    domain::room::{MediaRoute, MediaRouteUpdate, RoomStore, ScreenShareState, VideoCallState},
    media::{IceCandidate, MediaAnswer, MediaController},
};
use std::sync::Arc;

/// 媒体路由服务编排房间媒体状态、P2P 路由和 SFU 控制器之间的同步。
#[derive(Clone)]
pub struct MediaRouteService {
    rooms: Arc<RoomStore>,
    media: Arc<MediaController>,
}

/// 屏幕共享服务结果，供信令层广播开始、停止和观看人数变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenShareOutcome {
    pub screen_share: Option<ScreenShareState>,
    pub stopped_member_id: Option<String>,
    pub viewer_count: usize,
}

/// 摄像头发布服务结果，描述是否需要广播开始或停止以及当前发布人数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoCallOutcome {
    pub publisher: Option<VideoCallState>,
    pub stopped: bool,
    pub publisher_count: usize,
}

/// P2P 定向信令类型，保持与 SFU offer/ICE 路径分离。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2pForwardKind {
    Offer { sdp: String },
    Answer { sdp: String },
    IceCandidate { candidate: IceCandidate },
}

/// P2P 定向转发结果，信令层根据目标成员发送对应 ServerSignal。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P2pForwardOutcome {
    pub target_member_id: String,
    pub from_member_id: String,
    pub kind: P2pForwardKind,
}

impl MediaRouteService {
    /// 创建媒体路由服务；房间状态仍由 RoomStore 负责，SFU 操作仍由 MediaController 负责。
    pub fn new(rooms: Arc<RoomStore>, media: Arc<MediaController>) -> Self {
        Self { rooms, media }
    }

    /// 开始屏幕共享并同步 SFU 屏幕共享拥有者，保持共享占位和媒体路由一致。
    pub async fn start_screen_share(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<ScreenShareOutcome> {
        let room = self.rooms.start_screen_share(room_id, member_id)?;
        if let Some(screen_share) = &room.screen_share {
            self.media
                .set_screen_share_owner(room_id, Some(&screen_share.member_id))
                .await?;
        }
        let viewer_count = self.media.screen_viewer_count(room_id).await;
        Ok(ScreenShareOutcome {
            screen_share: room.screen_share,
            stopped_member_id: None,
            viewer_count,
        })
    }

    /// 停止屏幕共享并释放 SFU 下行视频槽位，重复停止保持原有幂等行为。
    pub async fn stop_screen_share(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<ScreenShareOutcome> {
        let stopped_member_id = self
            .rooms
            .get_room(room_id)
            .ok()
            .and_then(|room| room.screen_share.map(|screen_share| screen_share.member_id));
        self.rooms.stop_screen_share(room_id, member_id)?;
        if stopped_member_id.is_some() {
            // 旧路径停止共享时忽略媒体层释放失败，保证房间状态和停止广播先落地。
            let _ = self.media.set_screen_share_owner(room_id, None).await;
        }
        Ok(ScreenShareOutcome {
            screen_share: None,
            stopped_member_id,
            viewer_count: 0,
        })
    }

    /// 更新屏幕观看状态并返回最新观看人数，信令层只负责广播结果。
    pub async fn set_screen_viewing(
        &self,
        room_id: &str,
        member_id: &str,
        viewing: bool,
    ) -> Result<usize> {
        self.media
            .set_screen_viewing(room_id, member_id, viewing)
            .await
    }

    /// 开启摄像头发布状态，并在媒体层失败时回滚房间占位。
    pub async fn start_video_call(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<VideoCallOutcome> {
        let room = self.rooms.start_video_call(room_id, member_id)?;
        let publisher = room.video_call_publishers.get(member_id).cloned();
        match self
            .media
            .set_video_call_publisher(room_id, member_id, true)
            .await
        {
            Ok(publisher_count) => Ok(VideoCallOutcome {
                publisher,
                stopped: false,
                publisher_count,
            }),
            Err(error) => {
                let _ = self.rooms.stop_video_call(room_id, member_id);
                Err(error)
            }
        }
    }

    /// 关闭摄像头发布状态，并返回是否需要广播 stopped 事件和最新发布人数。
    pub async fn stop_video_call(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<VideoCallOutcome> {
        let was_publishing = self
            .rooms
            .get_room(room_id)
            .ok()
            .is_some_and(|room| room.video_call_publishers.contains_key(member_id));
        self.rooms.stop_video_call(room_id, member_id)?;
        let publisher_count = self
            .media
            .set_video_call_publisher(room_id, member_id, false)
            .await?;
        Ok(VideoCallOutcome {
            publisher: None,
            stopped: was_publishing,
            publisher_count,
        })
    }

    /// 处理浏览器到后端 SFU 的 offer；P2P offer 不应进入这个路径。
    pub async fn handle_sfu_offer(
        &self,
        room_id: &str,
        member_id: &str,
        sdp: String,
    ) -> Result<MediaAnswer> {
        self.media.handle_offer(room_id, member_id, sdp).await
    }

    /// 添加浏览器发给后端 SFU PeerConnection 的 ICE candidate。
    pub async fn add_sfu_ice_candidate(
        &self,
        room_id: &str,
        member_id: &str,
        candidate: IceCandidate,
    ) -> Result<()> {
        self.media
            .add_ice_candidate(room_id, member_id, candidate)
            .await
    }

    /// 校验并生成 P2P 定向转发意图，发送者身份由服务端当前会话决定。
    pub fn forward_p2p_signal(
        &self,
        room_id: &str,
        sender_member_id: &str,
        target_member_id: &str,
        kind: P2pForwardKind,
    ) -> Result<P2pForwardOutcome> {
        self.rooms
            .validate_p2p_target(room_id, sender_member_id, target_member_id)
            .map_err(p2p_signal_error)?;
        if self
            .rooms
            .media_route(room_id, sender_member_id, target_member_id)
            .map_err(p2p_signal_error)?
            == MediaRoute::Sfu
        {
            return Err(Error::InvalidMessage(
                "成员对已回退 SFU，不能继续发送 P2P 信令".to_string(),
            ));
        }
        Ok(P2pForwardOutcome {
            target_member_id: target_member_id.to_string(),
            from_member_id: sender_member_id.to_string(),
            kind,
        })
    }

    /// 校验 P2P 目标成员是否可达；失败信息按信令兼容格式归一化。
    pub fn validate_p2p_target(
        &self,
        room_id: &str,
        sender_member_id: &str,
        target_member_id: &str,
    ) -> Result<()> {
        self.rooms
            .validate_p2p_target(room_id, sender_member_id, target_member_id)
            .map_err(p2p_signal_error)
    }

    /// 记录单个成员对 P2P 失败并返回规范化媒体路由更新。
    pub fn report_p2p_failure(
        &self,
        room_id: &str,
        sender_member_id: &str,
        target_member_id: &str,
    ) -> Result<MediaRouteUpdate> {
        self.rooms
            .validate_p2p_target(room_id, sender_member_id, target_member_id)
            .map_err(p2p_signal_error)?;
        self.rooms
            .mark_p2p_connection_failed(room_id, sender_member_id, target_member_id)
            .map_err(p2p_signal_error)
    }
}

/// 将 P2P 目标不存在统一包装为 invalid_message，避免信令错误泄漏跨房间成员存在性。
pub fn p2p_signal_error(error: Error) -> Error {
    match error {
        Error::MemberNotFound => {
            Error::InvalidMessage("目标成员不存在或不在当前房间，不能发送 P2P 信令".to_string())
        }
        other => other,
    }
}
