//! Bounded blocking Argon2 work, isolated from asynchronous request execution.

use std::sync::Arc;

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use tokio::sync::Semaphore;

use crate::error::AppError;

#[derive(Clone)]
pub struct PasswordService {
    permits: Arc<Semaphore>,
}

impl Default for PasswordService {
    fn default() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(4)),
        }
    }
}

impl PasswordService {
    /// Validate password length and hash it on a bounded blocking worker.
    pub async fn hash(&self, password: &str) -> Result<String, AppError> {
        if !(12..=128).contains(&password.chars().count()) {
            return Err(AppError::invalid("密码需要 12–128 个字符"));
        }
        // Keep the permit inside the blocking closure, including cancellation of its caller.
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AppError::unavailable())?;

        let password = password.to_owned();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            Argon2::default()
                .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                .map(|h| h.to_string())
                .map_err(|_| AppError::unavailable())
        })
        .await
        .map_err(|_| AppError::unavailable())?
    }

    /// Verify bounded input without blocking a Tokio executor thread.
    pub async fn verify(&self, password: &str, hash: &str) -> Result<bool, AppError> {
        if !(12..=128).contains(&password.chars().count()) {
            return Ok(false);
        }
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AppError::unavailable())?;
        let password = password.to_owned();
        let hash = hash.to_owned();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let parsed = PasswordHash::new(&hash).map_err(|_| AppError::unavailable())?;
            Ok(Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok())
        })
        .await
        .map_err(|_| AppError::unavailable())?
    }
}
