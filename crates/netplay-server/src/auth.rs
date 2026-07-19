//! Client authorization — the seam, not the token.
//!
//! The relay depends only on *"was this connection authorized?"*, never on
//! *how*. [`Authenticator::verify`] runs after the protocol-version check and
//! before the client joins the lobby; on failure the connection is rejected.
//! [`SharedTokenAuth`] is the reference implementation (a versioned shared
//! token); a platform-attestation authenticator can be swapped in later behind
//! the same trait with no relay changes.
//!
//! Honest threat model: a client cannot keep a secret, so this deters anonymous
//! clients and casual spam — it is not tamper-proofing.

use std::collections::HashMap;

use netplay_protocol::SharedTokenCredential;
use sqlx::SqlitePool;

use crate::store::{self, Role};

/// What `verify` returns on success: the connection's authorization role (which
/// gates the admin surface) plus a short label for logs.
#[derive(Clone, Debug)]
pub struct Identity {
    pub role: Role,
    /// How this connection authorized, for logs (e.g. "key 2" or "account root").
    pub label: String,
}

/// Why authorization failed. The message is sent to the client before closing.
#[derive(Clone, Copy, Debug)]
pub enum AuthError {
    Malformed,
    UnknownKey,
    BadToken,
}

impl AuthError {
    pub fn message(self) -> &'static str {
        match self {
            AuthError::Malformed => "malformed credential",
            AuthError::UnknownKey => "unknown key id",
            AuthError::BadToken => "invalid token",
        }
    }
}

/// Authorizes a connecting client from its opaque credential (arbitrary JSON the
/// implementation interprets; the relay never inspects it). Async because a
/// DB-backed implementation looks the account up.
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    async fn verify(&self, credential: &serde_json::Value) -> Result<Identity, AuthError>;
}

/// Reference authenticator: accept a versioned shared token. Holds a *set* of
/// valid keys (by id) so `N` and `N+1` can be accepted during a rotation.
pub struct SharedTokenAuth {
    keys: HashMap<u16, String>,
}

impl SharedTokenAuth {
    pub fn new(keys: HashMap<u16, String>) -> Self {
        Self { keys }
    }

    /// The development default key (matches `netplay_protocol::DEV_*`).
    pub fn dev() -> Self {
        let mut keys = HashMap::new();
        keys.insert(
            netplay_protocol::DEV_KEY_ID,
            netplay_protocol::DEV_TOKEN.to_string(),
        );
        Self::new(keys)
    }

    /// Parse `NETPLAY_TOKENS` (`"id:token,id:token,..."`) if set, else the dev
    /// default. Lets you rotate keys without a code change.
    pub fn from_env_or_dev() -> Self {
        match std::env::var("NETPLAY_TOKENS") {
            Ok(spec) if !spec.trim().is_empty() => {
                let mut keys = HashMap::new();
                for entry in spec.split(',') {
                    if let Some((id, token)) = entry.split_once(':') {
                        if let Ok(id) = id.trim().parse::<u16>() {
                            keys.insert(id, token.trim().to_string());
                        }
                    }
                }
                Self::new(keys)
            }
            _ => Self::dev(),
        }
    }
}

#[async_trait::async_trait]
impl Authenticator for SharedTokenAuth {
    async fn verify(&self, credential: &serde_json::Value) -> Result<Identity, AuthError> {
        let cred = SharedTokenCredential::from_value(credential).ok_or(AuthError::Malformed)?;
        match self.keys.get(&cred.key_id) {
            Some(expected) if *expected == cred.token => Ok(Identity {
                role: Role::Player,
                label: format!("key {}", cred.key_id),
            }),
            Some(_) => Err(AuthError::BadToken),
            None => Err(AuthError::UnknownKey),
        }
    }
}

/// Authenticator backed by the accounts table, with the shared token as an
/// anonymous-player fallback. A named-account login (`{name, password}`) is
/// argon2id-verified against the DB and yields that account's role; anything
/// else falls through to the shared-token (anonymous `player`) path. Removing
/// anonymous access later is just dropping the fallback.
pub struct DbAuth {
    pool: SqlitePool,
    anonymous: SharedTokenAuth,
}

impl DbAuth {
    pub fn new(pool: SqlitePool, anonymous: SharedTokenAuth) -> Self {
        Self { pool, anonymous }
    }
}

/// A named-account login credential.
#[derive(serde::Deserialize)]
struct AccountCredential {
    name: String,
    password: String,
}

#[async_trait::async_trait]
impl Authenticator for DbAuth {
    async fn verify(&self, credential: &serde_json::Value) -> Result<Identity, AuthError> {
        // A `{name, password}` shape is an account login; verify it against the DB.
        if let Ok(account) = serde_json::from_value::<AccountCredential>(credential.clone()) {
            return match store::verify_account(&self.pool, &account.name, &account.password).await {
                Ok(Some(role)) => Ok(Identity {
                    role,
                    label: format!("account {}", account.name),
                }),
                Ok(None) => Err(AuthError::BadToken),
                Err(_) => Err(AuthError::Malformed),
            };
        }
        // Otherwise, the shared-token anonymous-player path.
        self.anonymous.verify(credential).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_valid_token_rejects_others() {
        let auth = SharedTokenAuth::dev();
        let good = SharedTokenCredential {
            key_id: netplay_protocol::DEV_KEY_ID,
            token: netplay_protocol::DEV_TOKEN.into(),
        }
        .to_value();
        assert_eq!(auth.verify(&good).await.unwrap().role, Role::Player);

        let wrong_token = SharedTokenCredential {
            key_id: netplay_protocol::DEV_KEY_ID,
            token: "nope".into(),
        }
        .to_value();
        assert!(matches!(
            auth.verify(&wrong_token).await,
            Err(AuthError::BadToken)
        ));

        let unknown_key = SharedTokenCredential {
            key_id: 999,
            token: netplay_protocol::DEV_TOKEN.into(),
        }
        .to_value();
        assert!(matches!(
            auth.verify(&unknown_key).await,
            Err(AuthError::UnknownKey)
        ));

        assert!(matches!(
            auth.verify(&serde_json::json!("garbage")).await,
            Err(AuthError::Malformed)
        ));
    }
}
