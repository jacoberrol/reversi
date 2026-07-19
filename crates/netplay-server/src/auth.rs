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
    /// Login failed: no such account or wrong password.
    BadLogin,
    /// Registration failed: the name is already taken.
    NameTaken,
    /// Registration failed: the password is too short.
    WeakPassword,
}

impl AuthError {
    pub fn message(self) -> &'static str {
        match self {
            AuthError::Malformed => "malformed credential",
            AuthError::UnknownKey => "unknown key id",
            AuthError::BadToken => "invalid token",
            AuthError::BadLogin => "wrong name or password",
            AuthError::NameTaken => "that name is taken",
            AuthError::WeakPassword => "password too short (min 8 characters)",
        }
    }
}

/// Minimum length for a self-registered password.
pub const MIN_PASSWORD_LEN: usize = 8;

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

/// Accounts-only authenticator. Every connection must present an account
/// credential `{name, password}` (login) or `{name, password, register: true}`
/// (create the account, then log in). argon2id-verified against the DB.
pub struct DbAuth {
    pool: SqlitePool,
}

impl DbAuth {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// A login (`register` false/absent) or registration (`register` true) credential.
#[derive(serde::Deserialize)]
struct AccountCredential {
    name: String,
    password: String,
    #[serde(default)]
    register: bool,
}

#[async_trait::async_trait]
impl Authenticator for DbAuth {
    async fn verify(&self, credential: &serde_json::Value) -> Result<Identity, AuthError> {
        let account = serde_json::from_value::<AccountCredential>(credential.clone())
            .map_err(|_| AuthError::Malformed)?;
        if account.name.trim().is_empty() {
            return Err(AuthError::Malformed);
        }
        let label = format!("account {}", account.name);

        if account.register {
            if account.password.len() < MIN_PASSWORD_LEN {
                return Err(AuthError::WeakPassword);
            }
            match store::create_account(&self.pool, &account.name, &account.password).await {
                Ok(role) => Ok(Identity { role, label }),
                Err(store::CreateError::NameTaken) => Err(AuthError::NameTaken),
                Err(store::CreateError::Db(_)) => Err(AuthError::Malformed),
            }
        } else {
            match store::verify_account(&self.pool, &account.name, &account.password).await {
                Ok(Some(role)) => Ok(Identity { role, label }),
                Ok(None) => Err(AuthError::BadLogin),
                Err(_) => Err(AuthError::Malformed),
            }
        }
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
