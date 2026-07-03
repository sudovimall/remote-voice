use crate::{
    Error, Result,
    domain::room::{MemberListeningState, Room, RoomStore},
    media::MediaController,
};
use std::sync::Arc;

/// 成员控制服务同步房间状态和 SFU 媒体策略，降低 WebSocket 分支复杂度。
#[derive(Clone)]
pub struct MemberControlService {
    rooms: Arc<RoomStore>,
    media: Arc<MediaController>,
}

/// 成员状态变化结果，告知信令层是否还要广播停止发言事件。
#[derive(Debug, Clone, PartialEq)]
pub struct MemberUpdatedOutcome {
    pub room: Room,
    pub member_id: String,
    pub force_speaking_false: bool,
}

/// 成员收听偏好变化结果，携带只返回给当前听众的不听名单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberListeningOutcome {
    pub request_id: Option<String>,
    pub state: MemberListeningState,
}

impl MemberControlService {
    /// 创建成员控制服务，集中处理房间状态和 SFU 媒体策略的同步。
    pub fn new(rooms: Arc<RoomStore>, media: Arc<MediaController>) -> Self {
        Self { rooms, media }
    }

    /// 更新自己的静音状态；静音后需要广播发言状态为 false。
    pub fn set_self_muted(
        &self,
        room_id: &str,
        member_id: &str,
        self_muted: bool,
    ) -> Result<MemberUpdatedOutcome> {
        let room = self.rooms.set_self_muted(room_id, member_id, self_muted)?;
        Ok(MemberUpdatedOutcome {
            room,
            member_id: member_id.to_string(),
            force_speaking_false: self_muted,
        })
    }

    /// 房主修改成员发言权限，并同步给媒体层决定是否转发上行音频。
    pub async fn set_member_can_speak(
        &self,
        room_id: &str,
        actor_member_id: &str,
        target_member_id: &str,
        can_speak: bool,
    ) -> Result<MemberUpdatedOutcome> {
        let room = self.rooms.set_member_can_speak(
            room_id,
            actor_member_id,
            target_member_id,
            can_speak,
        )?;
        // 旧信令路径对媒体层同步失败保持容忍，避免房主权限更新被 SFU 瞬时状态阻断。
        let _ = self
            .media
            .set_member_can_speak(room_id, target_member_id, can_speak)
            .await;
        Ok(MemberUpdatedOutcome {
            room,
            member_id: target_member_id.to_string(),
            force_speaking_false: !can_speak,
        })
    }

    /// 更新听众对某发布者的收听偏好，并同步 SFU 下行策略。
    pub async fn set_member_listening(
        &self,
        room_id: &str,
        listener_member_id: &str,
        publisher_member_id: &str,
        listening: bool,
        request_id: Option<String>,
    ) -> Result<MemberListeningOutcome> {
        let state = self.rooms.set_member_listening(
            room_id,
            listener_member_id,
            publisher_member_id,
            listening,
        )?;
        self.media
            .set_member_listening(room_id, listener_member_id, publisher_member_id, listening)
            .await?;
        Ok(MemberListeningOutcome { request_id, state })
    }

    /// 根据房间快照恢复所有成员音频策略，断线恢复后补回被 close_member 清理的媒体缓存。
    pub async fn sync_room_media_policies(&self, room: &Room) -> Result<()> {
        for member in room.members.values() {
            let not_listening_member_ids = member
                .not_listening_member_ids()
                .into_iter()
                .filter(|publisher_member_id| room.members.contains_key(publisher_member_id))
                .collect::<Vec<_>>();
            self.media
                .sync_member_audio_policy(
                    &room.id,
                    &member.id,
                    member.can_speak,
                    &not_listening_member_ids,
                )
                .await?;
        }

        Ok(())
    }

    /// 根据成员权限和静音状态归一化发言广播，防止客户端伪造正在说话。
    pub fn normalized_speaking(&self, room_id: &str, member_id: &str, speaking: bool) -> bool {
        self.rooms
            .get_room(room_id)
            .ok()
            .and_then(|room| room.members.get(member_id).cloned())
            .is_some_and(|member| speaking && member.can_speak && !member.self_muted)
    }

    /// 校验成员延迟上报值，保证广播给前端的毫秒数可比较且非负。
    pub fn validate_latency(&self, server_ms: f64) -> Result<f64> {
        if !server_ms.is_finite() || server_ms < 0.0 {
            return Err(Error::InvalidMessage(
                "成员延迟必须是非负毫秒数".to_string(),
            ));
        }
        Ok(server_ms)
    }
}
