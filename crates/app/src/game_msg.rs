//! The Reversi-specific in-game message and its mapping to the netplay layer.
//!
//! The netplay protocol carries an opaque `Vec<u8>` payload; Reversi defines its
//! own [`GameMsg`] and (de)serializes it into that payload. It also maps the
//! abstract [`Seat`] to a `Player` (seat 0 = Black, who moves first).

use game_core::Player;
use netplay_protocol::Seat;
use serde::{Deserialize, Serialize};

/// A Reversi in-game action. Rides inside the netplay `Game` payload.
///
/// Passes are never sent: both clients derive forced passes locally. Leaving a
/// game isn't a message either — the server reports a departed opponent as
/// `OpponentLeft`. Internally tagged (`{"type":"Move","square":19}`) to match
/// the netplay envelope's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GameMsg {
    /// Place a disc on the square with this flat index (`0..64`).
    Move { square: u8 },
    /// Start a new game (both sides reset).
    Restart,
}

/// Serialize a message into an opaque netplay payload.
pub fn encode(msg: &GameMsg) -> Vec<u8> {
    serde_json::to_vec(msg).expect("GameMsg always serializes")
}

/// Deserialize an opaque netplay payload, or `None` if it isn't a valid message.
pub fn decode(payload: &[u8]) -> Option<GameMsg> {
    serde_json::from_slice(payload).ok()
}

/// Map a netplay seat to a Reversi player (seat 0 = Black).
pub fn player_of(seat: Seat) -> Player {
    if seat.0 == 0 {
        Player::Black
    } else {
        Player::White
    }
}
