//! WebSocket authorization — the seam, not the mechanism.
//!
//! Gameplay auth is now token-based: the client obtains a bearer token from the
//! REST auth endpoints ([`crate::player`]) and presents it in `Hello`. The relay
//! depends only on *"is this token valid, and for which account/role?"*, never on
//! how the token was obtained. [`Authenticator::verify`] runs after the
//! protocol-version check and before the client joins the lobby; on failure the
//! connection is rejected. [`DbAuth`] looks the session up in the store. A
//! different token-issuing scheme (platform attestation, OAuth) can reuse this
//! trait unchanged — it only ever hands the socket a token.

use sqlx::SqlitePool;

use crate::store::{self, Role};

/// What `verify` returns on success: the account's authorization role (which
/// gates nothing on the game socket today, but travels with the identity) and its
/// display name, derived from the token — never from client-supplied input.
#[derive(Clone, Debug)]
pub struct Identity {
    pub role: Role,
    pub name: String,
}

/// Why gameplay authorization failed. The message is sent to the client before
/// closing the socket. Registration/login errors live on the REST side now; the
/// socket only ever rejects a missing, invalid, or expired token.
#[derive(Clone, Copy, Debug)]
pub enum AuthError {
    /// The token was absent, unknown, or expired.
    Unauthorized,
}

impl AuthError {
    pub fn message(self) -> &'static str {
        match self {
            AuthError::Unauthorized => "invalid or expired session token — log in again",
        }
    }
}

/// Minimum length for a self-registered password (enforced by the REST register
/// handler; kept here as the one source of truth for the account policy).
pub const MIN_PASSWORD_LEN: usize = 8;

/// Authorizes a connecting client from the bearer token in its `Hello`. Async
/// because the DB-backed implementation looks the session up.
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    async fn verify(&self, token: &str) -> Result<Identity, AuthError>;
}

/// Token-validating authenticator: the `Hello` token must name a live session.
pub struct DbAuth {
    pool: SqlitePool,
}

impl DbAuth {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl Authenticator for DbAuth {
    async fn verify(&self, token: &str) -> Result<Identity, AuthError> {
        match store::session_account(&self.pool, token).await {
            Ok(Some((_, role, name))) => Ok(Identity { role, name }),
            Ok(None) | Err(_) => Err(AuthError::Unauthorized),
        }
    }
}
