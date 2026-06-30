use crate::{Error, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::{Display, Formatter};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// HTTP 服务监听端口。
    pub port: u16,
    #[serde(default)]
    pub room: RoomSettings,
    #[serde(default)]
    pub media: MediaSettings,
    #[serde(default)]
    pub screen_share: ScreenShareSettings,
    #[serde(default)]
    pub video_call: VideoCallSettings,
    #[serde(default)]
    pub auth: AuthSettings,
    #[serde(default)]
    pub storage: StorageSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSettings {
    #[serde(default = "default_max_members")]
    pub max_members: usize,
    #[serde(default = "default_disconnect_grace_seconds")]
    pub disconnect_grace_seconds: u64,
    #[serde(default = "default_chat_history_limit")]
    pub chat_history_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSettings {
    #[serde(default = "default_udp_port_min")]
    pub udp_port_min: u16,
    #[serde(default = "default_udp_port_max")]
    pub udp_port_max: u16,
    #[serde(default)]
    pub public_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareSettings {
    #[serde(default = "default_screen_share_max_width")]
    pub max_width: u32,
    #[serde(default = "default_screen_share_max_height")]
    pub max_height: u32,
    #[serde(default = "default_screen_share_max_frame_rate")]
    pub max_frame_rate: u32,
    #[serde(default = "default_screen_share_bitrate_rules")]
    pub bitrate_rules: Vec<ScreenShareBitrateRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareBitrateRule {
    pub max_viewers: u32,
    pub max_bitrate_bps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoCallSettings {
    #[serde(default = "default_video_call_max_width")]
    pub max_width: u32,
    #[serde(default = "default_video_call_max_height")]
    pub max_height: u32,
    #[serde(default = "default_video_call_max_frame_rate")]
    pub max_frame_rate: u32,
    #[serde(
        default = "default_video_call_bitrate_rules",
        deserialize_with = "deserialize_video_call_bitrate_rules"
    )]
    pub bitrate_rules: Vec<VideoCallBitrateRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoCallBitrateRule {
    pub max_publishers: u32,
    pub max_bitrate_bps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub admin: Option<AuthAdminSettings>,
    #[serde(default)]
    pub session: AuthSessionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAdminSettings {
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSessionSettings {
    #[serde(default = "default_session_cookie_name")]
    pub cookie_name: String,
    #[serde(default = "default_session_ttl_hours")]
    pub ttl_hours: u64,
    #[serde(default)]
    pub secure: SessionSecureSetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionSecureSetting {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    #[serde(default)]
    pub kind: StorageKind,
    #[serde(default)]
    pub sqlite: SqliteSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    #[default]
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteSettings {
    #[serde(default = "default_sqlite_path")]
    pub path: String,
}

impl Default for RoomSettings {
    fn default() -> Self {
        Self {
            max_members: default_max_members(),
            disconnect_grace_seconds: default_disconnect_grace_seconds(),
            chat_history_limit: default_chat_history_limit(),
        }
    }
}

impl Default for MediaSettings {
    fn default() -> Self {
        Self {
            udp_port_min: default_udp_port_min(),
            udp_port_max: default_udp_port_max(),
            public_ip: None,
        }
    }
}

impl Default for ScreenShareSettings {
    fn default() -> Self {
        Self {
            max_width: default_screen_share_max_width(),
            max_height: default_screen_share_max_height(),
            max_frame_rate: default_screen_share_max_frame_rate(),
            bitrate_rules: default_screen_share_bitrate_rules(),
        }
    }
}

impl Default for VideoCallSettings {
    fn default() -> Self {
        Self {
            max_width: default_video_call_max_width(),
            max_height: default_video_call_max_height(),
            max_frame_rate: default_video_call_max_frame_rate(),
            bitrate_rules: default_video_call_bitrate_rules(),
        }
    }
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            admin: None,
            session: AuthSessionSettings::default(),
        }
    }
}

impl Default for AuthSessionSettings {
    fn default() -> Self {
        Self {
            cookie_name: default_session_cookie_name(),
            ttl_hours: default_session_ttl_hours(),
            secure: SessionSecureSetting::Auto,
        }
    }
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            kind: StorageKind::Sqlite,
            sqlite: SqliteSettings::default(),
        }
    }
}

impl Default for SqliteSettings {
    fn default() -> Self {
        Self {
            path: default_sqlite_path(),
        }
    }
}

fn default_max_members() -> usize {
    8
}

fn default_disconnect_grace_seconds() -> u64 {
    30
}

fn default_chat_history_limit() -> usize {
    100
}

fn default_udp_port_min() -> u16 {
    40000
}

fn default_udp_port_max() -> u16 {
    40100
}

fn default_screen_share_max_width() -> u32 {
    1280
}

fn default_screen_share_max_height() -> u32 {
    720
}

fn default_screen_share_max_frame_rate() -> u32 {
    12
}

fn default_screen_share_bitrate_rules() -> Vec<ScreenShareBitrateRule> {
    vec![
        ScreenShareBitrateRule {
            max_viewers: 1,
            max_bitrate_bps: 2_000_000,
        },
        ScreenShareBitrateRule {
            max_viewers: 2,
            max_bitrate_bps: 1_200_000,
        },
        ScreenShareBitrateRule {
            max_viewers: 8,
            max_bitrate_bps: 800_000,
        },
    ]
}

fn default_video_call_max_width() -> u32 {
    640
}

fn default_video_call_max_height() -> u32 {
    360
}

fn default_video_call_max_frame_rate() -> u32 {
    15
}

fn default_video_call_bitrate_rules() -> Vec<VideoCallBitrateRule> {
    vec![
        VideoCallBitrateRule {
            max_publishers: 1,
            max_bitrate_bps: 800_000,
        },
        VideoCallBitrateRule {
            max_publishers: 4,
            max_bitrate_bps: 500_000,
        },
        VideoCallBitrateRule {
            max_publishers: 8,
            max_bitrate_bps: 300_000,
        },
    ]
}

fn deserialize_video_call_bitrate_rules<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<VideoCallBitrateRule>, D::Error>
where
    D: Deserializer<'de>,
{
    let rules = Vec::<VideoCallBitrateRule>::deserialize(deserializer)?;
    Ok(normalize_video_call_bitrate_rules(rules))
}

fn normalize_video_call_bitrate_rules(
    rules: Vec<VideoCallBitrateRule>,
) -> Vec<VideoCallBitrateRule> {
    let mut rules = rules
        .into_iter()
        .filter(|rule| rule.max_publishers > 0 && rule.max_bitrate_bps > 0)
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return default_video_call_bitrate_rules();
    }

    rules.sort_by_key(|rule| rule.max_publishers);
    rules
}

fn default_session_cookie_name() -> String {
    "remote_voice_session".to_string()
}

fn default_session_ttl_hours() -> u64 {
    168
}

fn default_sqlite_path() -> String {
    "remote-voice.db".to_string()
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.auth.enabled {
            let Some(admin) = &self.auth.admin else {
                return Err(Error::ConfigValue(
                    "auth.enabled=true 时必须配置 auth.admin".to_string(),
                ));
            };

            if admin.username.trim().is_empty() {
                return Err(Error::ConfigValue(
                    "auth.admin.username 不能为空".to_string(),
                ));
            }
            if admin.password_hash.trim().is_empty() {
                return Err(Error::ConfigValue(
                    "auth.admin.password_hash 不能为空".to_string(),
                ));
            }
            if admin.display_name.trim().is_empty() {
                return Err(Error::ConfigValue(
                    "auth.admin.display_name 不能为空".to_string(),
                ));
            }
            if self.auth.session.cookie_name.trim().is_empty() {
                return Err(Error::ConfigValue(
                    "auth.session.cookie_name 不能为空".to_string(),
                ));
            }
            if self.auth.session.ttl_hours == 0 {
                return Err(Error::ConfigValue(
                    "auth.session.ttl_hours 必须大于 0".to_string(),
                ));
            }
            if self.storage.sqlite.path.trim().is_empty() {
                return Err(Error::ConfigValue(
                    "storage.sqlite.path 不能为空".to_string(),
                ));
            }
        }

        Ok(())
    }
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[监听端口 = {}, 房间人数上限 = {}, 断线保留秒数 = {}, 聊天历史条数 = {}, 媒体 UDP 端口范围 = {}-{}, 对外媒体 IP = {}, 屏幕共享 = {}x{}@{}fps, 视频通话 = {}x{}@{}fps, 认证 = {}]",
            self.port,
            self.room.max_members,
            self.room.disconnect_grace_seconds,
            self.room.chat_history_limit,
            self.media.udp_port_min,
            self.media.udp_port_max,
            self.media.public_ip.as_deref().unwrap_or("未配置"),
            self.screen_share.max_width,
            self.screen_share.max_height,
            self.screen_share.max_frame_rate,
            self.video_call.max_width,
            self.video_call.max_height,
            self.video_call.max_frame_rate,
            if self.auth.enabled {
                "开启"
            } else {
                "关闭"
            }
        )?;
        Ok(())
    }
}

pub fn init_config() -> Result<Settings> {
    let s = match std::env::var("REMOTE_VOICE_CONFIG") {
        Ok(path) => std::fs::read_to_string(path)?,
        Err(_) => std::fs::read_to_string("application.yaml").unwrap_or_else(|_| {
            r#"
            port: 18080
            room:
              max_members: 8
              chat_history_limit: 100
            media:
              udp_port_min: 40000
              udp_port_max: 40100
	            screen_share:
	              max_width: 1280
	              max_height: 720
	              max_frame_rate: 12
	            video_call:
	              max_width: 640
	              max_height: 360
	              max_frame_rate: 15
	           "#
            .to_string()
        }),
    };
    let config = serde_yaml::from_str::<Settings>(s.as_str())?;
    config.validate()?;
    info!("后端配置已加载：{config}");
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn 最小配置只需要端口() {
        let settings: Settings = serde_yaml::from_str("port: 9000").expect("解析最小配置");

        assert_eq!(settings.port, 9000);
        assert_eq!(settings.room.max_members, 8);
        assert_eq!(settings.room.disconnect_grace_seconds, 30);
        assert_eq!(settings.room.chat_history_limit, 100);
        assert_eq!(settings.media.udp_port_min, 40000);
        assert_eq!(settings.media.udp_port_max, 40100);
        assert_eq!(settings.media.public_ip, None);
        assert_eq!(settings.screen_share.max_width, 1280);
        assert_eq!(settings.screen_share.max_height, 720);
        assert_eq!(settings.screen_share.max_frame_rate, 12);
        assert_eq!(settings.screen_share.bitrate_rules.len(), 3);
        assert_eq!(settings.video_call.max_width, 640);
        assert_eq!(settings.video_call.max_height, 360);
        assert_eq!(settings.video_call.max_frame_rate, 15);
        assert_eq!(settings.video_call.bitrate_rules.len(), 3);
    }

    #[test]
    fn 媒体_udp_端口范围可以配置并显示() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            media:
              udp_port_min: 41000
              udp_port_max: 41015
              public_ip: 111.228.39.21
            "#,
        )
        .expect("解析媒体配置");

        assert_eq!(settings.media.udp_port_min, 41000);
        assert_eq!(settings.media.udp_port_max, 41015);
        assert_eq!(settings.media.public_ip.as_deref(), Some("111.228.39.21"));
        assert!(
            settings
                .to_string()
                .contains("媒体 UDP 端口范围 = 41000-41015")
        );
        assert!(settings.to_string().contains("对外媒体 IP = 111.228.39.21"));
    }

    #[test]
    fn 屏幕共享码率策略可以配置并显示() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            screen_share:
              max_width: 1024
              max_height: 576
              max_frame_rate: 10
              bitrate_rules:
                - max_viewers: 1
                  max_bitrate_bps: 1500000
                - max_viewers: 4
                  max_bitrate_bps: 600000
            "#,
        )
        .expect("解析屏幕共享配置");

        assert_eq!(settings.screen_share.max_width, 1024);
        assert_eq!(settings.screen_share.max_height, 576);
        assert_eq!(settings.screen_share.max_frame_rate, 10);
        assert_eq!(settings.screen_share.bitrate_rules.len(), 2);
        assert_eq!(settings.screen_share.bitrate_rules[1].max_viewers, 4);
        assert_eq!(
            settings.screen_share.bitrate_rules[1].max_bitrate_bps,
            600000
        );
        assert!(settings.to_string().contains("屏幕共享 = 1024x576@10fps"));
    }

    #[test]
    fn 视频通话码率策略可以配置并过滤无效规则() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            video_call:
              max_width: 960
              max_height: 540
              max_frame_rate: 20
              bitrate_rules:
                - max_publishers: 4
                  max_bitrate_bps: 500000
                - max_publishers: 0
                  max_bitrate_bps: 1
                - max_publishers: 1
                  max_bitrate_bps: 900000
            "#,
        )
        .expect("解析视频通话配置");

        assert_eq!(settings.video_call.max_width, 960);
        assert_eq!(settings.video_call.max_height, 540);
        assert_eq!(settings.video_call.max_frame_rate, 20);
        assert_eq!(settings.video_call.bitrate_rules.len(), 2);
        assert_eq!(settings.video_call.bitrate_rules[0].max_publishers, 1);
        assert_eq!(
            settings.video_call.bitrate_rules[0].max_bitrate_bps,
            900000
        );
        assert!(settings.to_string().contains("视频通话 = 960x540@20fps"));
    }

    #[test]
    fn 视频通话码率策略为空时使用默认值() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            video_call:
              bitrate_rules: []
            "#,
        )
        .expect("解析空视频码率配置");

        assert_eq!(settings.video_call.bitrate_rules.len(), 3);
        assert_eq!(settings.video_call.bitrate_rules[0].max_publishers, 1);
        assert_eq!(
            settings.video_call.bitrate_rules[2].max_bitrate_bps,
            300000
        );
    }

    #[test]
    fn 配置日志展示使用中文字段名() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            room:
              max_members: 12
              disconnect_grace_seconds: 45
              chat_history_limit: 25
            media:
              udp_port_min: 41000
              udp_port_max: 41015
            "#,
        )
        .expect("解析配置");

        let display = settings.to_string();
        assert!(display.contains("监听端口 = 9000"));
        assert!(display.contains("房间人数上限 = 12"));
        assert!(display.contains("断线保留秒数 = 45"));
        assert!(display.contains("聊天历史条数 = 25"));
        assert!(display.contains("媒体 UDP 端口范围 = 41000-41015"));
        assert!(display.contains("对外媒体 IP = 未配置"));
        assert!(display.contains("视频通话 = 640x360@15fps"));
    }

    #[test]
    fn 认证配置默认关闭且使用_sqlite_存储() {
        let settings: Settings = serde_yaml::from_str("port: 9000").expect("解析最小配置");

        assert!(!settings.auth.enabled);
        assert_eq!(settings.auth.session.cookie_name, "remote_voice_session");
        assert_eq!(settings.auth.session.ttl_hours, 168);
        assert_eq!(settings.storage.sqlite.path, "remote-voice.db");
    }

    #[test]
    fn 认证开启时必须配置管理员() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            auth:
              enabled: true
            "#,
        )
        .expect("解析认证配置");

        assert!(settings.auth.enabled);
        assert!(settings.auth.admin.is_none());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn 认证开启且配置管理员时校验通过() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            auth:
              enabled: true
              admin:
                username: admin
                password_hash: "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$c29tZWhhc2g"
                display_name: 管理员
            storage:
              sqlite:
                path: /tmp/remote-voice-auth-test.db
            "#,
        )
        .expect("解析认证配置");

        assert!(settings.validate().is_ok());
        assert_eq!(
            settings.auth.admin.as_ref().expect("管理员配置").username,
            "admin"
        );
        assert_eq!(
            settings.storage.sqlite.path,
            "/tmp/remote-voice-auth-test.db"
        );
    }
}
