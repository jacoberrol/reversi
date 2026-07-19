//! End-to-end lobby test over WebSocket: a real server on an ephemeral port, two
//! WebSocket clients that see each other, invite/accept, exchange opaque payloads
//! through the relay, and see a disconnect notification. No GUI, runs in CI.

use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use netplay_protocol::{
    ClientMsg, Seat, ServerMsg, SharedTokenCredential, DEV_KEY_ID, DEV_TOKEN, PROTOCOL_VERSION,
};
use netplay_server::auth::{DbAuth, SharedTokenAuth};
use netplay_server::store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn dev_credential() -> serde_json::Value {
    SharedTokenCredential {
        key_id: DEV_KEY_ID,
        token: DEV_TOKEN.to_string(),
    }
    .to_value()
}

async fn start_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(netplay_server::serve(
        listener,
        Arc::new(SharedTokenAuth::dev()),
    ));
    addr
}

/// A server backed by a real accounts DB with a seeded admin. Returns the
/// address, the admin's login credential, and the temp dir (kept alive by the
/// caller so the SQLite file outlives the test).
async fn start_server_with_admin() -> (SocketAddr, serde_json::Value, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let pool = store::open(db.to_str().unwrap()).await.unwrap();
    store::upsert_admin(&pool, "root", "s3cret").await.unwrap();
    let auth = Arc::new(DbAuth::new(pool, SharedTokenAuth::dev()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(netplay_server::serve(listener, auth));
    let admin_cred = serde_json::json!({ "name": "root", "password": "s3cret" });
    (addr, admin_cred, dir)
}

async fn connect_ws(addr: SocketAddr, name: &str, credential: serde_json::Value) -> Ws {
    let (mut ws, _) = connect_async(format!("ws://{addr}"))
        .await
        .expect("connect");
    ws.send(Message::binary(netplay_protocol::to_bytes(
        &ClientMsg::Hello {
            name: name.to_string(),
            protocol: PROTOCOL_VERSION,
            credential,
        },
    )))
    .await
    .expect("send hello");
    ws
}

async fn recv(ws: &mut Ws) -> ServerMsg {
    loop {
        match ws.next().await.expect("a message").expect("no ws error") {
            Message::Binary(bytes) => return netplay_protocol::decode(&bytes).expect("decode"),
            Message::Text(text) => {
                return netplay_protocol::decode(text.as_bytes()).expect("decode")
            }
            _ => continue, // ping/pong
        }
    }
}

async fn send(ws: &mut Ws, msg: ClientMsg) {
    ws.send(Message::binary(netplay_protocol::to_bytes(&msg)))
        .await
        .expect("send");
}

async fn expect_matched(ws: &mut Ws) -> Seat {
    loop {
        if let ServerMsg::Matched { seat, .. } = recv(ws).await {
            return seat;
        }
    }
}

#[tokio::test]
async fn invite_accept_relays_and_reports_disconnect() {
    let addr = start_server().await;
    let mut alice = connect_ws(addr, "Alice", dev_credential()).await;
    let mut bob = connect_ws(addr, "Bob", dev_credential()).await;

    // Alice sees Bob in a presence list (skipping any empty one that arrived
    // before Bob connected).
    let bob_id = loop {
        if let ServerMsg::Presence { players } = recv(&mut alice).await {
            if let Some(player) = players.first() {
                break player.id;
            }
        }
    };

    // Alice invites Bob; Bob learns who invited him.
    send(&mut alice, ClientMsg::Invite { to: bob_id }).await;
    let alice_id = loop {
        if let ServerMsg::Invited { from, .. } = recv(&mut bob).await {
            break from;
        }
    };

    // Bob accepts; both are matched with different seats.
    send(&mut bob, ClientMsg::Accept { inviter: alice_id }).await;
    let a_seat = expect_matched(&mut alice).await;
    let b_seat = expect_matched(&mut bob).await;
    assert_ne!(a_seat, b_seat, "players must get different seats");

    // Opaque payloads relay both ways (the server never decodes them).
    send(&mut alice, ClientMsg::Game { payload: vec![19] }).await;
    assert_eq!(recv(&mut bob).await, ServerMsg::Game { payload: vec![19] });
    send(
        &mut bob,
        ClientMsg::Game {
            payload: vec![2, 6],
        },
    )
    .await;
    assert_eq!(
        recv(&mut alice).await,
        ServerMsg::Game {
            payload: vec![2, 6]
        }
    );

    // When Alice drops, Bob is told the opponent left.
    drop(alice);
    assert_eq!(recv(&mut bob).await, ServerMsg::OpponentLeft);
}

#[tokio::test]
async fn rejects_a_bad_credential() {
    let addr = start_server().await;
    let mut ws = connect_ws(addr, "Mallory", serde_json::json!("garbage")).await;
    assert!(matches!(recv(&mut ws).await, ServerMsg::Error { .. }));
}

#[tokio::test]
async fn admin_queries_report_players_matches_and_stats() {
    let (addr, admin_cred, _dir) = start_server_with_admin().await;
    let mut alice = connect_ws(addr, "Alice", dev_credential()).await;
    let mut bob = connect_ws(addr, "Bob", dev_credential()).await;

    // Match Alice (seat 0, the inviter) and Bob (seat 1).
    let bob_id = loop {
        if let ServerMsg::Presence { players } = recv(&mut alice).await {
            if let Some(p) = players.first() {
                break p.id;
            }
        }
    };
    send(&mut alice, ClientMsg::Invite { to: bob_id }).await;
    let alice_id = loop {
        if let ServerMsg::Invited { from, .. } = recv(&mut bob).await {
            break from;
        }
    };
    send(&mut bob, ClientMsg::Accept { inviter: alice_id }).await;
    expect_matched(&mut alice).await;
    expect_matched(&mut bob).await;

    // A third connection queries the relay's admin surface (authorized as admin).
    let mut admin = connect_ws(addr, "admin", admin_cred).await;

    send(&mut admin, ClientMsg::ListPlayers).await;
    let players = loop {
        if let ServerMsg::Players { players } = recv(&mut admin).await {
            break players;
        }
    };
    assert_eq!(players.len(), 3, "alice, bob, admin");

    send(&mut admin, ClientMsg::ListMatches).await;
    let matches = loop {
        if let ServerMsg::Matches { matches } = recv(&mut admin).await {
            break matches;
        }
    };
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].seat0.name, "Alice", "the inviter is seat 0");
    assert_eq!(matches[0].seat1.name, "Bob");

    send(&mut admin, ClientMsg::GetStats).await;
    let stats = loop {
        if let ServerMsg::Stats { stats } = recv(&mut admin).await {
            break stats;
        }
    };
    assert_eq!(stats.players_online, 3);
    assert_eq!(stats.matches_active, 1);
}

#[tokio::test]
async fn non_admin_is_refused_the_admin_surface() {
    let (addr, _admin_cred, _dir) = start_server_with_admin().await;
    // An anonymous player (shared token) — role Player, not admin.
    let mut player = connect_ws(addr, "Randy", dev_credential()).await;
    send(&mut player, ClientMsg::ListPlayers).await;
    // Gets an error, never a Players reply.
    let reply = loop {
        match recv(&mut player).await {
            ServerMsg::Presence { .. } => continue,
            other => break other,
        }
    };
    assert!(matches!(reply, ServerMsg::Error { .. }));
}

#[tokio::test]
async fn admin_subscription_streams_live_events() {
    let (addr, admin_cred, _dir) = start_server_with_admin().await;
    let mut admin = connect_ws(addr, "admin", admin_cred).await;
    send(&mut admin, ClientMsg::SubscribeEvents).await;
    // Round-trip a query so the subscription is registered before anyone joins;
    // the lobby processes this connection's messages in order, so a reply here
    // proves the Subscribe landed (avoids racing the first PlayerJoined).
    send(&mut admin, ClientMsg::ListPlayers).await;
    loop {
        if let ServerMsg::Players { .. } = recv(&mut admin).await {
            break;
        }
    }

    // A join is pushed to the subscriber (with the new player's id).
    let mut alice = connect_ws(addr, "Alice", dev_credential()).await;
    let alice_id = loop {
        if let ServerMsg::PlayerJoined { player } = recv(&mut admin).await {
            if player.name == "Alice" {
                break player.id;
            }
        }
    };
    let mut bob = connect_ws(addr, "Bob", dev_credential()).await;
    let bob_id = loop {
        if let ServerMsg::PlayerJoined { player } = recv(&mut admin).await {
            if player.name == "Bob" {
                break player.id;
            }
        }
    };

    // Pairing them is pushed as MatchStarted (Alice, the inviter, is seat 0).
    send(&mut alice, ClientMsg::Invite { to: bob_id }).await;
    send(&mut bob, ClientMsg::Accept { inviter: alice_id }).await;
    let pairing = loop {
        if let ServerMsg::MatchStarted { pairing } = recv(&mut admin).await {
            break pairing;
        }
    };
    assert_eq!(pairing.seat0.name, "Alice");
    assert_eq!(pairing.seat1.name, "Bob");

    // A disconnect is pushed as PlayerLeft.
    drop(bob);
    let left = loop {
        if let ServerMsg::PlayerLeft { id } = recv(&mut admin).await {
            break id;
        }
    };
    assert_eq!(left, bob_id);
}

#[tokio::test]
async fn schema_endpoint_serves_the_descriptor() {
    let addr = start_server().await;
    // A plain HTTP GET (no WebSocket upgrade) on the same endpoint.
    let response = http_get(addr, "/schema").await;
    assert!(
        response.contains("200 OK"),
        "expected 200, got:\n{response}"
    );
    assert!(response.contains("application/json"));
    // The descriptor carries metadata and the message schemas.
    assert!(response.contains("protocolVersion"));
    assert!(response.contains("ClientMsg"));
    assert!(response.contains("ServerMsg"));
}

#[tokio::test]
async fn asyncapi_endpoint_serves_the_document() {
    let addr = start_server().await;
    let response = http_get(addr, "/asyncapi.json").await;
    assert!(
        response.contains("200 OK"),
        "expected 200, got:\n{response}"
    );
    assert!(response.contains("application/json"));
    assert!(response.contains(r#""asyncapi":"3.0.0""#));
    // One named message per variant (not one anonymous oneOf blob).
    assert!(response.contains("ClientHello"));
    assert!(response.contains("ClientInvite"));
    assert!(response.contains("ServerMatched"));
    assert!(response.contains("ServerPlayerJoined"));
}

/// Issue a plain HTTP/1.1 GET on the relay port and return the full raw response.
async fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    String::from_utf8_lossy(&raw).into_owned()
}
