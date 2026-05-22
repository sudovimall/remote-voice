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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSettings {
    #[serde(default = "default_max_members")]
    pub max_members: usize,
    #[serde(default = "default_disconnect_grace_seconds")]
    pub disconnect_grace_seconds: u64,
}

impl Default for RoomSettings {
    fn default() -> Self {
        Self {
            max_members: default_max_members(),
            disconnect_grace_seconds: default_disconnect_grace_seconds(),
        }
    }
}

fn default_max_members() -> usize {
    8
}

fn default_disconnect_grace_seconds() -> u64 {
    30
}

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[port = {}, room.max_members = {}, room.disconnect_grace_seconds = {}]",
            self.port, self.room.max_members, self.room.disconnect_grace_seconds
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
    }
}
