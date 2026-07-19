//! End-to-end tests: a real server on an ephemeral port. The **game** rides the
//! WebSocket (register/login → lobby → invite/accept → relay). The **admin** REST
//! API rides plain HTTP on the admin host (login → bearer → read endpoints). No
//! GUI; runs in CI.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use netplay_protocol::{ClientMsg, PlayerInfo, Seat, ServerMsg, ServerStats, PROTOCOL_VERSION};
use netplay_server::store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The admin host the test server routes REST traffic for.
const ADMIN_HOST: &str = "admin.test";

/// Credential that registers a new account over the game WebSocket.
fn register(name: &str, password: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "password": password, "register": true })
}

/// An accounts-only server on an ephemeral port. Returns the address and the
/// temp dir (kept alive by the caller so the SQLite file outlives the test).
async fn start_server() -> (SocketAddr, tempfile::TempDir) {
    let (addr, _pool, dir) = spawn(false).await;
    (addr, dir)
}

/// Like [`start_server`], plus a seeded admin (`root` / `s3cret`).
async fn start_server_with_admin() -> (SocketAddr, tempfile::TempDir) {
    let (addr, _pool, dir) = spawn(true).await;
    (addr, dir)
}

async fn spawn(with_admin: bool) -> (SocketAddr, sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let pool = store::open(db.to_str().unwrap()).await.unwrap();
    if with_admin {
        store::upsert_admin(&pool, "root", "s3cret").await.unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(netplay_server::serve(
        listener,
        pool.clone(),
        ADMIN_HOST.to_string(),
    ));
    (addr, pool, dir)
}

// --- game (WebSocket) helpers ------------------------------------------------

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

// --- admin (HTTP) helpers ----------------------------------------------------

/// Send a raw HTTP/1.1 request (Connection: close) and return the full response.
async fn http(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    String::from_utf8_lossy(&raw).into_owned()
}

fn get(path: &str, bearer: Option<&str>) -> String {
    let auth = bearer
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    format!("GET {path} HTTP/1.1\r\nHost: {ADMIN_HOST}\r\n{auth}Connection: close\r\n\r\n")
}

fn post(path: &str, body: &str) -> String {
    post_auth(path, body, None)
}

fn post_auth(path: &str, body: &str, bearer: Option<&str>) -> String {
    let auth = bearer
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    format!(
        "POST {path} HTTP/1.1\r\nHost: {ADMIN_HOST}\r\nContent-Type: application/json\r\n\
         {auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn body_of(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or("")
}

#[derive(serde::Deserialize)]
struct TokenResp {
    token: String,
    expires_in_hours: i64,
}

/// Log in as the seeded admin (`root`/`s3cret`) and return the bearer token.
async fn admin_login(addr: SocketAddr) -> String {
    let r = http(
        addr,
        &post("/admin/login", r#"{"name":"root","password":"s3cret"}"#),
    )
    .await;
    assert!(r.starts_with("HTTP/1.1 200"), "{}", first_line(&r));
    serde_json::from_str::<TokenResp>(body_of(&r))
        .unwrap()
        .token
}

// --- tests -------------------------------------------------------------------

#[tokio::test]
async fn invite_accept_relays_and_reports_disconnect() {
    let (addr, _dir) = start_server().await;
    let mut alice = connect_ws(addr, "Alice", register("Alice", "password")).await;
    let mut bob = connect_ws(addr, "Bob", register("Bob", "password")).await;

    let bob_id = loop {
        if let ServerMsg::Presence { players } = recv(&mut alice).await {
            if let Some(player) = players.first() {
                break player.id;
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
    let (addr, _dir) = start_server().await;
    let mut ws = connect_ws(addr, "Mallory", serde_json::json!("garbage")).await;
    assert!(matches!(recv(&mut ws).await, ServerMsg::Error { .. }));
}

#[tokio::test]
async fn registration_login_and_their_failures() {
    let (addr, _dir) = start_server().await;

    let mut dave = connect_ws(addr, "Dave", register("Dave", "password")).await;
    assert!(matches!(recv(&mut dave).await, ServerMsg::Presence { .. }));

    let mut dupe = connect_ws(addr, "Dave", register("Dave", "different")).await;
    assert!(matches!(recv(&mut dupe).await, ServerMsg::Error { .. }));

    let mut weak = connect_ws(addr, "Eve", register("Eve", "short")).await;
    assert!(matches!(recv(&mut weak).await, ServerMsg::Error { .. }));

    // Log in with the registered password (register omitted).
    let login = serde_json::json!({ "name": "Dave", "password": "password" });
    let mut relog = connect_ws(addr, "Dave", login).await;
    assert!(matches!(recv(&mut relog).await, ServerMsg::Presence { .. }));
}

#[tokio::test]
async fn admin_rest_login_then_reads_lobby_state() {
    let (addr, _dir) = start_server_with_admin().await;

    // Two players join over the game WebSocket.
    let mut alice = connect_ws(addr, "Alice", register("Alice", "password")).await;
    let _bob = connect_ws(addr, "Bob", register("Bob", "password")).await;
    loop {
        if let ServerMsg::Presence { players } = recv(&mut alice).await {
            if players.iter().any(|p| p.name == "Bob") {
                break;
            }
        }
    }

    // Without a bearer token the read endpoints are refused.
    let r = http(addr, &get("/admin/players", None)).await;
    assert!(r.starts_with("HTTP/1.1 401"), "{}", first_line(&r));

    // Log in for a bearer token.
    let r = http(
        addr,
        &post("/admin/login", r#"{"name":"root","password":"s3cret"}"#),
    )
    .await;
    assert!(r.starts_with("HTTP/1.1 200"), "{}", first_line(&r));
    let token = serde_json::from_str::<TokenResp>(body_of(&r))
        .unwrap()
        .token;

    // Players and stats reflect the two connected players.
    let r = http(addr, &get("/admin/players", Some(&token))).await;
    assert!(r.starts_with("HTTP/1.1 200"), "{}", first_line(&r));
    let players: Vec<PlayerInfo> = serde_json::from_str(body_of(&r)).unwrap();
    assert_eq!(players.len(), 2);

    let r = http(addr, &get("/admin/stats", Some(&token))).await;
    let stats: ServerStats = serde_json::from_str(body_of(&r)).unwrap();
    assert_eq!(stats.players_online, 2);
}

#[tokio::test]
async fn admin_login_rejects_bad_password_and_non_admins() {
    let (addr, _dir) = start_server_with_admin().await;

    let r = http(
        addr,
        &post("/admin/login", r#"{"name":"root","password":"nope"}"#),
    )
    .await;
    assert!(r.starts_with("HTTP/1.1 401"), "{}", first_line(&r));

    // A registered player is not an admin → 403.
    let mut randy = connect_ws(addr, "Randy", register("Randy", "password")).await;
    let _ = recv(&mut randy).await; // presence
    let r = http(
        addr,
        &post("/admin/login", r#"{"name":"Randy","password":"password"}"#),
    )
    .await;
    assert!(r.starts_with("HTTP/1.1 403"), "{}", first_line(&r));
}

#[tokio::test]
async fn admin_durable_token_is_minted_from_a_bearer_and_authorizes() {
    let (addr, _dir) = start_server_with_admin().await;
    let login = admin_login(addr).await;

    // Trade the login session for a durable token of a chosen lifetime.
    let r = http(
        addr,
        &post_auth("/admin/tokens", r#"{"days":14}"#, Some(&login)),
    )
    .await;
    assert!(r.starts_with("HTTP/1.1 200"), "{}", first_line(&r));
    let durable: TokenResp = serde_json::from_str(body_of(&r)).unwrap();
    assert_eq!(durable.expires_in_hours, 14 * 24);
    assert_ne!(durable.token, login, "a distinct token is issued");

    // The durable token authorizes the read endpoints on its own.
    let r = http(addr, &get("/admin/stats", Some(&durable.token))).await;
    assert!(r.starts_with("HTTP/1.1 200"), "{}", first_line(&r));

    // An empty body takes the default lifetime; no bearer is refused.
    let r = http(addr, &post_auth("/admin/tokens", "", Some(&login))).await;
    let defaulted: TokenResp = serde_json::from_str(body_of(&r)).unwrap();
    assert_eq!(defaulted.expires_in_hours, 30 * 24);

    let r = http(addr, &post_auth("/admin/tokens", r#"{"days":7}"#, None)).await;
    assert!(r.starts_with("HTTP/1.1 401"), "{}", first_line(&r));
}

#[tokio::test]
async fn openapi_doc_is_served_without_auth() {
    let (addr, _dir) = start_server_with_admin().await;

    // No bearer token required — it's how a client discovers the API.
    let r = http(addr, &get("/admin/openapi.json", None)).await;
    assert!(r.starts_with("HTTP/1.1 200"), "{}", first_line(&r));
    let doc: serde_json::Value = serde_json::from_str(body_of(&r)).unwrap();
    assert_eq!(doc["openapi"], "3.0.3");
    assert!(doc["paths"]["/admin/login"]["post"].is_object());
    assert!(doc["paths"]["/admin/stats"]["get"].is_object());
}

fn first_line(response: &str) -> &str {
    response.lines().next().unwrap_or("")
}
