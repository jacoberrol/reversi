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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_migrates_and_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = open(path.to_str().unwrap()).await.expect("open");
        // Migrations ran (the table exists) and there are no accounts yet.
        assert_eq!(user_count(&pool).await.expect("count"), 0);
    }
}
