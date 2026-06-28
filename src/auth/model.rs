use crate::storage::sqlite::{StoredInvite, StoredPersistentRoom, StoredUser};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub fn from_storage(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            _ => Self::User,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
}

impl From<StoredUser> for CurrentUser {
    fn from(user: StoredUser) -> Self {
        Self {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            role: UserRole::from_storage(&user.role),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub user: CurrentUser,
    pub token: String,
    pub expires_at_epoch_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InviteView {
    pub id: i64,
    pub expires_at_epoch_seconds: i64,
    pub used_by_user_id: Option<i64>,
    pub used_at_epoch_seconds: Option<i64>,
    pub created_at_epoch_seconds: i64,
}

impl From<StoredInvite> for InviteView {
    fn from(invite: StoredInvite) -> Self {
        Self {
            id: invite.id,
            expires_at_epoch_seconds: invite.expires_at_epoch_seconds,
            used_by_user_id: invite.used_by_user_id,
            used_at_epoch_seconds: invite.used_at_epoch_seconds,
            created_at_epoch_seconds: invite.created_at_epoch_seconds,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedInvite {
    pub code: String,
    pub invite: InviteView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersistentRoomView {
    pub room_id: String,
    pub owner_user_id: i64,
    pub created_at_epoch_seconds: i64,
    pub last_active_at_epoch_seconds: i64,
}

impl From<StoredPersistentRoom> for PersistentRoomView {
    fn from(room: StoredPersistentRoom) -> Self {
        Self {
            room_id: room.room_id,
            owner_user_id: room.owner_user_id,
            created_at_epoch_seconds: room.created_at_epoch_seconds,
            last_active_at_epoch_seconds: room.last_active_at_epoch_seconds,
        }
    }
}
