use crate::{Error, Result};
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const ROOM_ID_LENGTH: usize = 6;
const CHAT_MESSAGE_ID_LENGTH: usize = 22;
const CHAT_MESSAGE_MAX_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRole {
    Owner,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub nickname: String,
    pub role: MemberRole,
    pub can_speak: bool,
    pub self_muted: bool,
    pub connected: bool,
    #[serde(skip, default)]
    not_listening_member_ids: HashSet<String>,
    #[serde(skip)]
    resume_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberListeningState {
    pub not_listening_member_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenShareState {
    pub member_id: String,
    pub nickname: String,
}

/// 记录成员摄像头发布状态，供房间快照恢复视频宫格占位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoCallState {
    pub member_id: String,
    pub nickname: String,
}

/// 描述一对成员当前应使用的媒体路径，默认缺省值保持为 P2P。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRoute {
    P2p,
    Sfu,
}

/// 记录媒体路径切换原因，便于前端按原因决定清理 P2P 资源还是重试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRouteReason {
    P2pFailed,
}

/// 返回一次媒体路由变化的规范化成员对和新状态，避免信令层重复排序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRouteUpdate {
    pub member_ids: Vec<String>,
    pub route: MediaRoute,
    pub reason: MediaRouteReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaRouteState {
    route: MediaRoute,
    reason: MediaRouteReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MemberPairKey {
    first_member_id: String,
    second_member_id: String,
}

impl MemberPairKey {
    fn new(first_member_id: &str, second_member_id: &str) -> Result<Self> {
        if first_member_id == second_member_id {
            return Err(Error::InvalidMessage(
                "不能向自己建立 P2P 媒体路由".to_string(),
            ));
        }

        let (first_member_id, second_member_id) = if first_member_id < second_member_id {
            (first_member_id, second_member_id)
        } else {
            (second_member_id, first_member_id)
        };

        Ok(Self {
            first_member_id: first_member_id.to_string(),
            second_member_id: second_member_id.to_string(),
        })
    }

    fn member_ids(&self) -> Vec<String> {
        vec![self.first_member_id.clone(), self.second_member_id.clone()]
    }

    fn contains(&self, member_id: &str) -> bool {
        self.first_member_id == member_id || self.second_member_id == member_id
    }
}

impl Member {
    /// 返回按成员 ID 排序的不听名单，让房间快照和恢复响应保持稳定。
    pub fn not_listening_member_ids(&self) -> Vec<String> {
        sorted_member_ids(&self.not_listening_member_ids)
    }
}

/// 房间快照包含前端需要恢复的状态，聊天和媒体路由作为后端私有状态跳过序列化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub owner_member_id: String,
    pub members: HashMap<String, Member>,
    pub created_at_epoch_seconds: u64,
    pub last_active_epoch_seconds: u64,
    pub screen_share: Option<ScreenShareState>,
    #[serde(default)]
    pub video_call_publishers: HashMap<String, VideoCallState>,
    #[serde(skip, default)]
    chat_messages: Vec<ChatMessage>,
    #[serde(skip, default)]
    media_routes: HashMap<MemberPairKey, MediaRouteState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomJoin {
    pub room: Room,
    pub member: Member,
    pub resume_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomSummary {
    pub id: String,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub room_id: String,
    pub member_id: String,
    pub nickname: String,
    pub content: String,
    pub sent_at_epoch_millis: u64,
    #[serde(default)]
    pub mentions: Vec<ChatMention>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMention {
    pub member_id: String,
    pub nickname: String,
}

#[derive(Debug)]
pub struct RoomStore {
    rooms: RwLock<HashMap<String, Room>>,
    max_members: usize,
    chat_history_limit: usize,
    room_id_seed: u64,
    next_room_seq: AtomicU64,
}

impl RoomStore {
    /// 创建房间存储，集中管理房间、成员和后端私有的媒体路由状态。
    pub fn new(max_members: usize) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            max_members,
            chat_history_limit: 100,
            room_id_seed: new_room_id_seed(),
            next_room_seq: AtomicU64::new(1),
        }
    }

    /// 调整聊天历史保留条数，测试和部署可复用同一套裁剪逻辑。
    pub fn with_chat_history_limit(mut self, chat_history_limit: usize) -> Self {
        self.chat_history_limit = chat_history_limit;
        self
    }

    /// 创建新房间并把创建者设为房主，媒体路由保持空表以表示默认 P2P。
    pub fn create_room(&self, nickname: impl Into<String>) -> Result<RoomJoin> {
        let member = self.new_member(nickname, MemberRole::Owner);
        let now = now_epoch_seconds();
        let mut rooms = self.write_rooms()?;
        let room_id = loop {
            let candidate = self.next_room_id();
            if !rooms.contains_key(&candidate) {
                break candidate;
            }
        };
        let room = Room {
            id: room_id.clone(),
            owner_member_id: member.id.clone(),
            members: HashMap::from([(member.id.clone(), member.clone())]),
            created_at_epoch_seconds: now,
            last_active_epoch_seconds: now,
            screen_share: None,
            video_call_publishers: HashMap::new(),
            chat_messages: Vec::new(),
            media_routes: HashMap::new(),
        };

        rooms.insert(room_id, room.clone());

        Ok(RoomJoin {
            room,
            resume_token: member.resume_token.clone(),
            member,
        })
    }

    /// 加入已有房间，默认以普通成员身份进入并继承房间的当前状态。
    pub fn join_room(&self, room_id: &str, nickname: impl Into<String>) -> Result<RoomJoin> {
        self.join_room_with_role(room_id, nickname, MemberRole::Member)
    }

    /// 以指定身份加入房间，持久房间恢复房主身份时复用这条路径。
    pub fn join_room_with_role(
        &self,
        room_id: &str,
        nickname: impl Into<String>,
        role: MemberRole,
    ) -> Result<RoomJoin> {
        let nickname = nickname.into();
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        if room.members.len() >= self.max_members {
            return Err(Error::RoomFull);
        }

        let member = loop {
            let candidate = self.new_member(nickname.clone(), role.clone());
            if !room.members.contains_key(&candidate.id) {
                break candidate;
            }
        };

        if role == MemberRole::Owner {
            for existing in room.members.values_mut() {
                existing.role = MemberRole::Member;
            }
            room.owner_member_id = member.id.clone();
        }

        room.members.insert(member.id.clone(), member.clone());
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(RoomJoin {
            room: room.clone(),
            resume_token: member.resume_token.clone(),
            member,
        })
    }

    /// 从持久化记录恢复运行时房间，用于认证房间在内存状态缺失时重新上线。
    pub fn restore_room_with_member(
        &self,
        room_id: &str,
        nickname: impl Into<String>,
        role: MemberRole,
    ) -> Result<RoomJoin> {
        let mut rooms = self.write_rooms()?;
        if rooms.contains_key(room_id) {
            return Err(Error::InvalidMessage("房间已经在运行中".to_string()));
        }

        let member = self.new_member(nickname, role.clone());
        let now = now_epoch_seconds();
        let owner_member_id = if role == MemberRole::Owner {
            member.id.clone()
        } else {
            String::new()
        };
        let room = Room {
            id: room_id.to_string(),
            owner_member_id,
            members: HashMap::from([(member.id.clone(), member.clone())]),
            created_at_epoch_seconds: now,
            last_active_epoch_seconds: now,
            screen_share: None,
            video_call_publishers: HashMap::new(),
            chat_messages: Vec::new(),
            media_routes: HashMap::new(),
        };

        rooms.insert(room_id.to_string(), room.clone());

        Ok(RoomJoin {
            room,
            resume_token: member.resume_token.clone(),
            member,
        })
    }

    /// 使用恢复凭据把断线成员恢复为在线状态，保留成员偏好和媒体路由状态。
    pub fn resume_room(
        &self,
        room_id: &str,
        member_id: &str,
        resume_token: &str,
    ) -> Result<RoomJoin> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room
            .members
            .get_mut(member_id)
            .ok_or(Error::MemberNotFound)?;

        if member.resume_token != resume_token {
            return Err(Error::InvalidResumeToken);
        }

        member.connected = true;
        let member = member.clone();
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(RoomJoin {
            room: room.clone(),
            resume_token: member.resume_token.clone(),
            member,
        })
    }

    /// 读取房间快照；私有媒体路由不会序列化给前端，避免改变现有快照协议。
    pub fn get_room(&self, room_id: &str) -> Result<Room> {
        let rooms = self.read_rooms()?;
        rooms.get(room_id).cloned().ok_or(Error::RoomNotFound)
    }

    /// 列出房间摘要，按房间 ID 排序让管理界面和测试输出稳定。
    pub fn list_room_summaries(&self) -> Result<Vec<RoomSummary>> {
        let rooms = self.read_rooms()?;
        let mut summaries = rooms
            .values()
            .map(|room| RoomSummary {
                id: room.id.clone(),
                member_count: room.members.len(),
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(summaries)
    }

    /// 房主修改成员发言权限；权限判断留在领域层，避免信令入口各自实现。
    pub fn set_member_can_speak(
        &self,
        room_id: &str,
        actor_member_id: &str,
        target_member_id: &str,
        can_speak: bool,
    ) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        // 权限判断集中在领域层，HTTP 和信令层只传入操作者身份。
        if room.owner_member_id != actor_member_id {
            return Err(Error::NotRoomOwner);
        }

        let member = room
            .members
            .get_mut(target_member_id)
            .ok_or(Error::MemberNotFound)?;
        member.can_speak = can_speak;
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 成员更新自己的静音状态，供房间广播和媒体层共同使用同一状态。
    pub fn set_self_muted(&self, room_id: &str, member_id: &str, self_muted: bool) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room
            .members
            .get_mut(member_id)
            .ok_or(Error::MemberNotFound)?;

        member.self_muted = self_muted;
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 读取成员的私有收听偏好，只返回当前成员自己的不听名单。
    pub fn member_listening_state(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<MemberListeningState> {
        let rooms = self.read_rooms()?;
        let room = rooms.get(room_id).ok_or(Error::RoomNotFound)?;
        let member = room.members.get(member_id).ok_or(Error::MemberNotFound)?;

        Ok(MemberListeningState {
            not_listening_member_ids: member.not_listening_member_ids(),
        })
    }

    /// 更新成员对另一发布者的收听偏好，并拒绝屏蔽自己这种无效状态。
    pub fn set_member_listening(
        &self,
        room_id: &str,
        listener_member_id: &str,
        publisher_member_id: &str,
        listening: bool,
    ) -> Result<MemberListeningState> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        if listener_member_id == publisher_member_id {
            return Err(Error::InvalidMessage("不能屏蔽自己的语音".to_string()));
        }

        if !room.members.contains_key(publisher_member_id) {
            return Err(Error::MemberNotFound);
        }

        let listener = room
            .members
            .get_mut(listener_member_id)
            .ok_or(Error::MemberNotFound)?;
        if listening {
            listener
                .not_listening_member_ids
                .remove(publisher_member_id);
        } else {
            listener
                .not_listening_member_ids
                .insert(publisher_member_id.to_string());
        }
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(MemberListeningState {
            not_listening_member_ids: listener.not_listening_member_ids(),
        })
    }

    /// 保存聊天消息并校验 mention 成员，防止客户端伪造昵称或引用未知成员。
    pub fn send_chat_message(
        &self,
        room_id: &str,
        member_id: &str,
        content: &str,
        mentions: Vec<ChatMention>,
    ) -> Result<ChatMessage> {
        let content = content.trim();
        if content.is_empty() {
            return Err(Error::InvalidMessage("聊天消息不能为空".to_string()));
        }
        if content.chars().count() > CHAT_MESSAGE_MAX_CHARS {
            return Err(Error::InvalidMessage(format!(
                "聊天消息不能超过 {CHAT_MESSAGE_MAX_CHARS} 个字符"
            )));
        }

        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room.members.get(member_id).ok_or(Error::MemberNotFound)?;
        let mut seen_mentions = HashSet::new();
        let mut checked_mentions = Vec::new();
        for mention in mentions {
            if mention.member_id == member_id || !seen_mentions.insert(mention.member_id.clone()) {
                continue;
            }
            let mentioned_member = room
                .members
                .get(&mention.member_id)
                .ok_or(Error::MemberNotFound)?;
            checked_mentions.push(ChatMention {
                member_id: mentioned_member.id.clone(),
                nickname: mentioned_member.nickname.clone(),
            });
        }
        let message = ChatMessage {
            id: new_chat_message_id(),
            room_id: room_id.to_string(),
            member_id: member_id.to_string(),
            nickname: member.nickname.clone(),
            content: content.to_string(),
            sent_at_epoch_millis: now_epoch_millis(),
            mentions: checked_mentions,
        };

        room.chat_messages.push(message.clone());
        if room.chat_messages.len() > self.chat_history_limit {
            let overflow = room.chat_messages.len() - self.chat_history_limit;
            room.chat_messages.drain(0..overflow);
        }
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(message)
    }

    /// 返回房间聊天历史，加入房间时用于补齐最近消息。
    pub fn chat_history(&self, room_id: &str) -> Result<Vec<ChatMessage>> {
        let rooms = self.read_rooms()?;
        let room = rooms.get(room_id).ok_or(Error::RoomNotFound)?;
        Ok(room.chat_messages.clone())
    }

    /// 开启屏幕共享占位，先在领域层防止多个成员同时占用共享源。
    pub fn start_screen_share(&self, room_id: &str, member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room.members.get(member_id).ok_or(Error::MemberNotFound)?;

        if !member.connected {
            return Err(Error::InvalidMessage("离线成员不能共享屏幕".to_string()));
        }

        if let Some(screen_share) = &room.screen_share {
            if screen_share.member_id != member_id {
                return Err(Error::InvalidMessage(
                    "当前已有成员正在共享屏幕。".to_string(),
                ));
            }
            room.last_active_epoch_seconds = now_epoch_seconds();
            return Ok(room.clone());
        }

        room.screen_share = Some(ScreenShareState {
            member_id: member.id.clone(),
            nickname: member.nickname.clone(),
        });
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 停止屏幕共享；共享者本人或房主可以释放占位，重复停止保持幂等。
    pub fn stop_screen_share(&self, room_id: &str, requester_member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        if !room.members.contains_key(requester_member_id) {
            return Err(Error::MemberNotFound);
        }

        let can_stop = match &room.screen_share {
            Some(screen_share) => {
                screen_share.member_id == requester_member_id
                    || room.owner_member_id == requester_member_id
            }
            None => true,
        };
        if !can_stop {
            return Err(Error::NotRoomOwner);
        }

        room.screen_share = None;
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 开启成员摄像头发布状态；先占用房间状态可以避免本地拿到权限后被服务端拒绝。
    pub fn start_video_call(&self, room_id: &str, member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room.members.get(member_id).ok_or(Error::MemberNotFound)?;

        if !member.connected {
            return Err(Error::InvalidMessage(
                "离线成员不能开启摄像头。".to_string(),
            ));
        }

        room.video_call_publishers
            .entry(member_id.to_string())
            .or_insert_with(|| VideoCallState {
                member_id: member.id.clone(),
                nickname: member.nickname.clone(),
            });
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 关闭成员摄像头发布状态；重复关闭保持幂等，便于权限拒绝和协商失败时释放占位。
    pub fn stop_video_call(&self, room_id: &str, member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        if !room.members.contains_key(member_id) {
            return Err(Error::MemberNotFound);
        }

        room.video_call_publishers.remove(member_id);
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 标记成员为可恢复断线；此时保留 P2P 路由，等待成员在宽限期内恢复。
    pub fn mark_member_disconnected(&self, room_id: &str, member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room
            .members
            .get_mut(member_id)
            .ok_or(Error::MemberNotFound)?;

        member.connected = false;
        clear_screen_share_for_member(room, member_id);
        clear_video_call_for_member(room, member_id);
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 清理超过恢复宽限期的断线成员，并移除该成员相关的私有媒体路由。
    pub fn expire_disconnected_member(
        &self,
        room_id: &str,
        member_id: &str,
    ) -> Result<Option<Room>> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room.members.get(member_id).ok_or(Error::MemberNotFound)?;

        if member.connected {
            return Ok(None);
        }

        if room.owner_member_id == member_id {
            let room = room.clone();
            rooms.remove(room_id);
            return Ok(Some(room));
        }

        room.members.remove(member_id);
        remove_listening_references(room, member_id);
        clear_screen_share_for_member(room, member_id);
        clear_video_call_for_member(room, member_id);
        clear_media_routes_for_member(room, member_id);
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(Some(room.clone()))
    }

    /// 成员显式离开房间；普通成员离开时清理其偏好和媒体路由，房主离开则关房。
    pub fn leave_room(&self, room_id: &str, member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        if !room.members.contains_key(member_id) {
            return Err(Error::MemberNotFound);
        }

        if room.owner_member_id == member_id {
            let room = room.clone();
            rooms.remove(room_id);
            return Ok(room);
        }

        room.members.remove(member_id);
        remove_listening_references(room, member_id);
        clear_screen_share_for_member(room, member_id);
        clear_video_call_for_member(room, member_id);
        clear_media_routes_for_member(room, member_id);
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    /// 关闭房间并移除全部后端私有状态，包括该房间的媒体路由表。
    pub fn close_room(&self, room_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        rooms.remove(room_id).ok_or(Error::RoomNotFound)
    }

    /// 校验 P2P 信令目标必须是同房间在线成员，避免客户端跨房间或自连。
    pub fn validate_p2p_target(
        &self,
        room_id: &str,
        sender_member_id: &str,
        target_member_id: &str,
    ) -> Result<()> {
        let rooms = self.read_rooms()?;
        let room = rooms.get(room_id).ok_or(Error::RoomNotFound)?;
        validate_distinct_members(room, sender_member_id, target_member_id)?;
        let target = room
            .members
            .get(target_member_id)
            .ok_or(Error::MemberNotFound)?;
        if !target.connected {
            return Err(Error::InvalidMessage(
                "目标成员已离线，不能发送 P2P 信令".to_string(),
            ));
        }

        Ok(())
    }

    /// 读取成员对当前媒体路由；没有显式记录时返回默认 P2P。
    pub fn media_route(
        &self,
        room_id: &str,
        first_member_id: &str,
        second_member_id: &str,
    ) -> Result<MediaRoute> {
        let key = MemberPairKey::new(first_member_id, second_member_id)?;
        let rooms = self.read_rooms()?;
        let room = rooms.get(room_id).ok_or(Error::RoomNotFound)?;

        Ok(room
            .media_routes
            .get(&key)
            .map(|state| state.route)
            .unwrap_or(MediaRoute::P2p))
    }

    /// 将某一对成员标记为 P2P 失败并回退 SFU，只影响这个规范化成员对。
    pub fn mark_p2p_connection_failed(
        &self,
        room_id: &str,
        first_member_id: &str,
        second_member_id: &str,
    ) -> Result<MediaRouteUpdate> {
        let key = MemberPairKey::new(first_member_id, second_member_id)?;
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        validate_distinct_members(room, first_member_id, second_member_id)?;

        let route = MediaRoute::Sfu;
        let reason = MediaRouteReason::P2pFailed;
        room.media_routes
            .insert(key.clone(), MediaRouteState { route, reason });
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(MediaRouteUpdate {
            member_ids: key.member_ids(),
            route,
            reason,
        })
    }

    fn new_member(&self, nickname: impl Into<String>, role: MemberRole) -> Member {
        Member {
            id: new_member_id(),
            nickname: nickname.into(),
            role,
            can_speak: true,
            self_muted: false,
            connected: true,
            not_listening_member_ids: HashSet::new(),
            resume_token: new_resume_token(),
        }
    }

    fn next_room_id(&self) -> String {
        let seq = self.next_room_seq.fetch_add(1, Ordering::Relaxed);
        let space = base36_space(ROOM_ID_LENGTH);
        let sequential = seq % space;
        let mut mixed = mix64(seq ^ self.room_id_seed) % space;

        if mixed == sequential {
            mixed = (mixed + (space / 2)) % space;
        }

        to_base36_fixed(mixed, ROOM_ID_LENGTH)
    }

    fn read_rooms(&self) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, Room>>> {
        self.rooms
            .read()
            .map_err(|_| Error::Internal("房间读锁已损坏".to_string()))
    }

    fn write_rooms(&self) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, Room>>> {
        self.rooms
            .write()
            .map_err(|_| Error::Internal("房间写锁已损坏".to_string()))
    }
}

fn remove_listening_references(room: &mut Room, member_id: &str) {
    for member in room.members.values_mut() {
        member.not_listening_member_ids.remove(member_id);
    }
}

fn clear_screen_share_for_member(room: &mut Room, member_id: &str) {
    if room
        .screen_share
        .as_ref()
        .is_some_and(|screen_share| screen_share.member_id == member_id)
    {
        room.screen_share = None;
    }
}

fn clear_video_call_for_member(room: &mut Room, member_id: &str) {
    room.video_call_publishers.remove(member_id);
}

fn clear_media_routes_for_member(room: &mut Room, member_id: &str) {
    room.media_routes
        .retain(|pair, _state| !pair.contains(member_id));
}

fn validate_distinct_members(
    room: &Room,
    first_member_id: &str,
    second_member_id: &str,
) -> Result<()> {
    if first_member_id == second_member_id {
        return Err(Error::InvalidMessage("不能向自己发送 P2P 信令".to_string()));
    }
    if !room.members.contains_key(first_member_id) || !room.members.contains_key(second_member_id) {
        return Err(Error::MemberNotFound);
    }

    Ok(())
}

fn sorted_member_ids(member_ids: &HashSet<String>) -> Vec<String> {
    let mut member_ids = member_ids.iter().cloned().collect::<Vec<_>>();
    member_ids.sort();
    member_ids
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn now_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn new_room_id_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default()
}

fn new_member_id() -> String {
    let mut rng = rand::rng();
    // member_id 会作为当前 MVP 的临时连接凭据，不能再使用 m1/m2 这类可猜序列。
    let suffix: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(22)
        .map(char::from)
        .collect();

    format!("m_{suffix}")
}

fn new_resume_token() -> String {
    let mut rng = rand::rng();
    let suffix: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();

    format!("r_{suffix}")
}

fn new_chat_message_id() -> String {
    let mut rng = rand::rng();
    let suffix: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(CHAT_MESSAGE_ID_LENGTH)
        .map(char::from)
        .collect();

    format!("c_{suffix}")
}

fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn to_base36_fixed(mut value: u64, width: usize) -> String {
    const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    value %= base36_space(width);
    let mut chars = Vec::new();

    while value > 0 {
        let index = (value % 36) as usize;
        chars.push(DIGITS[index] as char);
        value /= 36;
    }

    while chars.len() < width {
        chars.push('0');
    }

    chars.iter().rev().collect()
}

fn base36_space(width: usize) -> u64 {
    36_u64.pow(width as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined_member_id(join: &RoomJoin) -> String {
        join.member.id.clone()
    }

    #[test]
    fn 聊天消息保存服务端校验后的_mentions() {
        let store = RoomStore::new(8);
        let owner = store.create_room("房主").expect("创建房间");
        let room_id = owner.room.id.clone();
        let owner_id = joined_member_id(&owner);
        let member = store.join_room(&room_id, "阿木").expect("加入房间");
        let member_id = joined_member_id(&member);

        let message = store
            .send_chat_message(
                &room_id,
                &owner_id,
                "@阿木 晚上打哪张图？",
                vec![ChatMention {
                    member_id: member_id.clone(),
                    nickname: "伪造昵称".to_string(),
                }],
            )
            .expect("发送聊天消息");

        assert_eq!(
            message.mentions,
            vec![ChatMention {
                member_id,
                nickname: "阿木".to_string(),
            }]
        );
    }

    #[test]
    fn 聊天消息_mentions_兼容空列表并过滤自己和重复成员() {
        let store = RoomStore::new(8);
        let owner = store.create_room("房主").expect("创建房间");
        let room_id = owner.room.id.clone();
        let owner_id = joined_member_id(&owner);
        let member = store.join_room(&room_id, "队友").expect("加入房间");
        let member_id = joined_member_id(&member);

        let message = store
            .send_chat_message(
                &room_id,
                &owner_id,
                " @队友 @房主 ",
                vec![
                    ChatMention {
                        member_id: owner_id.clone(),
                        nickname: "房主".to_string(),
                    },
                    ChatMention {
                        member_id: member_id.clone(),
                        nickname: "队友".to_string(),
                    },
                    ChatMention {
                        member_id: member_id.clone(),
                        nickname: "队友".to_string(),
                    },
                ],
            )
            .expect("发送聊天消息");

        assert_eq!(message.content, "@队友 @房主");
        assert_eq!(
            message.mentions,
            vec![ChatMention {
                member_id,
                nickname: "队友".to_string(),
            }]
        );
    }

    #[test]
    fn 聊天消息_mentions_拒绝未知成员() {
        let store = RoomStore::new(8);
        let owner = store.create_room("房主").expect("创建房间");
        let room_id = owner.room.id.clone();
        let owner_id = joined_member_id(&owner);

        let error = store
            .send_chat_message(
                &room_id,
                &owner_id,
                "@不存在 晚上打哪张图？",
                vec![ChatMention {
                    member_id: "m_missing".to_string(),
                    nickname: "不存在".to_string(),
                }],
            )
            .expect_err("未知成员不能被 mention");

        assert!(matches!(error, Error::MemberNotFound));
    }
}
