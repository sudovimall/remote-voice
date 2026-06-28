use crate::{Error, Result, storage::migrations::SQLITE_SCHEMA};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredUser {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub role: String,
    pub created_at_epoch_seconds: i64,
    pub updated_at_epoch_seconds: i64,
    pub disabled_at_epoch_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub id: i64,
    pub token_hash: String,
    pub user_id: i64,
    pub expires_at_epoch_seconds: i64,
    pub created_at_epoch_seconds: i64,
    pub last_seen_at_epoch_seconds: i64,
    pub revoked_at_epoch_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredInvite {
    pub id: i64,
    pub code_hash: String,
    pub created_by_user_id: i64,
    pub expires_at_epoch_seconds: i64,
    pub used_by_user_id: Option<i64>,
    pub used_at_epoch_seconds: Option<i64>,
    pub created_at_epoch_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPersistentRoom {
    pub room_id: String,
    pub owner_user_id: i64,
    pub created_at_epoch_seconds: i64,
    pub last_active_at_epoch_seconds: i64,
    pub closed_at_epoch_seconds: Option<i64>,
}

pub struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path).map_err(map_sql_error)?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory().map_err(map_sql_error)?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute_batch(SQLITE_SCHEMA)
            .map_err(map_sql_error)
    }

    pub fn upsert_admin_user(
        &self,
        username: &str,
        password_hash: &str,
        display_name: &str,
        now_epoch_seconds: i64,
    ) -> Result<StoredUser> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                INSERT INTO users (
                    username,
                    password_hash,
                    display_name,
                    role,
                    created_at_epoch_seconds,
                    updated_at_epoch_seconds,
                    disabled_at_epoch_seconds
                )
                VALUES (?1, ?2, ?3, 'admin', ?4, ?4, NULL)
                ON CONFLICT(username) DO UPDATE SET
                    password_hash = excluded.password_hash,
                    display_name = excluded.display_name,
                    role = 'admin',
                    updated_at_epoch_seconds = excluded.updated_at_epoch_seconds,
                    disabled_at_epoch_seconds = NULL
                "#,
                params![username, password_hash, display_name, now_epoch_seconds],
            )
            .map_err(map_sql_error)?;

        connection
            .query_row(
                r#"
                SELECT id, username, password_hash, display_name, role,
                       created_at_epoch_seconds, updated_at_epoch_seconds,
                       disabled_at_epoch_seconds
                FROM users
                WHERE username = ?1
                "#,
                params![username],
                stored_user_from_row,
            )
            .map_err(map_sql_error)
    }

    pub fn find_user_by_username(&self, username: &str) -> Result<Option<StoredUser>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT id, username, password_hash, display_name, role,
                       created_at_epoch_seconds, updated_at_epoch_seconds,
                       disabled_at_epoch_seconds
                FROM users
                WHERE username = ?1
                "#,
                params![username],
                stored_user_from_row,
            )
            .optional()
            .map_err(map_sql_error)
    }

    pub fn find_user_by_id(&self, user_id: i64) -> Result<Option<StoredUser>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT id, username, password_hash, display_name, role,
                       created_at_epoch_seconds, updated_at_epoch_seconds,
                       disabled_at_epoch_seconds
                FROM users
                WHERE id = ?1
                "#,
                params![user_id],
                stored_user_from_row,
            )
            .optional()
            .map_err(map_sql_error)
    }

    pub fn list_users(&self) -> Result<Vec<StoredUser>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                r#"
                SELECT id, username, password_hash, display_name, role,
                       created_at_epoch_seconds, updated_at_epoch_seconds,
                       disabled_at_epoch_seconds
                FROM users
                ORDER BY id
                "#,
            )
            .map_err(map_sql_error)?;
        collect_rows(statement.query_map([], stored_user_from_row))
    }

    pub fn insert_session(
        &self,
        token_hash: &str,
        user_id: i64,
        expires_at_epoch_seconds: i64,
        now_epoch_seconds: i64,
    ) -> Result<StoredSession> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                INSERT INTO sessions (
                    token_hash,
                    user_id,
                    expires_at_epoch_seconds,
                    created_at_epoch_seconds,
                    last_seen_at_epoch_seconds,
                    revoked_at_epoch_seconds
                )
                VALUES (?1, ?2, ?3, ?4, ?4, NULL)
                "#,
                params![
                    token_hash,
                    user_id,
                    expires_at_epoch_seconds,
                    now_epoch_seconds
                ],
            )
            .map_err(map_sql_error)?;
        let id = connection.last_insert_rowid();
        connection
            .query_row(
                r#"
                SELECT id, token_hash, user_id, expires_at_epoch_seconds,
                       created_at_epoch_seconds, last_seen_at_epoch_seconds,
                       revoked_at_epoch_seconds
                FROM sessions
                WHERE id = ?1
                "#,
                params![id],
                stored_session_from_row,
            )
            .map_err(map_sql_error)
    }

    pub fn find_active_session(
        &self,
        token_hash: &str,
        now_epoch_seconds: i64,
    ) -> Result<Option<StoredSession>> {
        let connection = self.connection()?;
        let session = connection
            .query_row(
                r#"
                SELECT id, token_hash, user_id, expires_at_epoch_seconds,
                       created_at_epoch_seconds, last_seen_at_epoch_seconds,
                       revoked_at_epoch_seconds
                FROM sessions
                WHERE token_hash = ?1
                  AND revoked_at_epoch_seconds IS NULL
                  AND expires_at_epoch_seconds > ?2
                "#,
                params![token_hash, now_epoch_seconds],
                stored_session_from_row,
            )
            .optional()
            .map_err(map_sql_error)?;

        if session.is_some() {
            connection
                .execute(
                    "UPDATE sessions SET last_seen_at_epoch_seconds = ?1 WHERE token_hash = ?2",
                    params![now_epoch_seconds, token_hash],
                )
                .map_err(map_sql_error)?;
        }

        Ok(session)
    }

    pub fn revoke_session(&self, token_hash: &str, now_epoch_seconds: i64) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                UPDATE sessions
                SET revoked_at_epoch_seconds = ?1
                WHERE token_hash = ?2
                  AND revoked_at_epoch_seconds IS NULL
                "#,
                params![now_epoch_seconds, token_hash],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub fn insert_invite(
        &self,
        code_hash: &str,
        created_by_user_id: i64,
        expires_at_epoch_seconds: i64,
        now_epoch_seconds: i64,
    ) -> Result<StoredInvite> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                INSERT INTO invite_codes (
                    code_hash,
                    created_by_user_id,
                    expires_at_epoch_seconds,
                    used_by_user_id,
                    used_at_epoch_seconds,
                    created_at_epoch_seconds
                )
                VALUES (?1, ?2, ?3, NULL, NULL, ?4)
                "#,
                params![
                    code_hash,
                    created_by_user_id,
                    expires_at_epoch_seconds,
                    now_epoch_seconds
                ],
            )
            .map_err(map_sql_error)?;
        let id = connection.last_insert_rowid();
        connection
            .query_row(
                r#"
                SELECT id, code_hash, created_by_user_id, expires_at_epoch_seconds,
                       used_by_user_id, used_at_epoch_seconds, created_at_epoch_seconds
                FROM invite_codes
                WHERE id = ?1
                "#,
                params![id],
                stored_invite_from_row,
            )
            .map_err(map_sql_error)
    }

    pub fn find_invite_by_code_hash(&self, code_hash: &str) -> Result<Option<StoredInvite>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT id, code_hash, created_by_user_id, expires_at_epoch_seconds,
                       used_by_user_id, used_at_epoch_seconds, created_at_epoch_seconds
                FROM invite_codes
                WHERE code_hash = ?1
                "#,
                params![code_hash],
                stored_invite_from_row,
            )
            .optional()
            .map_err(map_sql_error)
    }

    pub fn list_invites(&self) -> Result<Vec<StoredInvite>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                r#"
                SELECT id, code_hash, created_by_user_id, expires_at_epoch_seconds,
                       used_by_user_id, used_at_epoch_seconds, created_at_epoch_seconds
                FROM invite_codes
                ORDER BY id DESC
                "#,
            )
            .map_err(map_sql_error)?;
        collect_rows(statement.query_map([], stored_invite_from_row))
    }

    pub fn register_user_with_invite(
        &self,
        invite_id: i64,
        username: &str,
        password_hash: &str,
        display_name: &str,
        now_epoch_seconds: i64,
    ) -> Result<StoredUser> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(map_sql_error)?;
        let invite = transaction
            .query_row(
                r#"
                SELECT id, code_hash, created_by_user_id, expires_at_epoch_seconds,
                       used_by_user_id, used_at_epoch_seconds, created_at_epoch_seconds
                FROM invite_codes
                WHERE id = ?1
                "#,
                params![invite_id],
                stored_invite_from_row,
            )
            .optional()
            .map_err(map_sql_error)?
            .ok_or(Error::InviteNotFound)?;

        if invite.expires_at_epoch_seconds <= now_epoch_seconds {
            return Err(Error::InviteExpired);
        }
        if invite.used_by_user_id.is_some() || invite.used_at_epoch_seconds.is_some() {
            return Err(Error::InviteUsed);
        }

        transaction
            .execute(
                r#"
                INSERT INTO users (
                    username,
                    password_hash,
                    display_name,
                    role,
                    created_at_epoch_seconds,
                    updated_at_epoch_seconds,
                    disabled_at_epoch_seconds
                )
                VALUES (?1, ?2, ?3, 'user', ?4, ?4, NULL)
                "#,
                params![username, password_hash, display_name, now_epoch_seconds],
            )
            .map_err(map_user_insert_error)?;
        let user_id = transaction.last_insert_rowid();
        transaction
            .execute(
                r#"
                UPDATE invite_codes
                SET used_by_user_id = ?1,
                    used_at_epoch_seconds = ?2
                WHERE id = ?3
                "#,
                params![user_id, now_epoch_seconds, invite_id],
            )
            .map_err(map_sql_error)?;

        let user = transaction
            .query_row(
                r#"
                SELECT id, username, password_hash, display_name, role,
                       created_at_epoch_seconds, updated_at_epoch_seconds,
                       disabled_at_epoch_seconds
                FROM users
                WHERE id = ?1
                "#,
                params![user_id],
                stored_user_from_row,
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(user)
    }

    pub fn create_persistent_room(
        &self,
        room_id: &str,
        owner_user_id: i64,
        now_epoch_seconds: i64,
    ) -> Result<StoredPersistentRoom> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                INSERT INTO persistent_rooms (
                    room_id,
                    owner_user_id,
                    created_at_epoch_seconds,
                    last_active_at_epoch_seconds,
                    closed_at_epoch_seconds
                )
                VALUES (?1, ?2, ?3, ?3, NULL)
                ON CONFLICT(room_id) DO UPDATE SET
                    owner_user_id = excluded.owner_user_id,
                    last_active_at_epoch_seconds = excluded.last_active_at_epoch_seconds,
                    closed_at_epoch_seconds = NULL
                "#,
                params![room_id, owner_user_id, now_epoch_seconds],
            )
            .map_err(map_sql_error)?;

        connection
            .query_row(
                r#"
                SELECT room_id, owner_user_id, created_at_epoch_seconds,
                       last_active_at_epoch_seconds, closed_at_epoch_seconds
                FROM persistent_rooms
                WHERE room_id = ?1
                "#,
                params![room_id],
                stored_persistent_room_from_row,
            )
            .map_err(map_sql_error)
    }

    pub fn find_open_persistent_room(&self, room_id: &str) -> Result<Option<StoredPersistentRoom>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT room_id, owner_user_id, created_at_epoch_seconds,
                       last_active_at_epoch_seconds, closed_at_epoch_seconds
                FROM persistent_rooms
                WHERE room_id = ?1
                  AND closed_at_epoch_seconds IS NULL
                "#,
                params![room_id],
                stored_persistent_room_from_row,
            )
            .optional()
            .map_err(map_sql_error)
    }

    pub fn find_persistent_room(&self, room_id: &str) -> Result<Option<StoredPersistentRoom>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT room_id, owner_user_id, created_at_epoch_seconds,
                       last_active_at_epoch_seconds, closed_at_epoch_seconds
                FROM persistent_rooms
                WHERE room_id = ?1
                "#,
                params![room_id],
                stored_persistent_room_from_row,
            )
            .optional()
            .map_err(map_sql_error)
    }

    pub fn list_open_persistent_rooms(&self) -> Result<Vec<StoredPersistentRoom>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                r#"
                SELECT room_id, owner_user_id, created_at_epoch_seconds,
                       last_active_at_epoch_seconds, closed_at_epoch_seconds
                FROM persistent_rooms
                WHERE closed_at_epoch_seconds IS NULL
                ORDER BY room_id
                "#,
            )
            .map_err(map_sql_error)?;
        collect_rows(statement.query_map([], stored_persistent_room_from_row))
    }

    pub fn close_persistent_room(&self, room_id: &str, now_epoch_seconds: i64) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                UPDATE persistent_rooms
                SET closed_at_epoch_seconds = ?1,
                    last_active_at_epoch_seconds = ?1
                WHERE room_id = ?2
                  AND closed_at_epoch_seconds IS NULL
                "#,
                params![now_epoch_seconds, room_id],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    pub fn touch_persistent_room(&self, room_id: &str, now_epoch_seconds: i64) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                UPDATE persistent_rooms
                SET last_active_at_epoch_seconds = ?1
                WHERE room_id = ?2
                  AND closed_at_epoch_seconds IS NULL
                "#,
                params![now_epoch_seconds, room_id],
            )
            .map_err(map_sql_error)?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| Error::Database("SQLite 连接锁已损坏".to_string()))
    }
}

fn collect_rows<T>(
    rows: rusqlite::Result<rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>>,
) -> Result<Vec<T>> {
    let mut values = Vec::new();
    for row in rows.map_err(map_sql_error)? {
        values.push(row.map_err(map_sql_error)?);
    }
    Ok(values)
}

fn stored_user_from_row(row: &Row<'_>) -> rusqlite::Result<StoredUser> {
    Ok(StoredUser {
        id: row.get(0)?,
        username: row.get(1)?,
        password_hash: row.get(2)?,
        display_name: row.get(3)?,
        role: row.get(4)?,
        created_at_epoch_seconds: row.get(5)?,
        updated_at_epoch_seconds: row.get(6)?,
        disabled_at_epoch_seconds: row.get(7)?,
    })
}

fn stored_session_from_row(row: &Row<'_>) -> rusqlite::Result<StoredSession> {
    Ok(StoredSession {
        id: row.get(0)?,
        token_hash: row.get(1)?,
        user_id: row.get(2)?,
        expires_at_epoch_seconds: row.get(3)?,
        created_at_epoch_seconds: row.get(4)?,
        last_seen_at_epoch_seconds: row.get(5)?,
        revoked_at_epoch_seconds: row.get(6)?,
    })
}

fn stored_invite_from_row(row: &Row<'_>) -> rusqlite::Result<StoredInvite> {
    Ok(StoredInvite {
        id: row.get(0)?,
        code_hash: row.get(1)?,
        created_by_user_id: row.get(2)?,
        expires_at_epoch_seconds: row.get(3)?,
        used_by_user_id: row.get(4)?,
        used_at_epoch_seconds: row.get(5)?,
        created_at_epoch_seconds: row.get(6)?,
    })
}

fn stored_persistent_room_from_row(row: &Row<'_>) -> rusqlite::Result<StoredPersistentRoom> {
    Ok(StoredPersistentRoom {
        room_id: row.get(0)?,
        owner_user_id: row.get(1)?,
        created_at_epoch_seconds: row.get(2)?,
        last_active_at_epoch_seconds: row.get(3)?,
        closed_at_epoch_seconds: row.get(4)?,
    })
}

fn map_user_insert_error(error: rusqlite::Error) -> Error {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(ref sqlite_error, _)
            if sqlite_error.code == ErrorCode::ConstraintViolation
    ) {
        Error::UsernameTaken
    } else {
        map_sql_error(error)
    }
}

fn map_sql_error(error: rusqlite::Error) -> Error {
    Error::Database(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_迁移后可以同步管理员和查询用户() {
        let store = SqliteStore::open_in_memory().expect("打开内存数据库");

        let admin = store
            .upsert_admin_user("admin", "hash-1", "管理员", 1)
            .expect("同步管理员");
        assert_eq!(admin.username, "admin");
        assert_eq!(admin.role, "admin");

        let updated = store
            .upsert_admin_user("admin", "hash-2", "Root", 2)
            .expect("更新管理员");
        assert_eq!(updated.id, admin.id);
        assert_eq!(updated.password_hash, "hash-2");
        assert_eq!(updated.display_name, "Root");

        let found = store
            .find_user_by_username("admin")
            .expect("查询用户")
            .expect("用户存在");
        assert_eq!(found.id, admin.id);
    }

    #[test]
    fn sqlite_session_可以创建查询和撤销() {
        let store = SqliteStore::open_in_memory().expect("打开内存数据库");
        let admin = store
            .upsert_admin_user("admin", "hash", "管理员", 10)
            .expect("同步管理员");

        store
            .insert_session("token-hash", admin.id, 100, 10)
            .expect("创建 session");
        let session = store
            .find_active_session("token-hash", 20)
            .expect("查询 session")
            .expect("session 有效");
        assert_eq!(session.user_id, admin.id);

        store
            .revoke_session("token-hash", 30)
            .expect("撤销 session");
        assert!(
            store
                .find_active_session("token-hash", 40)
                .expect("查询 session")
                .is_none()
        );
    }

    #[test]
    fn sqlite_邀请码注册会创建用户并标记已使用() {
        let store = SqliteStore::open_in_memory().expect("打开内存数据库");
        let admin = store
            .upsert_admin_user("admin", "hash", "管理员", 10)
            .expect("同步管理员");
        let invite = store
            .insert_invite("invite-hash", admin.id, 100, 10)
            .expect("创建邀请码");

        let user = store
            .register_user_with_invite(invite.id, "alice", "alice-hash", "Alice", 20)
            .expect("注册用户");

        assert_eq!(user.username, "alice");
        assert_eq!(user.role, "user");
        let used = store
            .find_invite_by_code_hash("invite-hash")
            .expect("查询邀请码")
            .expect("邀请码存在");
        assert_eq!(used.used_by_user_id, Some(user.id));
        assert_eq!(used.used_at_epoch_seconds, Some(20));
    }

    #[test]
    fn sqlite_持久房间可以创建列表和关闭() {
        let store = SqliteStore::open_in_memory().expect("打开内存数据库");
        let admin = store
            .upsert_admin_user("admin", "hash", "管理员", 10)
            .expect("同步管理员");

        store
            .create_persistent_room("ABC123", admin.id, 20)
            .expect("创建持久房间");
        let rooms = store.list_open_persistent_rooms().expect("列出房间");
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].room_id, "ABC123");

        store.close_persistent_room("ABC123", 30).expect("关闭房间");
        assert!(
            store
                .find_open_persistent_room("ABC123")
                .expect("查询房间")
                .is_none()
        );
    }
}
