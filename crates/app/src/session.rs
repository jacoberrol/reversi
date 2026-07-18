//! Game session state and logic, independent of wgpu/windowing.
//!
//! Owns the [`Game`], the [`Animator`], and (in network mode) the connection.
//! `gpu::WindowState` delegates input, network events, and per-frame render
//! inputs here, so all the mode-aware rules live in one place.

use std::time::Instant;

use game_core::{Board, Outcome, Square};
use protocol::GameMsg;
use render::board_view::{PieceAnim, View};

use crate::anim::Animator;
use crate::game::Game;
use crate::net::{self, NetEvent, NetHandle};

/// Network connection status, shown in the title bar.
enum NetStatus {
    Waiting,
    Playing,
    OpponentLeft,
    Disconnected,
    Error(String),
}

/// Present only in network mode.
struct NetState {
    /// `None` if the connection failed at startup.
    handle: Option<NetHandle>,
    status: NetStatus,
    opponent: String,
}

/// The playable game plus its animator and optional network connection.
pub struct Session {
    game: Game,
    animator: Animator,
    net: Option<NetState>,
}

impl Session {
    /// A local single-player session (vs the AI).
    pub fn new() -> Self {
        Self {
            game: Game::new(),
            animator: Animator::default(),
            net: None,
        }
    }

    pub fn is_network(&self) -> bool {
        self.net.is_some()
    }

    pub fn is_animating(&self) -> bool {
        self.animator.is_active()
    }

    /// Enter network mode with a live connection (waiting to be matched).
    pub fn enter_network(&mut self, handle: NetHandle) {
        self.net = Some(NetState {
            handle: Some(handle),
            status: NetStatus::Waiting,
            opponent: String::new(),
        });
    }

    /// Enter network mode already failed (couldn't connect).
    pub fn set_net_error(&mut self, message: String) {
        self.net = Some(NetState {
            handle: None,
            status: NetStatus::Error(message),
            opponent: String::new(),
        });
    }

    pub fn set_difficulty_index(&mut self, index: usize) -> bool {
        // Difficulty is meaningless in network mode (no AI).
        if self.net.is_some() {
            return false;
        }
        match crate::game::Difficulty::from_index(index) {
            Some(difficulty) => {
                self.game.set_difficulty(difficulty);
                true
            }
            None => false,
        }
    }

    /// Start a new game. In network mode, also tell the opponent.
    pub fn restart(&mut self) {
        self.animator.clear();
        self.game.restart();
        if let Some(net) = &mut self.net {
            if let Some(handle) = &mut net.handle {
                handle.send(GameMsg::Restart);
            }
            net.status = NetStatus::Playing;
        }
    }

    /// Handle a click on board square `sq`. Returns whether something changed.
    pub fn click_square(&mut self, sq: Square) -> bool {
        if self.animator.is_active() {
            return false;
        }
        // In network mode, only interact once matched.
        if let Some(net) = &self.net {
            if !matches!(net.status, NetStatus::Playing) {
                return false;
            }
        }
        if self.game.is_over() {
            self.restart();
            return true;
        }
        if !self.game.awaiting_local() {
            return false;
        }

        let transitions = if self.net.is_some() {
            self.game.play_local(sq)
        } else {
            self.game.play_human(sq)
        };
        if transitions.is_empty() {
            return false;
        }

        if self.net.is_some() {
            let square = sq.index() as u8;
            if let Some(net) = &mut self.net {
                if let Some(handle) = &mut net.handle {
                    handle.send(GameMsg::Move { square });
                }
            }
        }
        self.animator.push(transitions);
        true
    }

    /// Apply a message from the network. Returns whether a redraw is needed.
    pub fn on_net_event(&mut self, event: NetEvent) -> bool {
        if self.net.is_none() {
            return false;
        }
        match event {
            NetEvent::Matched { color, opponent } => {
                self.game.set_local(net::player_of(color));
                if let Some(net) = &mut self.net {
                    net.status = NetStatus::Playing;
                    net.opponent = opponent;
                }
            }
            NetEvent::Remote(GameMsg::Move { square }) => {
                if let Some(sq) = Square::from_index(square as usize) {
                    if let Some(transitions) = self.game.apply_remote_move(sq) {
                        self.animator.push(transitions);
                    }
                }
            }
            NetEvent::Remote(GameMsg::Restart) => {
                self.animator.clear();
                self.game.restart();
            }
            NetEvent::Remote(GameMsg::Resign) | NetEvent::OpponentLeft => {
                if let Some(net) = &mut self.net {
                    net.status = NetStatus::OpponentLeft;
                }
            }
            NetEvent::Disconnected => {
                if let Some(net) = &mut self.net {
                    net.status = NetStatus::Disconnected;
                }
            }
            NetEvent::Error(message) => {
                if let Some(net) = &mut self.net {
                    net.status = NetStatus::Error(message);
                }
            }
        }
        true
    }

    /// The board and disc animations to draw this frame.
    pub fn frame(&mut self, now: Instant) -> (Board, Vec<PieceAnim>) {
        match self.animator.frame(now) {
            Some((board, anims)) => (board, anims),
            None => (self.game.board().clone(), Vec::new()),
        }
    }

    /// The non-board draw settings for this frame.
    pub fn view(&self, animating: bool) -> View {
        let can_play = self
            .net
            .as_ref()
            .is_none_or(|net| matches!(net.status, NetStatus::Playing));
        View {
            show_hints: !animating && can_play && self.game.awaiting_local(),
            show_controls: self.net.is_none(),
            selected_difficulty: self.game.difficulty().index(),
            outcome: if animating { None } else { self.game.outcome() },
        }
    }

    /// The window title reflecting the current state (our stand-in for on-screen
    /// text until a glyph renderer exists).
    pub fn title(&self) -> String {
        match &self.net {
            None => self.single_player_title(),
            Some(net) => self.network_title(net),
        }
    }

    fn single_player_title(&self) -> String {
        let (black, white) = self.game.score();
        let status = match self.game.outcome() {
            Some(Outcome::Win(p)) if p == self.game.local() => {
                format!("You win {black}\u{2013}{white} \u{00b7} click board for a new game")
            }
            Some(Outcome::Win(_)) => {
                format!("AI wins {white}\u{2013}{black} \u{00b7} click board for a new game")
            }
            Some(Outcome::Draw) => {
                format!("Draw {black}\u{2013}{white} \u{00b7} click board for a new game")
            }
            None => "Your move".to_string(),
        };
        format!(
            "Reversi \u{2014} {status} \u{00b7} {}",
            self.game.difficulty().name()
        )
    }

    fn network_title(&self, net: &NetState) -> String {
        let (me, opp) = self.game.score();
        let status = match &net.status {
            NetStatus::Waiting => "Waiting for opponent\u{2026}".to_string(),
            NetStatus::Playing if self.game.is_over() => match self.game.outcome() {
                Some(Outcome::Win(p)) if p == self.game.local() => {
                    format!("You win {me}\u{2013}{opp} \u{00b7} click board for a new game")
                }
                Some(Outcome::Win(_)) => {
                    format!("You lose {opp}\u{2013}{me} \u{00b7} click board for a new game")
                }
                _ => format!("Draw {me}\u{2013}{opp} \u{00b7} click board for a new game"),
            },
            NetStatus::Playing if self.game.awaiting_local() => "Your move".to_string(),
            NetStatus::Playing => "Opponent's move".to_string(),
            NetStatus::OpponentLeft => "Opponent left".to_string(),
            NetStatus::Disconnected => "Disconnected".to_string(),
            NetStatus::Error(message) => format!("Error: {message}"),
        };
        let side = match self.game.local() {
            game_core::Player::Black => "Black",
            game_core::Player::White => "White",
        };
        if matches!(net.status, NetStatus::Playing) {
            format!(
                "Reversi \u{2014} {status} \u{00b7} You are {side} vs {}",
                net.opponent
            )
        } else {
            format!("Reversi \u{2014} {status}")
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
