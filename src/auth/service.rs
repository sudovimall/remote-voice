use crate::{
    Error, Result,
    auth::{
        model::{AuthenticatedSession, CreatedInvite, CurrentUser, InviteView, UserRole},
        password::{hash_password, verify_password},
        session::{hash_secret, new_invite_code, new_session_token},
    },
    config::settings::AuthSessionSettings,
    storage::sqlite::{SqliteStore, StoredInvite, StoredUser},
};
use std::sync::Arc;

const MAX_INVITE_TTL_HOURS: u64 = 24 * 30;
const MAX_SESSION_TTL_HOURS: u64 = 24 * 365;
const MAX_USERNAME_CHARS: usize = 64;
const MAX_DISPLAY_NAME_CHARS: usize = 64;
const MAX_PASSWORD_CHARS: usize = 1024;

#[derive(Clone)]
pub struct AuthService {
    store: Arc<SqliteStore>,
    session: AuthSessionSettings,
}

impl AuthService {
    pub fn new(store: Arc<SqliteStore>, session: AuthSessionSettings) -> Self {
        Self { store, session }
    }

    pub fn new_for_test(store: Arc<SqliteStore>, ttl_hours: u64) -> Self {
        let mut session = AuthSessionSettings::default();
        session.ttl_hours = ttl_hours;
        Self::new(store, session)
    }

    pub fn store(&self) -> &Arc<SqliteStore> {
        &self.store
    }

    pub fn session_settings(&self) -> &AuthSessionSettings {
        &self.session
    }

    pub fn sync_admin(
        &self,
        username: &str,
        password_hash: &str,
        display_name: &str,
        now_epoch_seconds: i64,
    ) -> Result<CurrentUser> {
        self.store
            .upsert_admin_user(
                username.trim(),
                password_hash.trim(),
                display_name.trim(),
                now_epoch_seconds,
            )
            .map(CurrentUser::from)
    }

    pub fn login_at(
        &self,
        username: &str,
        password: &str,
        now_epoch_seconds: i64,
    ) -> Result<AuthenticatedSession> {
        let user = self
            .store
            .find_user_by_username(username.trim())?
            .ok_or(Error::InvalidCredentials)?;
        if user.disabled_at_epoch_seconds.is_some() {
            return Err(Error::InvalidCredentials);
        }
        if !verify_password(&user.password_hash, password)? {
            return Err(Error::InvalidCredentials);
        }

        self.create_session_for_user(user, now_epoch_seconds)
    }

    pub fn current_user_from_token_at(
        &self,
        token: &str,
        now_epoch_seconds: i64,
    ) -> Result<CurrentUser> {
        let token_hash = hash_secret(token);
        let session = self
            .store
            .find_active_session(&token_hash, now_epoch_seconds)?
            .ok_or(Error::SessionExpired)?;
        let user = self
            .store
            .find_user_by_id(session.user_id)?
            .ok_or(Error::SessionExpired)?;
        if user.disabled_at_epoch_seconds.is_some() {
            return Err(Error::SessionExpired);
        }
        Ok(CurrentUser::from(user))
    }

    pub fn logout_at(&self, token: &str, now_epoch_seconds: i64) -> Result<()> {
        self.store
            .revoke_session(&hash_secret(token), now_epoch_seconds)
    }

    pub fn create_invite_at(
        &self,
        actor: &CurrentUser,
        ttl_hours: u64,
        now_epoch_seconds: i64,
    ) -> Result<CreatedInvite> {
        self.require_admin(actor)?;
        let code = new_invite_code();
        let expires_at_epoch_seconds =
            expires_at_epoch_seconds(now_epoch_seconds, ttl_hours, MAX_INVITE_TTL_HOURS)?;
        let invite = self.store.insert_invite(
            &hash_secret(&code),
            actor.id,
            expires_at_epoch_seconds,
            now_epoch_seconds,
        )?;

        Ok(CreatedInvite {
            code,
            invite: InviteView::from(invite),
        })
    }

    pub fn register_with_invite_at(
        &self,
        code: &str,
        username: &str,
        password: &str,
        display_name: &str,
        now_epoch_seconds: i64,
    ) -> Result<AuthenticatedSession> {
        let username = validate_text("用户名", username, MAX_USERNAME_CHARS)?;
        let password = validate_text("密码", password, MAX_PASSWORD_CHARS)?;
        let display_name = validate_text("显示名", display_name, MAX_DISPLAY_NAME_CHARS)?;
        let invite = self
            .store
            .find_invite_by_code_hash(&hash_secret(code))?
            .ok_or(Error::InviteNotFound)?;
        validate_invite(&invite, now_epoch_seconds)?;

        let password_hash = hash_password(password)?;
        let user = self.store.register_user_with_invite(
            invite.id,
            username,
            &password_hash,
            display_name,
            now_epoch_seconds,
        )?;

        self.create_session_for_user(user, now_epoch_seconds)
    }

    pub fn list_users(&self, actor: &CurrentUser) -> Result<Vec<CurrentUser>> {
        self.require_admin(actor)?;
        self.store
            .list_users()
            .map(|users| users.into_iter().map(CurrentUser::from).collect())
    }

    pub fn list_invites(&self, actor: &CurrentUser) -> Result<Vec<InviteView>> {
        self.require_admin(actor)?;
        self.store
            .list_invites()
            .map(|invites| invites.into_iter().map(InviteView::from).collect())
    }

    pub fn require_admin(&self, actor: &CurrentUser) -> Result<()> {
        if actor.role == UserRole::Admin {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }

    fn create_session_for_user(
        &self,
        user: StoredUser,
        now_epoch_seconds: i64,
    ) -> Result<AuthenticatedSession> {
        let token = new_session_token();
        let expires_at_epoch_seconds = expires_at_epoch_seconds(
            now_epoch_seconds,
            self.session.ttl_hours,
            MAX_SESSION_TTL_HOURS,
        )?;
        self.store.insert_session(
            &hash_secret(&token),
            user.id,
            expires_at_epoch_seconds,
            now_epoch_seconds,
        )?;
        Ok(AuthenticatedSession {
            user: CurrentUser::from(user),
            token,
            expires_at_epoch_seconds,
        })
    }
}

fn validate_text<'a>(label: &str, value: &'a str, max_chars: usize) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::InvalidMessage(format!("{label}不能为空")));
    }
    if value.chars().count() > max_chars {
        return Err(Error::InvalidMessage(format!(
            "{label}不能超过 {max_chars} 个字符"
        )));
    }
    Ok(value)
}

fn validate_hours(label: &str, hours: u64, max_hours: u64) -> Result<()> {
    if hours == 0 {
        return Err(Error::InvalidMessage(format!("{label}必须大于 0 小时")));
    }
    if hours > max_hours {
        return Err(Error::InvalidMessage(format!(
            "{label}不能超过 {max_hours} 小时"
        )));
    }
    Ok(())
}

fn expires_at_epoch_seconds(now_epoch_seconds: i64, ttl_hours: u64, max_hours: u64) -> Result<i64> {
    validate_hours("有效期", ttl_hours, max_hours)?;
    let seconds = ttl_hours
        .checked_mul(60 * 60)
        .and_then(|seconds| i64::try_from(seconds).ok())
        .ok_or_else(|| Error::InvalidMessage("有效期过大".to_string()))?;
    now_epoch_seconds
        .checked_add(seconds)
        .ok_or_else(|| Error::InvalidMessage("有效期过大".to_string()))
}

fn validate_invite(invite: &StoredInvite, now_epoch_seconds: i64) -> Result<()> {
    if invite.expires_at_epoch_seconds <= now_epoch_seconds {
        return Err(Error::InviteExpired);
    }
    if invite.used_by_user_id.is_some() || invite.used_at_epoch_seconds.is_some() {
        return Err(Error::InviteUsed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::password::hash_password, storage::sqlite::SqliteStore};
    use std::sync::Arc;

    fn service() -> AuthService {
        let store = Arc::new(SqliteStore::open_in_memory().expect("打开内存数据库"));
        AuthService::new_for_test(store, 24)
    }

    #[test]
    fn 登录成功会创建_session_并能查询当前用户() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");

        let login = service.login_at("admin", "secret", 20).expect("登录成功");
        assert_eq!(login.user.username, "admin");
        assert_eq!(login.user.role, UserRole::Admin);
        assert!(login.token.starts_with("s_"));

        let current = service
            .current_user_from_token_at(&login.token, 21)
            .expect("查询当前用户");
        assert_eq!(current.id, login.user.id);
    }

    #[test]
    fn 登录失败不区分用户名和密码错误() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");

        assert!(matches!(
            service.login_at("admin", "bad", 20),
            Err(Error::InvalidCredentials)
        ));
        assert!(matches!(
            service.login_at("missing", "secret", 20),
            Err(Error::InvalidCredentials)
        ));
    }

    #[test]
    fn logout_后_session_不可用() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        let login = service.login_at("admin", "secret", 20).expect("登录成功");

        service.logout_at(&login.token, 30).expect("退出登录");

        assert!(matches!(
            service.current_user_from_token_at(&login.token, 31),
            Err(Error::SessionExpired)
        ));
    }

    #[test]
    fn 管理员可以创建邀请码且有效邀请码可以注册用户() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        let admin = service.login_at("admin", "secret", 20).expect("登录成功");

        let invite = service
            .create_invite_at(&admin.user, 48, 30)
            .expect("创建邀请码");
        let registered = service
            .register_with_invite_at(&invite.code, "alice", "password", "Alice", 40)
            .expect("注册用户");

        assert_eq!(registered.user.username, "alice");
        assert_eq!(registered.user.role, UserRole::User);
        assert!(registered.token.starts_with("s_"));
        assert!(matches!(
            service.register_with_invite_at(&invite.code, "bob", "password", "Bob", 41),
            Err(Error::InviteUsed)
        ));
    }

    #[test]
    fn 普通用户不能创建邀请码() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        let admin = service.login_at("admin", "secret", 20).expect("登录成功");
        let invite = service
            .create_invite_at(&admin.user, 48, 30)
            .expect("创建邀请码");
        let user = service
            .register_with_invite_at(&invite.code, "alice", "password", "Alice", 40)
            .expect("注册用户");

        assert!(matches!(
            service.create_invite_at(&user.user, 48, 50),
            Err(Error::Forbidden)
        ));
    }

    #[test]
    fn 邀请码_ttl_不能超过上限() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        let admin = service.login_at("admin", "secret", 20).expect("登录成功");

        assert!(matches!(
            service.create_invite_at(&admin.user, u64::MAX, 30),
            Err(Error::InvalidMessage(_))
        ));
    }

    #[test]
    fn 注册会拒绝空用户名空密码和空显示名() {
        let service = service();
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        let admin = service.login_at("admin", "secret", 20).expect("登录成功");

        let empty_username = service
            .create_invite_at(&admin.user, 48, 30)
            .expect("创建邀请码");
        assert!(matches!(
            service.register_with_invite_at(&empty_username.code, " ", "password", "Alice", 40),
            Err(Error::InvalidMessage(_))
        ));

        let empty_password = service
            .create_invite_at(&admin.user, 48, 31)
            .expect("创建邀请码");
        assert!(matches!(
            service.register_with_invite_at(&empty_password.code, "alice", " ", "Alice", 40),
            Err(Error::InvalidMessage(_))
        ));

        let empty_display_name = service
            .create_invite_at(&admin.user, 48, 32)
            .expect("创建邀请码");
        assert!(matches!(
            service.register_with_invite_at(&empty_display_name.code, "alice", "password", " ", 40),
            Err(Error::InvalidMessage(_))
        ));
    }
}
