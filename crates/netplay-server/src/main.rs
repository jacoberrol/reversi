//! Netplay relay server binary. Binds an address and serves connections.

use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:5000".to_string());
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("netplay relay listening on {addr}");
    netplay_server::serve(listener).await;
}
