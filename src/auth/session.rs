use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, distr::Alphanumeric};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn new_session_token() -> String {
    prefixed_secret("s_", 48)
}

pub fn new_invite_code() -> String {
    prefixed_secret("i_", 32)
}

pub fn hash_secret(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn prefixed_secret(prefix: &str, len: usize) -> String {
    let mut rng = rand::rng();
    let suffix: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect();
    format!("{prefix}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_token_hash_稳定且不等于原文() {
        let token = "session-secret";

        let first = hash_secret(token);
        let second = hash_secret(token);

        assert_eq!(first, second);
        assert_ne!(first, token);
    }
}
