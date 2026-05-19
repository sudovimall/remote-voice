use crate::R;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// 外部连接配置的主机ip
    pub port: u16,
    pub static_dir: String,
    pub index_file: String,
    pub template_extensions: Vec<String>,
    #[serde(default)]
    pub template_values: HashMap<String, String>,
}
impl Display for VoiceConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[port = {}, static_dir = {}, index_file = {}, template_extensions = {:?}]",
            self.port, self.static_dir, self.index_file, self.template_extensions
        )?;
        Ok(())
    }
}

pub fn init_config() -> R<VoiceConfig> {
    let s = std::fs::read_to_string("application.yaml").unwrap_or_else(|_| {
        r#"
            port: 8080
            static_dir: "static"
            index_file: "index.html"
            template_extensions:
              - "js"
            template_values:
              title: "Voice"
              api_base: "http://localhost:8080"
              message: "hello from config"
           "#
            .to_string()
    });
    let config = serde_yaml::from_str::<VoiceConfig>(s.as_str())?;
    info!(%config, "VoiceConfig:");
    Ok(config)
}
