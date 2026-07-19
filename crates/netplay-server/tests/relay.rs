//! End-to-end lobby test over WebSocket: a real server on an ephemeral port, two
//! WebSocket clients that see each other, invite/accept, exchange opaque payloads
//! through the relay, and see a disconnect notification. No GUI, runs in CI.

use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use netplay_protocol::{
    ClientMsg, Seat, ServerMsg, SharedTokenCredential, DEV_KEY_ID, DEV_TOKEN, PROTOCOL_VERSION,
};
use netplay_server::auth::SharedTokenAuth;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn dev_credential() -> Vec<u8> {
    SharedTokenCredential {
        key_id: DEV_KEY_ID,
        token: DEV_TOKEN.to_string(),
    }
    .to_bytes()
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

async fn connect_ws(addr: SocketAddr, name: &str, credential: Vec<u8>) -> Ws {
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
    let mut ws = connect_ws(addr, "Mallory", b"garbage".to_vec()).await;
    assert!(matches!(recv(&mut ws).await, ServerMsg::Error { .. }));
}

#[tokio::test]
async fn schema_endpoint_serves_the_descriptor() {
    let addr = start_server().await;
    // A plain HTTP GET (no WebSocket upgrade) on the same endpoint.
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /schema HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let response = String::from_utf8_lossy(&raw);

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
