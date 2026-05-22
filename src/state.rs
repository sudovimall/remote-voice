use crate::{
    Result, config::settings::Settings, domain::room::RoomStore, media::MediaController,
    transport::http::signaling::SignalHub,
};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
pub struct AppState {
    pub rooms: Arc<RoomStore>,
    pub signals: Arc<SignalHub>,
    pub media: Arc<MediaController>,
    pub disconnect_grace_period: Duration,
}

impl AppState {
    pub fn new(max_members: usize) -> Result<Self> {
        Self::with_disconnect_grace_period(max_members, Duration::from_secs(30))
    }

    pub fn with_disconnect_grace_period(
        max_members: usize,
        disconnect_grace_period: Duration,
    ) -> Result<Self> {
        Ok(Self {
            rooms: Arc::new(RoomStore::new(max_members)),
            signals: Arc::new(SignalHub::new()),
            media: Arc::new(MediaController::new()?),
            disconnect_grace_period,
        })
    }

    pub fn from_settings(settings: &Settings) -> Result<Self> {
        Ok(Self {
            rooms: Arc::new(RoomStore::new(settings.room.max_members)),
            signals: Arc::new(SignalHub::new()),
            media: Arc::new(MediaController::new_with_udp_port_range(
                settings.media.udp_port_min,
                settings.media.udp_port_max,
                settings.media.public_ip.clone(),
            )?),
            disconnect_grace_period: Duration::from_secs(settings.room.disconnect_grace_seconds),
        })
    }
}
