//! End-to-end lobby test: a real server on an ephemeral port, two blocking
//! clients that see each other, invite/accept, exchange messages through the
//! relay, and see a disconnect notification. No GUI, so it runs in CI.

use std::net::{SocketAddr, TcpStream};

use protocol::{ClientMsg, Color, GameMsg, PlayerId, ServerMsg, PROTOCOL_VERSION};
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

fn send(stream: &mut TcpStream, msg: ClientMsg) {
    protocol::write_msg(stream, &msg).expect("send msg");
}

/// Read frames until a `Presence` list arrives; return the ids in it.
fn wait_for_presence(stream: &mut TcpStream) -> Vec<PlayerId> {
    loop {
        if let ServerMsg::Presence { players } = recv(stream) {
            return players.into_iter().map(|p| p.id).collect();
        }
    }
}

#[tokio::test]
async fn invite_accept_relays_and_reports_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(server::serve(listener));

    tokio::task::spawn_blocking(move || scenario(addr))
        .await
        .expect("scenario task");
}

fn scenario(addr: SocketAddr) {
    let mut alice = connect(addr, "Alice");
    let mut bob = connect(addr, "Bob");

    // Once both are connected, each sees the other in a presence list. Alice
    // may first get an empty presence (before Bob joined), so wait for Bob.
    let bob_id = loop {
        let ids = wait_for_presence(&mut alice);
        if let Some(id) = ids.first() {
            break *id;
        }
    };

    // Alice invites Bob; Bob is told who invited him.
    send(&mut alice, ClientMsg::Invite { to: bob_id });
    let alice_id = loop {
        match recv(&mut bob) {
            ServerMsg::Invited { from, .. } => break from,
            _ => continue,
        }
    };

    // Bob accepts; both are matched with opposite colors.
    send(&mut bob, ClientMsg::Accept { inviter: alice_id });
    let a_color = expect_matched(&mut alice);
    let b_color = expect_matched(&mut bob);
    assert_ne!(a_color, b_color, "players must get opposite colors");
    assert!(matches!(a_color, Color::Black | Color::White));

    // Moves relay both ways.
    send(&mut alice, ClientMsg::Game(GameMsg::Move { square: 19 }));
    assert_eq!(
        recv(&mut bob),
        ServerMsg::Game(GameMsg::Move { square: 19 })
    );
    send(&mut bob, ClientMsg::Game(GameMsg::Move { square: 26 }));
    assert_eq!(
        recv(&mut alice),
        ServerMsg::Game(GameMsg::Move { square: 26 })
    );

    // When Alice drops, Bob is told the opponent left.
    drop(alice);
    assert_eq!(recv(&mut bob), ServerMsg::OpponentLeft);
}

/// Read frames until a `Matched` arrives and return the assigned color.
fn expect_matched(stream: &mut TcpStream) -> Color {
    loop {
        if let ServerMsg::Matched { your_color, .. } = recv(stream) {
            return your_color;
        }
    }
}
