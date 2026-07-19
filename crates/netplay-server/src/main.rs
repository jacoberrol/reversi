//! Netplay relay server binary. Binds an address and serves connections.
//!
//! Authorizes clients with a shared token: set `NETPLAY_TOKENS="id:token,..."`,
//! or it falls back to the development key (`netplay_protocol::DEV_*`).
//!
//! Persistent state lives in a SQLite database at `NETPLAY_DB` (default
//! `netplay.db`); it's created and migrated on startup.

use std::sync::Arc;

use netplay_server::auth::SharedTokenAuth;
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
    match store::user_count(&pool).await {
        Ok(n) => println!("store ready at {db_path} ({n} account(s))"),
        Err(e) => eprintln!("store query failed: {e}"),
    }

    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    let auth = Arc::new(SharedTokenAuth::from_env_or_dev());
    println!("netplay relay listening on {addr}");
    netplay_server::serve(listener, auth).await;
}
