pub mod authenticated_room;
pub mod chat;
pub mod media_route;
pub mod member_control;
pub mod room_lifecycle;

use crate::{auth::AuthRuntime, domain::room::RoomStore, media::MediaController};
use authenticated_room::AuthenticatedRoomService;
use chat::ChatService;
use media_route::MediaRouteService;
use member_control::MemberControlService;
use room_lifecycle::RoomLifecycleService;
use std::sync::Arc;

/// 聚合应用服务层入口，迁移期间与现有领域、媒体和信令组件并行存在。
#[derive(Clone)]
pub struct Services {
    pub authenticated_rooms: AuthenticatedRoomService,
    pub chat: ChatService,
    pub media_routes: MediaRouteService,
    pub member_controls: MemberControlService,
    pub room_lifecycle: RoomLifecycleService,
}

impl Services {
    /// 组装应用服务层；各服务共享现有领域和媒体组件，保持迁移期间的行为一致。
    pub fn new(rooms: Arc<RoomStore>, media: Arc<MediaController>, auth: AuthRuntime) -> Self {
        let authenticated_rooms = AuthenticatedRoomService::new(auth);
        Self {
            chat: ChatService::new(Arc::clone(&rooms)),
            media_routes: MediaRouteService::new(Arc::clone(&rooms), Arc::clone(&media)),
            member_controls: MemberControlService::new(Arc::clone(&rooms), Arc::clone(&media)),
            room_lifecycle: RoomLifecycleService::new(rooms, authenticated_rooms.clone()),
            authenticated_rooms,
        }
    }
}
