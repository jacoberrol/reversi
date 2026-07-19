//! The login / create-account screen: a themed panel that toggles between a
//! log-in form and a create-account form. Reuses the lobby's dark theme.

use egui::{Color32, Frame, Key, Margin, RichText, Rounding};

const SUBTLE: Color32 = Color32::from_rgb(150, 160, 176);
const CARD: Color32 = Color32::from_rgb(26, 31, 40);
const ERROR: Color32 = Color32::from_rgb(224, 108, 108);
const ACCENT: Color32 = Color32::from_rgb(96, 156, 230);

/// Which form is showing.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Login,
    Register,
}

/// Editable form state (shared fields; `confirm` only used when registering).
#[derive(Default)]
pub struct LoginForm {
    pub name: String,
    pub password: String,
    pub confirm: String,
    /// Last error to show (wrong login, name taken, passwords differ, …).
    pub error: Option<String>,
    /// A connection attempt is in flight (buttons disabled).
    pub connecting: bool,
    pub mode: Mode,
}

/// What the panel asked for this frame.
pub enum LoginAction {
    /// Connect: log in (`register` false) or create the account (`register` true).
    Submit { register: bool },
}

/// Draw the login / create-account screen, pushing any action taken this frame.
pub fn ui(ctx: &egui::Context, form: &mut LoginForm, actions: &mut Vec<LoginAction>) {
    let register = form.mode == Mode::Register;
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.heading("Reversi");
            let subtitle = if register {
                "Create an account to play online"
            } else {
                "Log in to play online"
            };
            ui.label(RichText::new(subtitle).color(SUBTLE));
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

                    let mut enter = (name.lost_focus() || password.lost_focus())
                        && ui.input(|i| i.key_pressed(Key::Enter));

                    if register {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Confirm password").color(SUBTLE).size(14.0));
                        let confirm = ui.add(
                            egui::TextEdit::singleline(&mut form.confirm)
                                .password(true)
                                .desired_width(f32::INFINITY)
                                .hint_text(RichText::new("re-enter password").color(SUBTLE)),
                        );
                        enter = enter
                            || (confirm.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                    }

                    if let Some(err) = &form.error {
                        ui.add_space(8.0);
                        ui.label(RichText::new(err).color(ERROR).size(14.0));
                    }

                    ui.add_space(18.0);
                    ui.vertical_centered(|ui| {
                        ui.add_enabled_ui(!form.connecting, |ui| {
                            let label = if register { "Create Account" } else { "Log in" };
                            let button = ui
                                .add(egui::Button::new(label).min_size(egui::vec2(180.0, 38.0)));
                            if button.clicked() || (enter && !form.connecting) {
                                actions.push(LoginAction::Submit { register });
                            }
                            ui.add_space(12.0);
                            let link = if register {
                                "back to log in"
                            } else {
                                "create a new account"
                            };
                            if ui.link(RichText::new(link).color(ACCENT)).clicked() {
                                form.mode = if register { Mode::Login } else { Mode::Register };
                                form.error = None;
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
