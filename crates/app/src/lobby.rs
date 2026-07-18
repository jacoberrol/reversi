//! The multiplayer lobby: presence/invite state and the themed egui UI.

use egui::{Align, Align2, Color32, Frame, Id, Layout, Margin, RichText, Rounding, Sense, Stroke};
use protocol::{PlayerId, PlayerInfo};

/// What the player did in the lobby this frame.
pub enum LobbyAction {
    Invite(PlayerId),
    Accept(PlayerId),
    Decline(PlayerId),
}

/// Lobby data the UI renders.
#[derive(Default)]
pub struct LobbyState {
    /// The local player's display name.
    pub me: String,
    /// Other players currently available.
    pub players: Vec<PlayerInfo>,
    /// An incoming invite: `(inviter id, inviter name)`.
    pub incoming: Option<(PlayerId, String)>,
    /// A short status line (waiting / declined / error).
    pub status: String,
}

const SUBTLE: Color32 = Color32::from_rgb(150, 160, 176);
const CARD: Color32 = Color32::from_rgb(26, 31, 40);
const ROW: Color32 = Color32::from_rgb(38, 44, 55);
const ONLINE: Color32 = Color32::from_rgb(84, 204, 128);
const ACCENT: Color32 = Color32::from_rgb(64, 132, 214);

/// Apply the game's dark, rounded, non-"windowy" theme to an egui context.
pub fn apply_theme(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};

    let mut style = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(13, 16, 21);
    v.override_text_color = Some(Color32::from_rgb(228, 231, 238));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(46, 52, 64);
    v.widgets.inactive.bg_fill = Color32::from_rgb(46, 52, 64);
    v.widgets.inactive.rounding = Rounding::same(8.0);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(222, 226, 233));
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(64, 72, 88);
    v.widgets.hovered.bg_fill = Color32::from_rgb(64, 72, 88);
    v.widgets.hovered.rounding = Rounding::same(8.0);
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.rounding = Rounding::same(8.0);
    v.selection.bg_fill = ACCENT;
    style.visuals = v;

    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(16.0, 9.0);
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(32.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(18.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(16.0, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_style(style);
}

/// Draw the lobby, pushing any actions the player took this frame.
pub fn ui(ctx: &egui::Context, state: &LobbyState, actions: &mut Vec<LobbyAction>) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.heading("Reversi");
            ui.label(RichText::new("Multiplayer lobby").color(SUBTLE));
            ui.add_space(6.0);
            ui.label(RichText::new(format!("You are {}", state.me)).size(16.0));
            if !state.status.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(&state.status).color(SUBTLE).size(14.0));
            }
            ui.add_space(20.0);

            Frame::none()
                .fill(CARD)
                .rounding(Rounding::same(14.0))
                .inner_margin(Margin::same(18.0))
                .show(ui, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new("Players online").strong().size(17.0));
                    ui.add_space(12.0);
                    if state.players.is_empty() {
                        ui.label(RichText::new("Waiting for others to join\u{2026}").color(SUBTLE));
                    }
                    for player in &state.players {
                        player_row(ui, player, actions);
                        ui.add_space(9.0);
                    }
                });
        });

        if let Some((from, name)) = &state.incoming {
            invite_modal(ctx, *from, name, actions);
        }
    });
}

fn player_row(ui: &mut egui::Ui, player: &PlayerInfo, actions: &mut Vec<LobbyAction>) {
    Frame::none()
        .fill(ROW)
        .rounding(Rounding::same(9.0))
        .inner_margin(Margin::symmetric(12.0, 9.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 5.0, ONLINE);
                ui.add_space(8.0);
                ui.label(RichText::new(&player.name).size(18.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Invite").clicked() {
                        actions.push(LobbyAction::Invite(player.id));
                    }
                });
            });
        });
}

fn invite_modal(ctx: &egui::Context, from: PlayerId, name: &str, actions: &mut Vec<LobbyAction>) {
    let screen = ctx.screen_rect();
    egui::Area::new(Id::new("lobby-dim"))
        .order(egui::Order::Middle)
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
        });

    egui::Area::new(Id::new("lobby-invite"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            Frame::none()
                .fill(Color32::from_rgb(30, 36, 46))
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::new(1.5, ACCENT))
                .inner_margin(Margin::same(20.0))
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(format!("{name} invites you to play")).size(18.0));
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.add_space(60.0);
                            if ui.button("Accept").clicked() {
                                actions.push(LobbyAction::Accept(from));
                            }
                            ui.add_space(8.0);
                            if ui.button("Decline").clicked() {
                                actions.push(LobbyAction::Decline(from));
                            }
                        });
                    });
                });
        });
}
