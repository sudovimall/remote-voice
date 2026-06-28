pub mod model;
pub mod password;
pub mod service;
pub mod session;

use crate::{
    Result,
    auth::{service::AuthService, session::now_epoch_seconds},
    config::settings::Settings,
    storage::sqlite::SqliteStore,
};
use std::sync::Arc;

pub use model::{AuthenticatedSession, CreatedInvite, CurrentUser, InviteView, UserRole};
pub use service::AuthService as Service;

#[derive(Clone)]
pub enum AuthRuntime {
    Disabled,
    Enabled(Arc<AuthService>),
}

impl AuthRuntime {
    pub fn from_settings(settings: &Settings) -> Result<Self> {
        if !settings.auth.enabled {
            return Ok(Self::Disabled);
        }

        let store = Arc::new(SqliteStore::open(&settings.storage.sqlite.path)?);
        let service = Arc::new(AuthService::new(store, settings.auth.session.clone()));
        let admin = settings
            .auth
            .admin
            .as_ref()
            .expect("Settings::validate 保证认证开启时有管理员");
        service.sync_admin(
            &admin.username,
            &admin.password_hash,
            &admin.display_name,
            now_epoch_seconds(),
        )?;

        Ok(Self::Enabled(service))
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    pub fn service(&self) -> Option<&Arc<AuthService>> {
        match self {
            Self::Disabled => None,
            Self::Enabled(service) => Some(service),
        }
    }
}
