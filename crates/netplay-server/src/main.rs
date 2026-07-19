//! Netplay relay server binary. Binds an address and serves connections.
//!
//! Auth: named accounts (argon2id) in the database gate the admin surface by
//! role; the shared token (`NETPLAY_TOKENS`, or the dev default) authorizes
//! anonymous players. Seed/rotate the admin with `NETPLAY_ADMIN="name:password"`.
//!
//! Persistent state lives in a SQLite database at `NETPLAY_DB` (default
//! `netplay.db`); it's created and migrated on startup.

use std::sync::Arc;

use netplay_server::auth::{DbAuth, SharedTokenAuth};
use netplay_server::store;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:5000".to_string());

    // Open + migrate the database up front; fail fast if the store is unusable.
    let db_path = std::env::var("NETPLAY_DB").unwrap_or_else(|_| "netplay.db".to_string());
    let pool = store::open(&db_path)
        .await
        .unwrap_or_else(|e| panic!("failed to open database {db_path}: {e}"));

    // Seed (or rotate) the admin account from NETPLAY_ADMIN="name:password".
    if let Ok(spec) = std::env::var("NETPLAY_ADMIN") {
        match spec.split_once(':') {
            Some((name, password)) if !name.trim().is_empty() && !password.is_empty() => {
                let name = name.trim();
                match store::upsert_admin(&pool, name, password).await {
                    Ok(()) => println!("seeded admin account '{name}'"),
                    Err(e) => eprintln!("failed to seed admin '{name}': {e}"),
                }
            }
            _ if !spec.trim().is_empty() => {
                eprintln!("NETPLAY_ADMIN must be \"name:password\"; ignoring");
            }
            _ => {}
        }
    }

    match store::user_count(&pool).await {
        Ok(n) => println!("store ready at {db_path} ({n} account(s))"),
        Err(e) => eprintln!("store query failed: {e}"),
    }

    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    // DB-backed auth: named accounts by role, shared token for anonymous players.
    let auth = Arc::new(DbAuth::new(pool, SharedTokenAuth::from_env_or_dev()));
    println!("netplay relay listening on {addr}");
    netplay_server::serve(listener, auth).await;
}
