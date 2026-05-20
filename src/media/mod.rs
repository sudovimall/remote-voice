use crate::{Error, Result};

#[derive(Debug, Default)]
pub struct MediaController;

impl MediaController {
    pub fn new() -> Self {
        Self
    }

    pub async fn handle_offer(
        &self,
        _room_id: &str,
        _member_id: &str,
        _sdp: String,
    ) -> Result<String> {
        Err(Error::MediaNotReady)
    }

    pub async fn add_ice_candidate(
        &self,
        _room_id: &str,
        _member_id: &str,
        _candidate: String,
    ) -> Result<()> {
        Err(Error::MediaNotReady)
    }

    pub async fn close_member(&self, _room_id: &str, _member_id: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MediaController;
    use crate::Error;

    #[tokio::test]
    async fn 占位媒体控制器返回_media_not_ready() {
        let media = MediaController::new();

        let err = media
            .handle_offer("room-1", "member-1", "v=0".to_string())
            .await
            .expect_err("真实媒体层接入前不生成 answer");

        assert!(matches!(err, Error::MediaNotReady));
    }
}
