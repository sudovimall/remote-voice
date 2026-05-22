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
            "[监听端口 = {}, 房间人数上限 = {}, 断线保留秒数 = {}, 媒体 UDP 端口范围 = {}-{}, 对外媒体 IP = {}]",
            self.port,
            self.room.max_members,
            self.room.disconnect_grace_seconds,
            self.media.udp_port_min,
            self.media.udp_port_max,
            self.media.public_ip.as_deref().unwrap_or("未配置")
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
                .contains("媒体 UDP 端口范围 = 41000-41015")
        );
        assert!(settings.to_string().contains("对外媒体 IP = 111.228.39.21"));
    }

    #[test]
    fn 配置日志展示使用中文字段名() {
        let settings: Settings = serde_yaml::from_str(
            r#"
            port: 9000
            room:
              max_members: 12
              disconnect_grace_seconds: 45
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
        assert!(display.contains("媒体 UDP 端口范围 = 41000-41015"));
        assert!(display.contains("对外媒体 IP = 未配置"));
    }
}
