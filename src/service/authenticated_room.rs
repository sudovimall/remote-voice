use crate::{
    Error, Result,
    auth::{AuthRuntime, CurrentUser, model::PersistentRoomView, session::now_epoch_seconds},
    domain::room::MemberRole,
};

/// 封装认证模式下的持久房间规则，避免 HTTP 和 WebSocket 层直接访问 SQLite。
#[derive(Clone)]
pub struct AuthenticatedRoomService {
    auth: AuthRuntime,
}

/// 描述持久房间加入决策，运行时房间据此决定成员身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistentJoinDecision {
    NotPersistent,
    JoinAs(MemberRole),
}

impl AuthenticatedRoomService {
    /// 创建认证房间服务，集中管理持久房间归属和关闭规则，避免 transport 直接访问存储。
    pub fn new(auth: AuthRuntime) -> Self {
        Self { auth }
    }

    /// 认证是否启用；HTTP 房间列表需要据此决定是否合并持久房间。
    pub fn is_enabled(&self) -> bool {
        self.auth.is_enabled()
    }

    /// 为认证创建者写入持久房间记录；未启用认证时保持无副作用。
    pub fn create_for_owner(&self, room_id: &str, owner: &CurrentUser) -> Result<()> {
        let Some(service) = self.auth.service() else {
            return Ok(());
        };
        service
            .store()
            .create_persistent_room(room_id, owner.id, now_epoch_seconds())?;
        Ok(())
    }

    /// 判断认证用户加入持久房间时应获得的运行时角色，并拒绝已关闭房间。
    pub fn join_decision(
        &self,
        room_id: &str,
        user: Option<&CurrentUser>,
    ) -> Result<PersistentJoinDecision> {
        let Some(user) = user else {
            return Ok(PersistentJoinDecision::NotPersistent);
        };
        let Some(service) = self.auth.service() else {
            return Ok(PersistentJoinDecision::NotPersistent);
        };
        let Some(persistent) = service.store().find_persistent_room(room_id)? else {
            return Ok(PersistentJoinDecision::NotPersistent);
        };
        if persistent.closed_at_epoch_seconds.is_some() {
            return Err(Error::RoomClosed);
        }

        if persistent.owner_user_id == user.id {
            Ok(PersistentJoinDecision::JoinAs(MemberRole::Owner))
        } else {
            Ok(PersistentJoinDecision::JoinAs(MemberRole::Member))
        }
    }

    /// 加入或恢复持久房间后刷新活跃时间；非持久房间保持无副作用。
    pub fn touch_if_persistent(&self, room_id: &str) -> Result<()> {
        let Some(service) = self.auth.service() else {
            return Ok(());
        };
        if service.store().find_persistent_room(room_id)?.is_some() {
            service
                .store()
                .touch_persistent_room(room_id, now_epoch_seconds())?;
        }
        Ok(())
    }

    /// 列出所有未关闭持久房间；调用方需先完成认证，普通大厅不要求管理员身份。
    pub fn list_open(&self) -> Result<Vec<PersistentRoomView>> {
        let service = self.auth_service()?;
        service
            .store()
            .list_open_persistent_rooms()
            .map(|rooms| rooms.into_iter().map(PersistentRoomView::from).collect())
    }

    /// 管理员列出所有未关闭持久房间，供管理界面复用并保留权限边界。
    pub fn list_open_for_admin(&self, actor: &CurrentUser) -> Result<Vec<PersistentRoomView>> {
        self.auth_service()?.require_admin(actor)?;
        self.list_open()
    }

    /// 管理员关闭持久房间记录；存储层当前对缺失或已关闭记录保持幂等。
    pub fn close_as_admin(&self, actor: &CurrentUser, room_id: &str) -> Result<()> {
        let service = self.auth_service()?;
        service.require_admin(actor)?;
        service
            .store()
            .close_persistent_room(room_id, now_epoch_seconds())
    }

    /// 房主离开或过期时只关闭自己拥有的持久房间，避免普通成员误关房间。
    pub fn close_as_owner_if_owned(
        &self,
        room_id: &str,
        actor: Option<&CurrentUser>,
    ) -> Result<bool> {
        let Some(service) = self.auth.service() else {
            return Ok(false);
        };
        let Some(actor) = actor else {
            return Ok(false);
        };
        let Some(persistent) = service.store().find_persistent_room(room_id)? else {
            return Ok(false);
        };
        if persistent.owner_user_id != actor.id {
            return Ok(false);
        }

        service
            .store()
            .close_persistent_room(room_id, now_epoch_seconds())?;
        Ok(true)
    }

    fn auth_service(&self) -> Result<&std::sync::Arc<crate::auth::service::AuthService>> {
        self.auth.service().ok_or(Error::AuthDisabled)
    }
}
