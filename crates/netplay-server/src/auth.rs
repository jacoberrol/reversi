//! WebSocket authorization: validate the bearer token a client presents in
//! `Hello` and resolve it to an account.
//!
//! Credentials never touch the socket — players log in / register over REST
//! ([`crate::player`]) for a token, and the relay only asks "does this token
//! name a live session, and for which account?". That REST boundary is the auth
//! seam now: a new scheme (attestation, OAuth) would change how tokens are
//! *minted*, while the socket contract stays "present a valid token". (An
//! earlier design kept an `Authenticator` trait here so credential-checking
//! implementations could swap behind the socket; token-based auth made the
//! trait ceremony with a single implementation, so it was removed.)

use sqlx::SqlitePool;

use crate::store::{self, Role};

/// The account behind a validated token.
#[derive(Clone, Debug)]
pub struct Identity {
    /// Kept deliberately although nothing on the game socket reads it today:
    /// it travels with the identity so a role-gated gameplay feature can use it
    /// without touching the auth path. (RBAC currently matters only on the
    /// admin REST surface.)
    pub role: Role,
    /// The account's display name — from the database, never from the client.
    pub name: String,
}

/// Sent to the client when its token is missing, unknown, or expired.
pub const UNAUTHORIZED_MESSAGE: &str = "invalid or expired session token — log in again";

/// Token validator for the game socket.
pub struct DbAuth {
    pool: SqlitePool,
}

impl DbAuth {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The identity a bearer token authorizes, or `None` if the token names no
    /// live session (the relay rejects the connection with
    /// [`UNAUTHORIZED_MESSAGE`]).
    pub async fn verify(&self, token: &str) -> Option<Identity> {
        match store::session_account(&self.pool, token).await {
            Ok(Some((_, role, name))) => Some(Identity { role, name }),
            Ok(None) | Err(_) => None,
        }
    }
}
