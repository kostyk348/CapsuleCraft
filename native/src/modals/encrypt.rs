//! OpenPGP passphrase modal: encrypt the current capsule to `.eml.asc`, or
//! decrypt an `.eml.asc` file back into the vault.

use egui::{vec2, Align, Layout, Rect, RichText, Sense, Stroke, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::theme;
use crate::{primary_button, secondary_button};

/// What the modal is currently used for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncryptMode {
    /// Encrypt the capsule with this file name into `.eml.asc`.
    Encrypt { file_name: String },
    /// Decrypt this `.eml.asc` file and import the `.eml` inside.
    Decrypt { file_name: String },
}

#[derive(Default)]
pub struct EncryptState {
    pub open: bool,
    pub mode: Option<EncryptMode>,
    pub password: String,
    pub confirm: String,
    pub error: Option<String>,
}

impl EncryptState {
    pub fn open(&mut self, mode: EncryptMode) {
        self.open = true;
        self.mode = Some(mode);
        self.password.clear();
        self.confirm.clear();
        self.error = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.mode = None;
        self.password.clear();
        self.confirm.clear();
        self.error = None;
    }
}

pub enum EncryptAction {
    None,
    /// Confirmed with the entered passphrase (already validated: non-empty,
    /// and repeated correctly in encrypt mode).
    Confirm(String),
    Cancel,
}

pub fn draw_encrypt_modal(
    state: &mut EncryptState,
    locale: Locale,
    ctx: &egui::Context,
) -> EncryptAction {
    if !state.open {
        return EncryptAction::None;
    }
    let mut action = EncryptAction::None;
    let encrypting = matches!(state.mode, Some(EncryptMode::Encrypt { .. }));

    egui::Area::new(egui::Id::new("encrypt-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));
            let panel = Rect::from_center_size(rect.center(), vec2(420.0, 260.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::PANEL,
                Stroke::new(1.0, theme::LINE_STRONG),
                egui::StrokeKind::Inside,
            );
            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(20.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            let title = if encrypting {
                t(locale, Key::EncryptTitle)
            } else {
                t(locale, Key::DecryptTitle)
            };
            body.label(
                RichText::new(title)
                    .family(theme::medium())
                    .size(14.0)
                    .color(theme::INK),
            );
            if let Some(mode) = &state.mode {
                let file_name = match mode {
                    EncryptMode::Encrypt { file_name } | EncryptMode::Decrypt { file_name } => {
                        file_name
                    }
                };
                body.add_space(4.0);
                body.label(
                    RichText::new(format!("{}: {file_name}", t(locale, Key::EncryptSource)))
                        .size(11.5)
                        .color(theme::MUTED),
                );
            }
            body.add_space(4.0);
            body.label(
                RichText::new(t(locale, Key::EncryptHint))
                    .size(10.5)
                    .color(theme::FAINT),
            );
            body.add_space(10.0);

            let pass_label = if encrypting {
                t(locale, Key::Passphrase)
            } else {
                t(locale, Key::Passphrase)
            };
            body.label(
                RichText::new(pass_label)
                    .size(11.0)
                    .color(theme::MUTED),
            );
            let pass_response = body.add(
                egui::TextEdit::singleline(&mut state.password)
                    .password(true)
                    .font(egui::FontId::new(12.5, theme::mono()))
                    .desired_width(f32::INFINITY),
            );
            if encrypting {
                body.add_space(6.0);
                body.label(
                    RichText::new(t(locale, Key::RepeatPassphrase))
                        .size(11.0)
                        .color(theme::MUTED),
                );
                body.add(
                    egui::TextEdit::singleline(&mut state.confirm)
                        .password(true)
                        .font(egui::FontId::new(12.5, theme::mono()))
                        .desired_width(f32::INFINITY),
                );
            }

            if let Some(error) = &state.error {
                body.add_space(6.0);
                body.label(
                    RichText::new(error)
                        .size(11.5)
                        .color(theme::BAD),
                );
            }

            body.add_space(12.0);
            body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let confirm_label = if encrypting {
                    t(locale, Key::EncryptNow)
                } else {
                    t(locale, Key::DecryptNow)
                };
                if primary_button(ui, confirm_label, 28.0).clicked() {
                    if state.password.is_empty() {
                        state.error = Some(t(locale, Key::PassEmpty).to_string());
                    } else if encrypting && state.password != state.confirm {
                        state.error = Some(t(locale, Key::PassMismatch).to_string());
                    } else {
                        action = EncryptAction::Confirm(state.password.clone());
                    }
                }
                if secondary_button(ui, None, t(locale, Key::Cancel), 28.0).clicked() {
                    action = EncryptAction::Cancel;
                }
            });

            // Enter confirms, Escape cancels.
            if pass_response.lost_focus()
                && ui.input(|input| input.key_pressed(egui::Key::Enter))
            {
                if state.password.is_empty() {
                    state.error = Some(t(locale, Key::PassEmpty).to_string());
                } else if encrypting && state.password != state.confirm {
                    state.error = Some(t(locale, Key::PassMismatch).to_string());
                } else {
                    action = EncryptAction::Confirm(state.password.clone());
                }
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                action = EncryptAction::Cancel;
            }
        });

    match action {
        EncryptAction::Cancel => state.close(),
        _ => {}
    }
    action
}
