use crate::{
    Result,
    auth::CurrentUser,
    domain::room::{RoomJoin, RoomStore},
    service::authenticated_room::{AuthenticatedRoomService, PersistentJoinDecision},
};
use std::sync::Arc;

/// 房间生命周期服务封装创建、加入、恢复和回滚加入等跨持久房间规则。
#[derive(Clone)]
pub struct RoomLifecycleService {
    rooms: Arc<RoomStore>,
    authenticated_rooms: AuthenticatedRoomService,
}

impl RoomLifecycleService {
    /// 创建房间生命周期服务；现阶段保留 RoomStore facade，逐步抽离 transport 编排。
    pub fn new(rooms: Arc<RoomStore>, authenticated_rooms: AuthenticatedRoomService) -> Self {
        Self {
            rooms,
            authenticated_rooms,
        }
    }

    /// 创建运行时房间，并在认证开启时写入持久房间归属。
    pub fn create_room(&self, nickname: String, user: Option<&CurrentUser>) -> Result<RoomJoin> {
        let join = match user {
            Some(user) => self.rooms.create_room_for_user(nickname, user.id)?,
            None => self.rooms.create_room(nickname)?,
        };
        if let Some(user) = user {
            if let Err(error) = self
                .authenticated_rooms
                .create_for_owner(&join.room.id, user)
            {
                let _ = self.rooms.leave_room(&join.room.id, &join.member.id);
                return Err(error);
            }
        }
        Ok(join)
    }

    /// 按认证持久房间规则加入房间，必要时恢复缺失的运行时房间。
    pub fn join_room(
        &self,
        room_id: &str,
        nickname: String,
        user: Option<&CurrentUser>,
    ) -> Result<RoomJoin> {
        let role = match self.authenticated_rooms.join_decision(room_id, user)? {
            PersistentJoinDecision::NotPersistent => None,
            PersistentJoinDecision::JoinAs(role) => Some(role),
        };
        let join = match role {
            Some(role) => {
                let Some(user) = user else {
                    return Err(crate::Error::Unauthenticated);
                };
                match self.rooms.join_room_with_role_for_user(
                    room_id,
                    nickname.clone(),
                    role.clone(),
                    user.id,
                ) {
                    Ok(join) => join,
                    Err(crate::Error::RoomNotFound) => self
                        .rooms
                        .restore_room_with_member_for_user(room_id, nickname, role, user.id)?,
                    Err(error) => return Err(error),
                }
            }
            None => match user {
                Some(user) => self.rooms.join_room_for_user(room_id, nickname, user.id)?,
                None => self.rooms.join_room(room_id, nickname)?,
            },
        };
        self.authenticated_rooms.touch_if_persistent(room_id)?;
        Ok(join)
    }

    /// 使用恢复凭据恢复成员，持久房间只刷新活跃时间不改变成员身份。
    pub fn resume_room(
        &self,
        room_id: &str,
        member_id: &str,
        resume_token: &str,
        user: Option<&CurrentUser>,
    ) -> Result<RoomJoin> {
        let join = self.rooms.resume_room_for_user(
            room_id,
            member_id,
            resume_token,
            user.map(|user| user.id),
        )?;
        // 恢复成员已经把运行时状态标为在线；持久房间活跃时间刷新失败不能阻断信令注册，
        // 否则会留下“在线但无 WebSocket”的成员，破坏断线恢复兼容性。
        let _ = self.authenticated_rooms.touch_if_persistent(room_id);
        Ok(join)
    }

    /// 回滚注册信令队列失败后的成员加入状态，保持运行时房间不残留半加入成员。
    pub fn rollback_join_after_register_failure(&self, room_id: &str, member_id: &str) {
        if let Ok(room) = self.rooms.leave_room(room_id, member_id) {
            if room.members.is_empty() {
                let _ = self.rooms.close_room(room_id);
            }
        }
    }

    /// 关闭指定运行时房间，供管理接口关闭持久房间后同步释放内存状态。
    pub fn close_room(&self, room_id: &str) -> Result<crate::domain::room::Room> {
        self.rooms.close_room(room_id)
    }

    /// 读取当前房间聊天历史；加入响应暂时由生命周期服务统一准备。
    pub fn chat_history(&self, room_id: &str) -> Vec<crate::domain::room::ChatMessage> {
        self.rooms.chat_history(room_id).unwrap_or_default()
    }

    /// 读取成员加入响应所需的不听名单，保持 joined_room 载荷兼容。
    pub fn not_listening_member_ids(&self, join: &RoomJoin) -> Vec<String> {
        join.member.not_listening_member_ids()
    }
}
