use crate::{Error, Result};
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const ROOM_ID_LENGTH: usize = 6;

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
    #[serde(skip)]
    resume_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub owner_member_id: String,
    pub members: HashMap<String, Member>,
    pub created_at_epoch_seconds: u64,
    pub last_active_epoch_seconds: u64,
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

#[derive(Debug)]
pub struct RoomStore {
    rooms: RwLock<HashMap<String, Room>>,
    max_members: usize,
    room_id_seed: u64,
    next_room_seq: AtomicU64,
}

impl RoomStore {
    pub fn new(max_members: usize) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            max_members,
            room_id_seed: new_room_id_seed(),
            next_room_seq: AtomicU64::new(1),
        }
    }

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
        };

        rooms.insert(room_id, room.clone());

        Ok(RoomJoin {
            room,
            resume_token: member.resume_token.clone(),
            member,
        })
    }

    pub fn join_room(&self, room_id: &str, nickname: impl Into<String>) -> Result<RoomJoin> {
        let nickname = nickname.into();
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;

        if room.members.len() >= self.max_members {
            return Err(Error::RoomFull);
        }

        let member = loop {
            let candidate = self.new_member(nickname.clone(), MemberRole::Member);
            if !room.members.contains_key(&candidate.id) {
                break candidate;
            }
        };

        room.members.insert(member.id.clone(), member.clone());
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(RoomJoin {
            room: room.clone(),
            resume_token: member.resume_token.clone(),
            member,
        })
    }

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

    pub fn get_room(&self, room_id: &str) -> Result<Room> {
        let rooms = self.read_rooms()?;
        rooms.get(room_id).cloned().ok_or(Error::RoomNotFound)
    }

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

    pub fn mark_member_disconnected(&self, room_id: &str, member_id: &str) -> Result<Room> {
        let mut rooms = self.write_rooms()?;
        let room = rooms.get_mut(room_id).ok_or(Error::RoomNotFound)?;
        let member = room
            .members
            .get_mut(member_id)
            .ok_or(Error::MemberNotFound)?;

        member.connected = false;
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

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
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(Some(room.clone()))
    }

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
        room.last_active_epoch_seconds = now_epoch_seconds();

        Ok(room.clone())
    }

    fn new_member(&self, nickname: impl Into<String>, role: MemberRole) -> Member {
        Member {
            id: new_member_id(),
            nickname: nickname.into(),
            role,
            can_speak: true,
            self_muted: false,
            connected: true,
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

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
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
