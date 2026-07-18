//! The wire protocol shared by the client (`app`) and the relay `server`.
//!
//! Messages use only primitive types (a move is a `u8` square index), so this
//! crate depends on nothing but `serde` — the server never pulls in game logic,
//! and `game-core` never gains a serialization dependency. `app` maps between
//! [`Color`] and `game_core::Player`, and between `square: u8` and
//! `game_core::Square`, at its boundary.
//!
//! Framing is length-delimited: a big-endian `u32` byte count followed by that
//! many bytes of JSON. JSON keeps the protocol easy to eyeball while debugging;
//! the messages are tiny, so it's cheap. A binary codec can replace it later
//! without touching call sites.

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change to the message types. The server rejects a
/// [`ClientMsg::Hello`] whose `protocol` doesn't match.
pub const PROTOCOL_VERSION: u16 = 1;

/// Reject frames larger than this (messages are tiny; this guards against a bad
/// or hostile length prefix causing a huge allocation).
const MAX_FRAME: usize = 1 << 16;

/// A connected player, for the lifetime of its connection.
pub type PlayerId = u64;

/// A player as advertised in the lobby presence list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
}

/// Which side a player controls. A primitive mirror of `game_core::Player`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Black,
    White,
}

/// An in-game action. Relayed opaquely by the server between the two players.
///
/// Passes are never sent: both clients derive forced passes locally from the
/// board after each move, so only real placements cross the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameMsg {
    /// Place a disc on the square with this flat index (`0..64`).
    Move { square: u8 },
    /// Start a new game (both sides reset).
    Restart,
    /// Concede the game.
    Resign,
}

/// A message from a client to the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// First message on connect: the player's display name and protocol version.
    Hello { name: String, protocol: u16 },
    /// Invite another player (by id) to a game.
    Invite { to: PlayerId },
    /// Accept an invite from `inviter`.
    Accept { inviter: PlayerId },
    /// Decline an invite from `inviter`.
    Decline { inviter: PlayerId },
    /// An in-game action, to be relayed to the opponent.
    Game(GameMsg),
}

/// A message from the server to a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    /// The other players currently available in the lobby.
    Presence { players: Vec<PlayerInfo> },
    /// You received an invite from player `from` (named `name`).
    Invited { from: PlayerId, name: String },
    /// An invite you sent was declined by player `by`.
    InviteDeclined { by: PlayerId },
    /// Paired with an opponent; you play `your_color`.
    Matched { your_color: Color, opponent: String },
    /// An in-game action from the opponent.
    Game(GameMsg),
    /// The opponent disconnected or resigned.
    OpponentLeft,
    /// A protocol-level error (e.g. version mismatch); the connection closes.
    Error(String),
}

/// Encode a message as a length-delimited frame.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let body = serde_json::to_vec(msg).expect("protocol messages always serialize");
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    frame
}

/// Decode a message from a frame body.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> serde_json::Result<T> {
    serde_json::from_slice(body)
}

/// Read one length-delimited frame body from a blocking reader. Returns
/// `Ok(None)` on a clean EOF at a frame boundary (peer closed the connection).
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Write a message as a length-delimited frame to a blocking writer.
pub fn write_msg<T: Serialize>(writer: &mut impl Write, msg: &T) -> io::Result<()> {
    writer.write_all(&encode(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn round_trip_client(msg: ClientMsg) {
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = Cursor::new(buf);
        let body = read_frame(&mut cursor).unwrap().expect("a frame");
        let decoded: ClientMsg = decode(&body).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn client_messages_round_trip() {
        round_trip_client(ClientMsg::Hello {
            name: "Jake".into(),
            protocol: PROTOCOL_VERSION,
        });
        round_trip_client(ClientMsg::Invite { to: 7 });
        round_trip_client(ClientMsg::Accept { inviter: 7 });
        round_trip_client(ClientMsg::Decline { inviter: 7 });
        round_trip_client(ClientMsg::Game(GameMsg::Move { square: 19 }));
        round_trip_client(ClientMsg::Game(GameMsg::Restart));
        round_trip_client(ClientMsg::Game(GameMsg::Resign));
    }

    #[test]
    fn server_messages_round_trip() {
        for msg in [
            ServerMsg::Presence {
                players: vec![
                    PlayerInfo {
                        id: 1,
                        name: "Bob".into(),
                    },
                    PlayerInfo {
                        id: 2,
                        name: "Carol".into(),
                    },
                ],
            },
            ServerMsg::Invited {
                from: 1,
                name: "Bob".into(),
            },
            ServerMsg::InviteDeclined { by: 1 },
            ServerMsg::Matched {
                your_color: Color::Black,
                opponent: "Bob".into(),
            },
            ServerMsg::Game(GameMsg::Move { square: 42 }),
            ServerMsg::OpponentLeft,
            ServerMsg::Error("bad version".into()),
        ] {
            let mut buf = Vec::new();
            write_msg(&mut buf, &msg).unwrap();
            let mut cursor = Cursor::new(buf);
            let body = read_frame(&mut cursor).unwrap().expect("a frame");
            let decoded: ServerMsg = decode(&body).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn clean_eof_returns_none() {
        let mut empty = Cursor::new(Vec::new());
        assert!(read_frame(&mut empty).unwrap().is_none());
    }

    #[test]
    fn two_frames_read_in_order() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &ClientMsg::Game(GameMsg::Move { square: 1 })).unwrap();
        write_msg(&mut buf, &ClientMsg::Game(GameMsg::Move { square: 2 })).unwrap();
        let mut cursor = Cursor::new(buf);
        let first: ClientMsg = decode(&read_frame(&mut cursor).unwrap().unwrap()).unwrap();
        let second: ClientMsg = decode(&read_frame(&mut cursor).unwrap().unwrap()).unwrap();
        assert_eq!(first, ClientMsg::Game(GameMsg::Move { square: 1 }));
        assert_eq!(second, ClientMsg::Game(GameMsg::Move { square: 2 }));
    }
}
