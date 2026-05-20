use crate::{
    config::settings::Settings, domain::room::RoomStore, transport::http::signaling::SignalHub,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppState {
    pub rooms: Arc<RoomStore>,
    pub signals: Arc<SignalHub>,
}

impl AppState {
    pub fn new(max_members: usize) -> Self {
        Self {
            rooms: Arc::new(RoomStore::new(max_members)),
            signals: Arc::new(SignalHub::new()),
        }
    }

    pub fn from_settings(settings: &Settings) -> Self {
        Self::new(settings.room.max_members)
    }
}
