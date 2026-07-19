//! Netplay relay server binary. Binds an address and serves connections.
//!
//! Gameplay is accounts-only WebSocket: clients log in (or self-register) with a
//! name + password (argon2id). The admin REST API is served on `NETPLAY_ADMIN_HOST`
//! (default `admin.netplay.oliverj.network`); seed the admin with
//! `NETPLAY_ADMIN="name:password"`.
//!
//! Persistent state lives in a SQLite database at `NETPLAY_DB` (default
//! `netplay.db`); it's created and migrated on startup.

use netplay_server::store;
use tokio::net::TcpListener;

const DEFAULT_ADMIN_HOST: &str = "admin.netplay.oliverj.network";

#[tokio::main]
async fn main() {
    let arg = std::env::args().nth(1);
    let db_path = std::env::var("NETPLAY_DB").unwrap_or_else(|_| "netplay.db".to_string());

    // `prune-tokens`: reclaim expired admin sessions, then exit. An operator
    // action — validation never prunes (see store::session_identity), so dead
    // rows accumulate until someone runs this against the DB.
    if arg.as_deref() == Some("prune-tokens") {
        let pool = store::open(&db_path)
            .await
            .unwrap_or_else(|e| panic!("failed to open database {db_path}: {e}"));
        match store::prune_expired_sessions(&pool).await {
            Ok(n) => println!("pruned {n} expired session(s) from {db_path}"),
            Err(e) => {
                eprintln!("prune failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let addr = arg.unwrap_or_else(|| "127.0.0.1:5000".to_string());

    // Open + migrate the database up front; fail fast if the store is unusable.
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

    let admin_host =
        std::env::var("NETPLAY_ADMIN_HOST").unwrap_or_else(|_| DEFAULT_ADMIN_HOST.to_string());

    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("netplay relay listening on {addr} (admin host: {admin_host})");
    netplay_server::serve(listener, pool, admin_host).await;
}
