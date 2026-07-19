//! The login / register screen: a themed panel with a username + password and
//! Log in / Create account buttons. Reuses the lobby's dark theme.

use egui::{Color32, Frame, Key, Margin, RichText, Rounding};

const SUBTLE: Color32 = Color32::from_rgb(150, 160, 176);
const CARD: Color32 = Color32::from_rgb(26, 31, 40);
const ERROR: Color32 = Color32::from_rgb(224, 108, 108);
const ACCENT: Color32 = Color32::from_rgb(96, 156, 230);

/// Editable login form state.
#[derive(Default)]
pub struct LoginForm {
    pub name: String,
    pub password: String,
    /// Last error to show (wrong login, name taken, …).
    pub error: Option<String>,
    /// A connection attempt is in flight (buttons disabled).
    pub connecting: bool,
}

/// What the login panel asked for this frame.
pub enum LoginAction {
    /// Connect: log in (`register` false) or create the account (`register` true).
    Submit { register: bool },
}

/// Draw the login screen, pushing any action the player took this frame.
pub fn ui(ctx: &egui::Context, form: &mut LoginForm, actions: &mut Vec<LoginAction>) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.heading("Reversi");
            ui.label(RichText::new("Log in or create an account to play online").color(SUBTLE));
            ui.add_space(24.0);

            Frame::none()
                .fill(CARD)
                .rounding(Rounding::same(14.0))
                .inner_margin(Margin::same(20.0))
                .show(ui, |ui| {
                    ui.set_width(320.0);

                    ui.label(RichText::new("Username").color(SUBTLE).size(14.0));
                    let name = ui.add(
                        egui::TextEdit::singleline(&mut form.name)
                            .desired_width(f32::INFINITY)
                            .hint_text(RichText::new("username").color(SUBTLE)),
                    );
                    ui.add_space(10.0);

                    ui.label(RichText::new("Password").color(SUBTLE).size(14.0));
                    let password = ui.add(
                        egui::TextEdit::singleline(&mut form.password)
                            .password(true)
                            .desired_width(f32::INFINITY)
                            .hint_text(RichText::new("password").color(SUBTLE)),
                    );

                    // Enter in either field acts as Log in.
                    let enter = (name.lost_focus() || password.lost_focus())
                        && ui.input(|i| i.key_pressed(Key::Enter));

                    if let Some(err) = &form.error {
                        ui.add_space(8.0);
                        ui.label(RichText::new(err).color(ERROR).size(14.0));
                    }

                    ui.add_space(18.0);
                    ui.vertical_centered(|ui| {
                        ui.add_enabled_ui(!form.connecting, |ui| {
                            let login = ui
                                .add(egui::Button::new("Log in").min_size(egui::vec2(160.0, 38.0)));
                            if login.clicked() || (enter && !form.connecting) {
                                actions.push(LoginAction::Submit { register: false });
                            }
                            ui.add_space(12.0);
                            if ui
                                .link(RichText::new("create a new account").color(ACCENT))
                                .clicked()
                            {
                                actions.push(LoginAction::Submit { register: true });
                            }
                        });
                    });

                    if form.connecting {
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Connecting\u{2026}").color(SUBTLE));
                        });
                    }
                });
        });
    });
}
