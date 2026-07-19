//! Netplay relay server binary. Binds an address and serves connections.
//!
//! Auth is accounts-only: clients log in (or self-register) with a name +
//! password (argon2id), and the account's role gates the admin surface. Seed
//! the admin with `NETPLAY_ADMIN="name:password"`.
//!
//! Persistent state lives in a SQLite database at `NETPLAY_DB` (default
//! `netplay.db`); it's created and migrated on startup.

use std::sync::Arc;

use netplay_server::auth::DbAuth;
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
    // Accounts-only: every connection logs in or registers.
    let auth = Arc::new(DbAuth::new(pool));
    println!("netplay relay listening on {addr}");
    netplay_server::serve(listener, auth).await;
}
