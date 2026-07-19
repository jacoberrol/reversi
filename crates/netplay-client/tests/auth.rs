//! The client auth SDK against a real relay: register/login round-trips and the
//! error surface. The server runs on its own thread + runtime; the blocking auth
//! calls run on the test thread.

use std::net::SocketAddr;

use netplay_client::auth::{self, AuthError};

/// Start a relay on an ephemeral port on a background thread, returning its
/// address once it's bound. The temp DB dir lives for the process (leaked with
/// the server thread), which is fine for a test.
fn spawn_relay() -> SocketAddr {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("test.db");
            let pool = netplay_server::store::open(db.to_str().unwrap())
                .await
                .unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let _dir = dir; // keep the SQLite file alive for the server's lifetime
            netplay_server::serve(listener, pool, "admin.invalid".to_string()).await;
        });
    });
    rx.recv().unwrap()
}

#[test]
fn register_then_login_yields_tokens_and_surfaces_errors() {
    let addr = spawn_relay();
    let base = format!("http://{addr}");

    // Register returns a token; logging in with the same password returns another.
    let registered = auth::player_register(&base, "alice", "password").unwrap();
    assert!(!registered.token.is_empty());
    assert!(registered.expires_in_hours > 0);

    let logged_in = auth::player_login(&base, "alice", "password").unwrap();
    assert!(!logged_in.token.is_empty());

    // Wrong password is a Rejected (with the server's message), not a Transport error.
    let err = auth::player_login(&base, "alice", "nope").unwrap_err();
    assert!(matches!(err, AuthError::Rejected(_)), "{err:?}");

    // Re-registering a taken name is rejected too.
    let err = auth::player_register(&base, "alice", "password").unwrap_err();
    assert!(matches!(err, AuthError::Rejected(_)), "{err:?}");
}
