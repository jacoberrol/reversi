//! Reusable client transport for the netplay layer.
//!
//! Async-free: `TcpStream::try_clone` splits the socket into an independent read
//! half and write half. A background thread blocks on reads and forwards decoded
//! [`NetEvent`]s to the winit event loop via an [`EventLoopProxy`]; the main
//! thread writes outgoing messages through [`NetHandle`]. No locks, no runtime.
//!
//! The in-game action is opaque here: [`NetHandle::game`] sends a `Vec<u8>` and
//! [`NetEvent::Game`] delivers one. The game defines and codes its own payload.

use std::io::{self, Write};
use std::net::TcpStream;
use std::thread;

use netplay_protocol::{ClientMsg, PlayerId, PlayerInfo, Seat, ServerMsg, PROTOCOL_VERSION};
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
    /// Paired with an opponent; you take `seat` (seat 0 moves first).
    Matched { seat: Seat, opponent: String },
    /// An opaque in-game payload from the opponent.
    Game(Vec<u8>),
    /// The opponent disconnected or resigned (server-side).
    OpponentLeft,
    /// The connection closed.
    Disconnected,
    /// A protocol/connection error.
    Error(String),
}

/// The write side of the connection. Held by the main thread to send messages.
pub struct NetHandle {
    writer: TcpStream,
}

impl NetHandle {
    /// Send an opaque in-game payload to the opponent (best effort; a broken
    /// connection surfaces as [`NetEvent::Disconnected`] on the read thread).
    pub fn game(&mut self, payload: Vec<u8>) {
        self.send_client(ClientMsg::Game(payload));
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
        let _ = netplay_protocol::write_msg(&mut self.writer, &msg);
        let _ = self.writer.flush();
    }
}

/// Connect to `addr`, send the handshake, and spawn the read thread. Returns the
/// write handle; incoming messages arrive as [`NetEvent`]s on `proxy`.
pub fn connect(addr: &str, name: &str, proxy: EventLoopProxy<NetEvent>) -> io::Result<NetHandle> {
    let read_half = TcpStream::connect(addr)?;
    let mut writer = read_half.try_clone()?;

    netplay_protocol::write_msg(
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
        let event = match netplay_protocol::read_frame(&mut reader) {
            Ok(Some(body)) => match netplay_protocol::decode::<ServerMsg>(&body) {
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
        ServerMsg::Matched { seat, opponent } => NetEvent::Matched { seat, opponent },
        ServerMsg::Game(payload) => NetEvent::Game(payload),
        ServerMsg::OpponentLeft => NetEvent::OpponentLeft,
        ServerMsg::Error(message) => NetEvent::Error(message),
    }
}
