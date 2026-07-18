//! Client networking: a blocking TCP connection to the relay server.
//!
//! The client stays async-free. `TcpStream::try_clone` splits the socket into an
//! independent read half and write half: a background thread blocks on reads and
//! forwards decoded messages to the winit event loop via an [`EventLoopProxy`]
//! (delivered as [`NetEvent`]s), while the main thread writes outgoing moves
//! through [`NetHandle`]. No locks, no runtime.

use std::io::{self, Write};
use std::net::TcpStream;
use std::thread;

use protocol::{ClientMsg, Color, GameMsg, PlayerId, PlayerInfo, ServerMsg, PROTOCOL_VERSION};
use winit::event_loop::EventLoopProxy;

/// A message from the network, injected into the winit event loop as a user
/// event.
#[derive(Debug)]
pub enum NetEvent {
    /// The other players currently available in the lobby.
    Presence(Vec<PlayerInfo>),
    /// You received an invite.
    Invited { from: PlayerId, name: String },
    /// An invite you sent was declined.
    InviteDeclined { by: PlayerId },
    /// Paired with an opponent; the local player controls `color`.
    Matched { color: Color, opponent: String },
    /// An in-game message from the opponent.
    Remote(GameMsg),
    /// The opponent disconnected or resigned (server-side).
    OpponentLeft,
    /// The connection closed.
    Disconnected,
    /// A protocol/connection error.
    Error(String),
}

/// The write side of the connection. Held by the main thread to send moves.
pub struct NetHandle {
    writer: TcpStream,
}

impl NetHandle {
    /// Send an in-game message to the opponent (best effort; a broken connection
    /// surfaces as [`NetEvent::Disconnected`] on the read thread).
    pub fn send(&mut self, msg: GameMsg) {
        self.send_client(ClientMsg::Game(msg));
    }

    /// Invite another player (by id) to a game.
    pub fn invite(&mut self, to: PlayerId) {
        self.send_client(ClientMsg::Invite { to });
    }

    /// Accept an invite from `inviter`.
    pub fn accept(&mut self, inviter: PlayerId) {
        self.send_client(ClientMsg::Accept { inviter });
    }

    /// Decline an invite from `inviter`.
    pub fn decline(&mut self, inviter: PlayerId) {
        self.send_client(ClientMsg::Decline { inviter });
    }

    fn send_client(&mut self, msg: ClientMsg) {
        let _ = protocol::write_msg(&mut self.writer, &msg);
        let _ = self.writer.flush();
    }
}

/// Connect to `addr`, send the handshake, and spawn the read thread. Returns the
/// write handle; incoming messages arrive as [`NetEvent`]s on `proxy`.
pub fn connect(addr: &str, name: &str, proxy: EventLoopProxy<NetEvent>) -> io::Result<NetHandle> {
    let read_half = TcpStream::connect(addr)?;
    let mut writer = read_half.try_clone()?;

    protocol::write_msg(
        &mut writer,
        &ClientMsg::Hello {
            name: name.to_string(),
            protocol: PROTOCOL_VERSION,
        },
    )?;
    writer.flush()?;

    thread::spawn(move || read_loop(read_half, proxy));
    Ok(NetHandle { writer })
}

fn read_loop(mut reader: TcpStream, proxy: EventLoopProxy<NetEvent>) {
    loop {
        let event = match protocol::read_frame(&mut reader) {
            Ok(Some(body)) => match protocol::decode::<ServerMsg>(&body) {
                Ok(msg) => server_msg_to_event(msg),
                Err(_) => NetEvent::Error("malformed message from server".to_string()),
            },
            // Clean EOF or read error: the connection is gone.
            Ok(None) | Err(_) => NetEvent::Disconnected,
        };
        let terminal = matches!(
            event,
            NetEvent::Disconnected | NetEvent::OpponentLeft | NetEvent::Error(_)
        );
        // If the event loop has exited, stop.
        if proxy.send_event(event).is_err() {
            break;
        }
        if terminal {
            break;
        }
    }
}

fn server_msg_to_event(msg: ServerMsg) -> NetEvent {
    match msg {
        ServerMsg::Presence { players } => NetEvent::Presence(players),
        ServerMsg::Invited { from, name } => NetEvent::Invited { from, name },
        ServerMsg::InviteDeclined { by } => NetEvent::InviteDeclined { by },
        ServerMsg::Matched {
            your_color,
            opponent,
        } => NetEvent::Matched {
            color: your_color,
            opponent,
        },
        ServerMsg::Game(game) => NetEvent::Remote(game),
        ServerMsg::OpponentLeft => NetEvent::OpponentLeft,
        ServerMsg::Error(message) => NetEvent::Error(message),
    }
}

/// Map a protocol color to a game-core player.
pub fn player_of(color: Color) -> game_core::Player {
    match color {
        Color::Black => game_core::Player::Black,
        Color::White => game_core::Player::White,
    }
}
