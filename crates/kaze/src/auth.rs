//! Signing in: the local account's password and browser sessions.

use std::fmt::{self, Write as _};

use argon2::{
    Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _, password_hash::SaltString,
};
use chrono::{TimeDelta, Utc};
use sha2::{Digest as _, Sha256};
use sqlx::SqlitePool;

use crate::db;

/// How long a sign-in lasts.
pub const SESSION_DAYS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("hashing the password: {0}")]
    Hash(argon2::password_hash::Error),
    #[error("generating a session token: {0}")]
    Random(#[from] getrandom::Error),
    #[error("password hashing task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// A password good enough to set.
pub struct NewPassword(String);

impl NewPassword {
    pub const MIN_LEN: usize = 8;

    #[must_use]
    pub fn new(password: String) -> Option<Self> {
        (password.chars().count() >= Self::MIN_LEN).then_some(Self(password))
    }

    /// The argon2 hash to store.
    pub async fn hash(self) -> Result<String, AuthError> {
        // Argon2 takes tens of milliseconds of CPU on purpose: keep it off
        // the async workers.
        tokio::task::spawn_blocking(move || {
            let mut salt = [0u8; 16];
            getrandom::fill(&mut salt)?;
            let salt = SaltString::encode_b64(&salt).map_err(AuthError::Hash)?;
            Argon2::default()
                .hash_password(self.0.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(AuthError::Hash)
        })
        .await?
    }
}

impl fmt::Debug for NewPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NewPassword(..)")
    }
}

/// Whether `password` matches the stored `hash`.
pub async fn verify(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

/// The secret in a browser's session cookie.
pub struct SessionToken(String);

impl SessionToken {
    fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })))
    }

    /// Whatever a cookie holds; an unknown token just matches no session.
    #[must_use]
    pub fn from_cookie(value: &str) -> Self {
        Self(value.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What the database keeps instead of the token.
    #[must_use]
    pub fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0.as_bytes()).to_vec()
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(..)")
    }
}

/// Sign a browser in.
pub async fn start_session(db: &SqlitePool) -> Result<SessionToken, AuthError> {
    let token = SessionToken::generate()?;
    let now = Utc::now();
    db::auth::prune_sessions(db, now).await?;
    let expires = now + TimeDelta::days(SESSION_DAYS);
    db::auth::create_session(db, &token.hash(), now, expires).await?;
    Ok(token)
}

pub async fn session_valid(db: &SqlitePool, token: &SessionToken) -> sqlx::Result<bool> {
    let expires = db::auth::session_expiry(db, &token.hash()).await?;
    Ok(expires.is_some_and(|expires| expires > Utc::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_passwords_are_refused() {
        assert!(NewPassword::new("1234567".into()).is_none());
        assert!(NewPassword::new("12345678".into()).is_some());
    }

    #[tokio::test]
    async fn hashes_verify_only_their_password() {
        let hash = NewPassword::new("correct horse".into())
            .unwrap()
            .hash()
            .await
            .unwrap();
        assert!(verify("correct horse".into(), hash.clone()).await);
        assert!(!verify("wrong horse".into(), hash).await);
        assert!(!verify("anything".into(), "not a hash".into()).await);
    }

    #[tokio::test]
    async fn sessions_expire() {
        let db = db::memory().await;
        let token = start_session(&db).await.unwrap();
        assert!(session_valid(&db, &token).await.unwrap());
        assert!(
            !session_valid(&db, &SessionToken::from_cookie("forged"))
                .await
                .unwrap()
        );
        sqlx::query("UPDATE sessions SET expires_at = '2000-01-01T00:00:00Z'")
            .execute(&db)
            .await
            .unwrap();
        assert!(!session_valid(&db, &token).await.unwrap());
        start_session(&db).await.unwrap();
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(left, 1, "expired sessions are pruned");
    }
}
