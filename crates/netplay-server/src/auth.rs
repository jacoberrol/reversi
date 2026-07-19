//! Client authorization — the seam, not the mechanism.
//!
//! The relay depends only on *"was this connection authorized, and as what
//! role?"*, never on *how*. [`Authenticator::verify`] runs after the
//! protocol-version check and before the client joins the lobby; on failure the
//! connection is rejected. [`DbAuth`] is the implementation: accounts-only, with
//! login and open self-registration (argon2id). A platform-attestation
//! authenticator could swap in later behind the same trait with no relay changes.

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
    /// The credential wasn't a valid account login/registration shape.
    Malformed,
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
