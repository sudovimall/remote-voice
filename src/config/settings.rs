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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSettings {
    #[serde(default = "default_max_members")]
    pub max_members: usize,
    #[serde(default = "default_disconnect_grace_seconds")]
    pub disconnect_grace_seconds: u64,
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

impl Default for RoomSettings {
    fn default() -> Self {
        Self {
            max_members: default_max_members(),
            disconnect_grace_seconds: default_disconnect_grace_seconds(),
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

fn default_max_members() -> usize {
    8
}

fn default_disconnect_grace_seconds() -> u64 {
    30
}

fn default_udp_port_min() -> u16 {
    40000
}

fn default_udp_port_max() -> u16 {
    40100
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[port = {}, room.max_members = {}, room.disconnect_grace_seconds = {}, media.udp_port_range = {}-{}, media.public_ip = {}]",
            self.port,
            self.room.max_members,
            self.room.disconnect_grace_seconds,
            self.media.udp_port_min,
            self.media.udp_port_max,
            self.media.public_ip.as_deref().unwrap_or("unset")
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
            media:
              udp_port_min: 40000
              udp_port_max: 40100
           "#
        .to_string()
    });
    let config = serde_yaml::from_str::<Settings>(s.as_str())?;
    info!(%config, "VoiceConfig:");
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
        assert_eq!(settings.media.udp_port_min, 40000);
        assert_eq!(settings.media.udp_port_max, 40100);
        assert_eq!(settings.media.public_ip, None);
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
                .contains("media.udp_port_range = 41000-41015")
        );
        assert!(
            settings
                .to_string()
                .contains("media.public_ip = 111.228.39.21")
        );
    }
}
