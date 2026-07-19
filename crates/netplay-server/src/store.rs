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

/// Verify an account login, returning its [`Role`] on success (or `None` for an
/// unknown name or wrong password). The argon2 verify runs on a blocking thread
/// so it never stalls the async runtime.
pub async fn verify_account(
    pool: &SqlitePool,
    name: &str,
    password: &str,
) -> Result<Option<Role>, sqlx::Error> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT password_hash, role FROM users WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    let Some((hash, role)) = row else {
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
    Ok(verified.then(|| Role::from_db(&role)))
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

        assert_eq!(
            verify_account(&pool, "root", "s3cret").await.unwrap(),
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
            verify_account(&pool, "root", "newpass").await.unwrap(),
            Some(Role::Admin)
        );
    }
}
