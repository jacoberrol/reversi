//! End-to-end relay test: a real server on an ephemeral port, two blocking
//! clients that get auto-matched, exchange messages through the relay, and see
//! a disconnect notification. No GUI, so it runs in CI.

use std::net::{SocketAddr, TcpStream};

use protocol::{ClientMsg, Color, GameMsg, ServerMsg, PROTOCOL_VERSION};
use tokio::net::TcpListener;

fn connect(addr: SocketAddr, name: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).expect("connect");
    protocol::write_msg(
        &mut stream,
        &ClientMsg::Hello {
            name: name.to_string(),
            protocol: PROTOCOL_VERSION,
        },
    )
    .expect("send hello");
    stream
}

fn recv(stream: &mut TcpStream) -> ServerMsg {
    let body = protocol::read_frame(stream)
        .expect("read frame")
        .expect("a frame (not EOF)");
    protocol::decode(&body).expect("decode server msg")
}

fn send(stream: &mut TcpStream, msg: GameMsg) {
    protocol::write_msg(stream, &ClientMsg::Game(msg)).expect("send game msg");
}

#[tokio::test]
async fn auto_match_relays_and_reports_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(server::serve(listener));

    // Run the blocking clients off the async runtime.
    tokio::task::spawn_blocking(move || scenario(addr))
        .await
        .expect("scenario task");
}

fn scenario(addr: SocketAddr) {
    let mut alice = connect(addr, "Alice");
    let mut bob = connect(addr, "Bob");

    // Both get matched with opposite colors and each other's names.
    let (a_color, a_opp) = match recv(&mut alice) {
        ServerMsg::Matched {
            your_color,
            opponent,
        } => (your_color, opponent),
        other => panic!("expected Matched for Alice, got {other:?}"),
    };
    let (b_color, b_opp) = match recv(&mut bob) {
        ServerMsg::Matched {
            your_color,
            opponent,
        } => (your_color, opponent),
        other => panic!("expected Matched for Bob, got {other:?}"),
    };
    assert_eq!(a_opp, "Bob");
    assert_eq!(b_opp, "Alice");
    assert_ne!(a_color, b_color, "players must get opposite colors");
    assert!(matches!(a_color, Color::Black | Color::White));

    // A move from Alice is relayed to Bob, and vice versa (the relay forwards
    // game messages regardless of turn).
    send(&mut alice, GameMsg::Move { square: 19 });
    assert_eq!(
        recv(&mut bob),
        ServerMsg::Game(GameMsg::Move { square: 19 })
    );

    send(&mut bob, GameMsg::Move { square: 26 });
    assert_eq!(
        recv(&mut alice),
        ServerMsg::Game(GameMsg::Move { square: 26 })
    );

    // When Alice drops, Bob is told the opponent left.
    drop(alice);
    assert_eq!(recv(&mut bob), ServerMsg::OpponentLeft);
}
