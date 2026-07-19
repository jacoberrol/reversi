//! Game/lobby session state, independent of wgpu/windowing.
//!
//! Owns the current [`Screen`], the [`Game`] + [`Animator`], and (in network
//! mode) the connection and lobby state. `gpu::WindowState` delegates input,
//! network events, and per-frame render inputs here.

use std::time::Instant;

use game_core::{Board, Outcome, Square};
use netplay_client::{NetEvent, NetHandle};
use render::board_view::{PieceAnim, View};

use crate::anim::Animator;
use crate::game::Game;
use crate::game_msg::{self, GameMsg};
use crate::lobby::{LobbyAction, LobbyState};
use crate::login::LoginForm;

/// Which screen is showing.
pub enum Screen {
    /// Log in / create an account (network mode, before connecting).
    Login,
    Lobby,
    InGame,
}

/// Why an in-progress network game ended (frozen board, message in the title).
enum EndReason {
    OpponentLeft,
    Disconnected,
    Error(String),
}

pub struct Session {
    screen: Screen,
    game: Game,
    animator: Animator,
    /// The connection write handle (network mode). `None` = single-player, or a
    /// failed connection.
    net: Option<NetHandle>,
    opponent: String,
    ended: Option<EndReason>,
    lobby: LobbyState,
    login: LoginForm,
}

impl Session {
    /// A local single-player session (vs the AI), straight into the game.
    pub fn new() -> Self {
        Self {
            screen: Screen::InGame,
            game: Game::new(),
            animator: Animator::default(),
            net: None,
            opponent: String::new(),
            ended: None,
            lobby: LobbyState::default(),
            login: LoginForm::default(),
        }
    }

    /// Enter network mode on the login screen, pre-filling `name`.
    pub fn begin_login(&mut self, name: String) {
        self.screen = Screen::Login;
        self.login.name = name;
    }

    /// A connection attempt is underway (holds the send handle; still on the
    /// login screen until the server confirms).
    pub fn start_connecting(&mut self, handle: NetHandle) {
        self.net = Some(handle);
        self.login.connecting = true;
        self.login.error = None;
    }

    /// The connection couldn't even be attempted; show the error on the form.
    pub fn login_error(&mut self, message: String) {
        self.login.error = Some(message);
        self.login.connecting = false;
        self.net = None;
    }

    pub fn login_connecting(&self) -> bool {
        self.login.connecting
    }

    pub fn is_login(&self) -> bool {
        matches!(self.screen, Screen::Login)
    }

    pub fn login_form(&self) -> &LoginForm {
        &self.login
    }

    pub fn login_form_mut(&mut self) -> &mut LoginForm {
        &mut self.login
    }

    /// Screens driven by egui (login + lobby) get pointer/keyboard input fed in.
    pub fn uses_egui(&self) -> bool {
        matches!(self.screen, Screen::Login | Screen::Lobby)
    }

    pub fn is_lobby(&self) -> bool {
        matches!(self.screen, Screen::Lobby)
    }

    pub fn is_network(&self) -> bool {
        self.net.is_some()
    }

    pub fn is_animating(&self) -> bool {
        self.animator.is_active()
    }

    pub fn lobby_state(&self) -> &LobbyState {
        &self.lobby
    }

    /// Act on a lobby button press.
    pub fn lobby_action(&mut self, action: LobbyAction) {
        match action {
            LobbyAction::Invite(id) => {
                if let Some(net) = &mut self.net {
                    net.invite(id);
                }
                self.lobby.status = "Invite sent\u{2026}".to_string();
            }
            LobbyAction::Accept(id) => {
                if let Some(net) = &mut self.net {
                    net.accept(id);
                }
                self.lobby.incoming = None;
            }
            LobbyAction::Decline(id) => {
                if let Some(net) = &mut self.net {
                    net.decline(id);
                }
                self.lobby.incoming = None;
            }
        }
    }

    pub fn set_difficulty_index(&mut self, index: usize) -> bool {
        if self.net.is_some() {
            return false; // no AI in network mode
        }
        match crate::game::Difficulty::from_index(index) {
            Some(difficulty) => {
                self.game.set_difficulty(difficulty);
                true
            }
            None => false,
        }
    }

    /// Start a new game (in-game only). In network mode, tell the opponent.
    pub fn restart(&mut self) {
        if self.is_lobby() || self.ended.is_some() {
            return;
        }
        self.animator.clear();
        self.game.restart();
        if let Some(net) = &mut self.net {
            net.game(game_msg::encode(&GameMsg::Restart));
        }
    }

    /// Handle a click on board square `sq` (in-game). Returns whether something
    /// changed.
    pub fn click_square(&mut self, sq: Square) -> bool {
        if self.is_lobby() || self.ended.is_some() || self.animator.is_active() {
            return false;
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
                net.game(game_msg::encode(&GameMsg::Move { square }));
            }
        }
        self.animator.push(transitions);
        true
    }

    /// Apply a network event. Returns whether a redraw is needed.
    pub fn on_net_event(&mut self, event: NetEvent) -> bool {
        // On the login screen, the first success enters the lobby and any
        // failure returns to the form with an error.
        if self.is_login() {
            match event {
                NetEvent::Presence(players) => {
                    self.lobby.me = self.login.name.clone();
                    self.login.connecting = false;
                    self.lobby.players = players;
                    self.lobby.status = if self.lobby.players.is_empty() {
                        "Waiting for others to join\u{2026}".to_string()
                    } else {
                        String::new()
                    };
                    self.screen = Screen::Lobby;
                }
                NetEvent::Error(message) => self.login_error(message),
                NetEvent::Disconnected => self.login_error("could not connect".to_string()),
                _ => {} // no other events before the lobby
            }
            return true;
        }
        match event {
            NetEvent::Presence(players) => {
                self.lobby.players = players;
                self.lobby.status = if self.lobby.players.is_empty() {
                    "Waiting for others to join\u{2026}".to_string()
                } else {
                    String::new()
                };
            }
            NetEvent::Invited { from, name } => {
                self.lobby.incoming = Some((from, name));
            }
            NetEvent::InviteDeclined { .. } => {
                self.lobby.status = "Invite declined".to_string();
            }
            NetEvent::Matched { seat, opponent } => {
                self.game.set_local(game_msg::player_of(seat));
                self.game.restart();
                self.animator.clear();
                self.opponent = opponent;
                self.ended = None;
                self.lobby.incoming = None;
                self.screen = Screen::InGame;
            }
            NetEvent::Game(payload) => match game_msg::decode(&payload) {
                Some(GameMsg::Move { square }) => {
                    if let Some(sq) = Square::from_index(square as usize) {
                        if let Some(transitions) = self.game.apply_remote_move(sq) {
                            self.animator.push(transitions);
                        }
                    }
                }
                Some(GameMsg::Restart) => {
                    self.animator.clear();
                    self.game.restart();
                }
                Some(GameMsg::Resign) => {
                    self.ended = Some(EndReason::OpponentLeft);
                }
                None => {} // ignore a malformed payload
            },
            NetEvent::OpponentLeft => {
                self.ended = Some(EndReason::OpponentLeft);
            }
            NetEvent::Disconnected => {
                if self.is_lobby() {
                    self.lobby.status = "Disconnected".to_string();
                } else {
                    self.ended = Some(EndReason::Disconnected);
                }
            }
            NetEvent::Error(message) => {
                if self.is_lobby() {
                    self.lobby.status = format!("Error: {message}");
                } else {
                    self.ended = Some(EndReason::Error(message));
                }
            }
        }
        true
    }

    /// The board and disc animations to draw this frame (in-game).
    pub fn frame(&mut self, now: Instant) -> (Board, Vec<PieceAnim>) {
        match self.animator.frame(now) {
            Some((board, anims)) => (board, anims),
            None => (self.game.board().clone(), Vec::new()),
        }
    }

    /// The non-board draw settings for this frame (in-game).
    pub fn view(&self, animating: bool) -> View {
        View {
            show_hints: !animating && self.ended.is_none() && self.game.awaiting_local(),
            show_controls: self.net.is_none(),
            selected_difficulty: self.game.difficulty().index(),
            outcome: if animating { None } else { self.game.outcome() },
        }
    }

    /// The window title reflecting the current state.
    pub fn title(&self) -> String {
        if self.is_login() {
            return "Reversi \u{2014} Log in".to_string();
        }
        if self.is_lobby() {
            return "Reversi \u{2014} Lobby".to_string();
        }
        if let Some(reason) = &self.ended {
            let message = match reason {
                EndReason::OpponentLeft => "Opponent left".to_string(),
                EndReason::Disconnected => "Disconnected".to_string(),
                EndReason::Error(e) => format!("Error: {e}"),
            };
            return format!("Reversi \u{2014} {message}");
        }
        if self.net.is_some() {
            self.network_title()
        } else {
            self.single_player_title()
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

    fn network_title(&self) -> String {
        let (me, opp) = self.game.score();
        let status = if self.game.is_over() {
            match self.game.outcome() {
                Some(Outcome::Win(p)) if p == self.game.local() => {
                    format!("You win {me}\u{2013}{opp} \u{00b7} click board for a new game")
                }
                Some(Outcome::Win(_)) => {
                    format!("You lose {opp}\u{2013}{me} \u{00b7} click board for a new game")
                }
                _ => format!("Draw {me}\u{2013}{opp} \u{00b7} click board for a new game"),
            }
        } else if self.game.awaiting_local() {
            "Your move".to_string()
        } else {
            "Opponent's move".to_string()
        };
        let side = match self.game.local() {
            game_core::Player::Black => "Black",
            game_core::Player::White => "White",
        };
        format!(
            "Reversi \u{2014} {status} \u{00b7} You are {side} vs {}",
            self.opponent
        )
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
