use crate::{config::settings::Settings, domain::room::RoomStore};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppState {
    pub rooms: Arc<RoomStore>,
}

impl AppState {
    pub fn new(max_members: usize) -> Self {
        Self {
            rooms: Arc::new(RoomStore::new(max_members)),
        }
    }

    pub fn from_settings(settings: &Settings) -> Self {
        Self::new(settings.room.max_members)
    }
}
