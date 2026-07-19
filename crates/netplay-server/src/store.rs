//! Persistent state (SQLite via `sqlx`).
//!
//! This is where the relay stops being purely in-memory: accounts and their
//! roles live here so the admin surface can be gated (RBAC). [`open`] creates
//! the database if missing and runs any pending migrations (embedded at build
//! time from `migrations/`), so a deploy stays one step — the server migrates
//! itself on startup.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// Open (creating if absent) the SQLite database at `path` and run migrations.
pub async fn open(path: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// How many accounts exist (a cheap health check of the store).
pub async fn user_count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// An account's authorization role. Gates the admin/control surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Player,
    Admin,
}

impl Role {
    fn from_db(s: &str) -> Self {
        if s == "admin" {
            Role::Admin
        } else {
            Role::Player
        }
    }
}

/// argon2id hash of `password` as a self-contained PHC string (salt embedded).
/// `Argon2::default()` is argon2id with OWASP params (~19 MiB, t=2, p=1).
fn hash_password(password: &str) -> String {
    use argon2::password_hash::{rand_core::OsRng, SaltString};
    use argon2::{Argon2, PasswordHasher};
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hashing never fails for a valid password")
        .to_string()
}

/// Why creating an account failed.
#[derive(Debug)]
pub enum CreateError {
    /// The name is already registered.
    NameTaken,
    Db(sqlx::Error),
}

/// Register a new `player` account. Errors with [`CreateError::NameTaken`] if the
/// name is taken (the `UNIQUE` constraint), else the DB error.
pub async fn create_account(
    pool: &SqlitePool,
    name: &str,
    password: &str,
) -> Result<Role, CreateError> {
    let hash = hash_password(password);
    let result =
        sqlx::query("INSERT INTO users (name, password_hash, role) VALUES (?, ?, 'player')")
            .bind(name)
            .bind(hash)
            .execute(pool)
            .await;
    match result {
        Ok(_) => Ok(Role::Player),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(CreateError::NameTaken),
        Err(e) => Err(CreateError::Db(e)),
    }
}

/// Create the admin account (or reset its password + role if it already exists).
/// Idempotent, so rotating the admin password is `NETPLAY_ADMIN` change + redeploy.
pub async fn upsert_admin(
    pool: &SqlitePool,
    name: &str,
    password: &str,
) -> Result<(), sqlx::Error> {
    let hash = hash_password(password);
    sqlx::query(
        "INSERT INTO users (name, password_hash, role) VALUES (?, ?, 'admin') \
         ON CONFLICT(name) DO UPDATE SET password_hash = excluded.password_hash, role = 'admin'",
    )
    .bind(name)
    .bind(hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// Verify an account login, returning its `(id, role)` on success (or `None` for
/// an unknown name or wrong password). The argon2 verify runs on a blocking
/// thread so it never stalls the async runtime.
pub async fn verify_account(
    pool: &SqlitePool,
    name: &str,
    password: &str,
) -> Result<Option<(i64, Role)>, sqlx::Error> {
    let row: Option<(i64, String, String)> =
        sqlx::query_as("SELECT id, password_hash, role FROM users WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    let Some((id, hash, role)) = row else {
        return Ok(None);
    };
    let password = password.to_string();
    let verified = tokio::task::spawn_blocking(move || {
        use argon2::password_hash::PasswordHash;
        use argon2::{Argon2, PasswordVerifier};
        PasswordHash::new(&hash)
            .map(|parsed| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    Ok(verified.then(|| (id, Role::from_db(&role))))
}

/// A random high-entropy bearer token (hex).
fn random_token() -> String {
    use argon2::password_hash::rand_core::{OsRng, RngCore};
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    to_hex(&bytes)
}

/// Lowercase-hex a byte slice.
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// sha256 hex of a string (for hashing high-entropy session tokens at rest).
fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    to_hex(&Sha256::digest(s.as_bytes()))
}

/// Create an admin session valid for `ttl_hours`, returning the raw bearer token
/// (only its sha256 is stored).
pub async fn create_session(
    pool: &SqlitePool,
    user_id: i64,
    role: Role,
    ttl_hours: i64,
) -> Result<String, sqlx::Error> {
    let token = random_token();
    let role_str = if role == Role::Admin {
        "admin"
    } else {
        "player"
    };
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, role, expires_at) \
         VALUES (?, ?, ?, datetime('now', ?))",
    )
    .bind(sha256_hex(&token))
    .bind(user_id)
    .bind(role_str)
    .bind(format!("{ttl_hours:+} hours"))
    .execute(pool)
    .await?;
    Ok(token)
}

/// The `(user_id, role)` a bearer token authorizes, or `None` if unknown/expired.
/// Callers that only need the role can ignore the id; the id lets an authenticated
/// caller mint a fresh session for the same user.
///
/// Validation is a **pure read**: an expired token is never honored (the query
/// filters on `expires_at`), but the dead row is left in place — reclaiming it is
/// an operator action ([`prune_expired_sessions`]), not something every request pays for.
pub async fn session_identity(
    pool: &SqlitePool,
    token: &str,
) -> Result<Option<(i64, Role)>, sqlx::Error> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT user_id, role FROM sessions WHERE token_hash = ? AND expires_at > datetime('now')",
    )
    .bind(sha256_hex(token))
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, role)| (id, Role::from_db(&role))))
}

/// Delete every session whose TTL has passed, returning how many were removed.
/// Because [`session_identity`] never honors an expired token, this only reclaims
/// storage — it's an operator action (the `prune-tokens` subcommand), deliberately
/// kept off the request path rather than run on a timer or on every lookup.
pub async fn prune_expired_sessions(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= datetime('now')")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_pool() -> (SqlitePool, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = open(path.to_str().unwrap()).await.expect("open");
        (pool, dir)
    }

    #[tokio::test]
    async fn open_migrates_and_starts_empty() {
        let (pool, _dir) = temp_pool().await;
        // Migrations ran (the table exists) and there are no accounts yet.
        assert_eq!(user_count(&pool).await.expect("count"), 0);
    }

    #[tokio::test]
    async fn admin_account_round_trips() {
        let (pool, _dir) = temp_pool().await;
        upsert_admin(&pool, "root", "s3cret").await.unwrap();
        assert_eq!(user_count(&pool).await.unwrap(), 1);

        let role = |o: Option<(i64, Role)>| o.map(|(_, r)| r);
        assert_eq!(
            role(verify_account(&pool, "root", "s3cret").await.unwrap()),
            Some(Role::Admin)
        );
        assert_eq!(verify_account(&pool, "root", "wrong").await.unwrap(), None);
        assert_eq!(
            verify_account(&pool, "ghost", "s3cret").await.unwrap(),
            None
        );

        // Upsert is idempotent and resets the password (no duplicate row).
        upsert_admin(&pool, "root", "newpass").await.unwrap();
        assert_eq!(user_count(&pool).await.unwrap(), 1);
        assert_eq!(
            role(verify_account(&pool, "root", "newpass").await.unwrap()),
            Some(Role::Admin)
        );
    }

    #[tokio::test]
    async fn create_account_registers_a_player_and_rejects_duplicates() {
        let (pool, _dir) = temp_pool().await;
        let created = create_account(&pool, "alice", "password").await.unwrap();
        assert_eq!(created, Role::Player);
        assert_eq!(
            verify_account(&pool, "alice", "password")
                .await
                .unwrap()
                .map(|(_, r)| r),
            Some(Role::Player)
        );
        // The name is taken now.
        assert!(matches!(
            create_account(&pool, "alice", "other").await,
            Err(CreateError::NameTaken)
        ));
    }

    #[tokio::test]
    async fn sessions_authorize_until_they_expire() {
        let (pool, _dir) = temp_pool().await;
        upsert_admin(&pool, "root", "s3cret").await.unwrap();
        let (id, role) = verify_account(&pool, "root", "s3cret")
            .await
            .unwrap()
            .unwrap();

        let token = create_session(&pool, id, role, 24).await.unwrap();
        assert_eq!(
            session_identity(&pool, &token).await.unwrap(),
            Some((id, Role::Admin))
        );
        assert_eq!(session_identity(&pool, "not-a-token").await.unwrap(), None);

        // An already-expired session is never honored (validation filters on TTL),
        // even though the row still exists until an operator prunes it.
        let expired = create_session(&pool, id, role, -1).await.unwrap();
        assert_eq!(session_identity(&pool, &expired).await.unwrap(), None);
    }

    #[tokio::test]
    async fn prune_removes_only_expired_sessions() {
        let (pool, _dir) = temp_pool().await;
        upsert_admin(&pool, "root", "s3cret").await.unwrap();
        let (id, role) = verify_account(&pool, "root", "s3cret")
            .await
            .unwrap()
            .unwrap();

        let live = create_session(&pool, id, role, 24).await.unwrap();
        create_session(&pool, id, role, -1).await.unwrap();
        create_session(&pool, id, role, -5).await.unwrap();

        // Only the two expired rows are reclaimed; the live token still works.
        assert_eq!(prune_expired_sessions(&pool).await.unwrap(), 2);
        assert_eq!(
            session_identity(&pool, &live).await.unwrap(),
            Some((id, Role::Admin))
        );
        // Idempotent: nothing left to prune.
        assert_eq!(prune_expired_sessions(&pool).await.unwrap(), 0);
    }
}
