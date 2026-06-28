use crate::{Error, Result};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| Error::Internal(format!("密码哈希失败: {error}")))
}

pub fn verify_password(password_hash: &str, password: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(password_hash)
        .map_err(|error| Error::Internal(format!("密码哈希格式无效: {error}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_密码_hash_可以校验原文并拒绝错误密码() {
        let hash = hash_password("correct horse battery staple").expect("生成密码 hash");

        assert!(verify_password(&hash, "correct horse battery staple").expect("校验正确密码"));
        assert!(!verify_password(&hash, "wrong password").expect("校验错误密码"));
    }
}
