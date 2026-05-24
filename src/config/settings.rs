use crate::Result;
use serde::{Deserialize, Serialize};
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

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[监听端口 = {}, 房间人数上限 = {}, 断线保留秒数 = {}, 聊天历史条数 = {}, 媒体 UDP 端口范围 = {}-{}, 对外媒体 IP = {}, 屏幕共享 = {}x{}@{}fps]",
            self.port,
            self.room.max_members,
            self.room.disconnect_grace_seconds,
            self.room.chat_history_limit,
            self.media.udp_port_min,
            self.media.udp_port_max,
            self.media.public_ip.as_deref().unwrap_or("未配置"),
            self.screen_share.max_width,
            self.screen_share.max_height,
            self.screen_share.max_frame_rate
        )?;
        Ok(())
    }
}

pub fn init_config() -> Result<Settings> {
    let s = std::fs::read_to_string("application.yaml").unwrap_or_else(|_| {
        r#"
            port: 8080
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
           "#
        .to_string()
    });
    let config = serde_yaml::from_str::<Settings>(s.as_str())?;
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
    }
}
