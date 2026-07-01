use crate::{
    Result,
    auth::AuthRuntime,
    config::settings::{ScreenShareSettings, Settings, VideoCallSettings},
    domain::room::RoomStore,
    media::MediaController,
    service::Services,
    transport::http::signaling::SignalHub,
};
use std::{sync::Arc, time::Duration};

/// 应用共享状态聚合房间、信令、媒体、认证和服务层，方便 HTTP/WebSocket handler 克隆使用。
#[derive(Clone)]
pub struct AppState {
    pub rooms: Arc<RoomStore>,
    pub signals: Arc<SignalHub>,
    pub media: Arc<MediaController>,
    pub screen_share: ScreenShareSettings,
    pub video_call: VideoCallSettings,
    pub disconnect_grace_period: Duration,
    pub auth: AuthRuntime,
    pub services: Services,
}

impl AppState {
    /// 创建未启用认证的测试或默认状态，使用默认断线宽限期。
    pub fn new(max_members: usize) -> Result<Self> {
        Self::with_disconnect_grace_period(max_members, Duration::from_secs(30))
    }

    /// 创建未启用认证但可自定义断线宽限期的状态，便于断线恢复测试覆盖。
    pub fn with_disconnect_grace_period(
        max_members: usize,
        disconnect_grace_period: Duration,
    ) -> Result<Self> {
        let rooms = Arc::new(RoomStore::new(max_members));
        let media = Arc::new(MediaController::new()?);
        let auth = AuthRuntime::Disabled;
        let services = Services::new(Arc::clone(&rooms), Arc::clone(&media), auth.clone());
        Ok(Self {
            rooms,
            signals: Arc::new(SignalHub::new()),
            media,
            screen_share: ScreenShareSettings::default(),
            video_call: VideoCallSettings::default(),
            disconnect_grace_period,
            auth,
            services,
        })
    }

    /// 创建启用指定认证运行时的状态，供认证接口和持久房间测试复用。
    pub fn new_with_auth(max_members: usize, auth: AuthRuntime) -> Result<Self> {
        let rooms = Arc::new(RoomStore::new(max_members));
        let media = Arc::new(MediaController::new()?);
        let services = Services::new(Arc::clone(&rooms), Arc::clone(&media), auth.clone());
        Ok(Self {
            rooms,
            signals: Arc::new(SignalHub::new()),
            media,
            screen_share: ScreenShareSettings::default(),
            video_call: VideoCallSettings::default(),
            disconnect_grace_period: Duration::from_secs(30),
            auth,
            services,
        })
    }

    /// 从配置构建生产状态，并在同一处装配房间、媒体、认证和服务层依赖。
    pub fn from_settings(settings: &Settings) -> Result<Self> {
        settings.validate()?;
        let rooms = Arc::new(
            RoomStore::new(settings.room.max_members)
                .with_chat_history_limit(settings.room.chat_history_limit),
        );
        let media = Arc::new(MediaController::new_with_udp_port_range(
            settings.media.udp_port_min,
            settings.media.udp_port_max,
            settings.media.public_ip.clone(),
        )?);
        let auth = AuthRuntime::from_settings(settings)?;
        let services = Services::new(Arc::clone(&rooms), Arc::clone(&media), auth.clone());
        Ok(Self {
            rooms,
            signals: Arc::new(SignalHub::new()),
            media,
            screen_share: settings.screen_share.clone(),
            video_call: settings.video_call.clone(),
            disconnect_grace_period: Duration::from_secs(settings.room.disconnect_grace_seconds),
            auth,
            services,
        })
    }
}
