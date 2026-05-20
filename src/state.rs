use crate::{
    Result, config::settings::Settings, domain::room::RoomStore, media::MediaController,
    transport::http::signaling::SignalHub,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppState {
    pub rooms: Arc<RoomStore>,
    pub signals: Arc<SignalHub>,
    pub media: Arc<MediaController>,
}

impl AppState {
    pub fn new(max_members: usize) -> Result<Self> {
        Ok(Self {
            rooms: Arc::new(RoomStore::new(max_members)),
            signals: Arc::new(SignalHub::new()),
            media: Arc::new(MediaController::new()?),
        })
    }

    pub fn from_settings(settings: &Settings) -> Result<Self> {
        Self::new(settings.room.max_members)
    }
}
