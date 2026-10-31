//! IMAP sync settings and connection modal dialog.

use egui::{vec2, Align, Label, Layout, Rect, RichText, Sense, Stroke, TextEdit, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::sync_imap::{self, ImapConfig, SyncStatus};
use crate::theme;
use crate::{primary_button, secondary_button};

pub enum ImapModalAction {
    None,
    Close,
    SaveSettings,
    TestConnection,
    SyncNow,
}

pub fn draw_imap_modal(
    show: bool,
    config: &mut ImapConfig,
    status: &SyncStatus,
    locale: Locale,
    ctx: &egui::Context,
) -> ImapModalAction {
    if !show {
        return ImapModalAction::None;
    }

    let mut action = ImapModalAction::None;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = ImapModalAction::Close;
    }

    egui::Area::new(egui::Id::new("imap-sync-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            if resp.clicked() {
                action = ImapModalAction::Close;
            }
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(520.0, 480.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::PANEL,
                Stroke::new(1.0, theme::LINE_STRONG),
                egui::StrokeKind::Inside,
            );

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(22.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            body.label(
                RichText::new(t(locale, Key::ImapSyncTitle))
                    .family(theme::medium())
                    .size(15.0)
                    .color(theme::INK),
            );
            body.add_space(4.0);
            body.add(
                Label::new(
                    RichText::new(t(locale, Key::ImapSyncDesc))
                        .size(12.0)
                        .color(theme::MUTED),
                )
                .wrap(),
            );
            body.add_space(14.0);

            // Enable checkbox
            body.checkbox(&mut config.enabled, t(locale, Key::ImapEnable));
            body.add_space(8.0);

            // Fields
            body.label(
                RichText::new(t(locale, Key::ImapHost))
                    .size(11.0)
                    .color(theme::MUTED),
            );
            body.add(
                TextEdit::singleline(&mut config.host)
                    .hint_text("imap.gmail.com")
                    .desired_width(f32::INFINITY),
            );
            body.add_space(6.0);

            body.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(t(locale, Key::ImapPort))
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                    let mut port_str = config.port.to_string();
                    if ui
                        .add(TextEdit::singleline(&mut port_str).desired_width(100.0))
                        .changed()
                    {
                        if let Ok(p) = port_str.parse::<u16>() {
                            config.port = p;
                        }
                    }
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(t(locale, Key::ImapFolder))
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                    ui.add(
                        TextEdit::singleline(&mut config.folder)
                            .hint_text("CapsuleCraft")
                            .desired_width(f32::INFINITY),
                    );
                });
            });
            body.add_space(6.0);

            body.label(
                RichText::new(t(locale, Key::ImapUsername))
                    .size(11.0)
                    .color(theme::MUTED),
            );
            body.add(
                TextEdit::singleline(&mut config.username)
                    .hint_text("user@example.com")
                    .desired_width(f32::INFINITY),
            );
            body.add_space(6.0);

            body.label(
                RichText::new(t(locale, Key::ImapPassword))
                    .size(11.0)
                    .color(theme::MUTED),
            );
            body.add(
                TextEdit::singleline(&mut config.password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            body.add_space(16.0);

            // Status info
            let status_text = match status {
                SyncStatus::Idle => String::new(),
                SyncStatus::Syncing => t(locale, Key::ImapSyncing).to_string(),
                SyncStatus::Success(s) => s.clone(),
                SyncStatus::Error(e) => format!("Error: {e}"),
            };
            if !status_text.is_empty() {
                let color = match status {
                    SyncStatus::Error(_) => theme::BAD,
                    SyncStatus::Syncing => theme::BADGE_BETA,
                    _ => theme::ACCENT,
                };
                body.label(RichText::new(status_text).size(11.5).color(color));
                body.add_space(8.0);
            }

            body.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if secondary_button(ui, None, t(locale, Key::ImapTestConnection), 28.0).clicked() {
                    action = ImapModalAction::TestConnection;
                }
                if config.enabled {
                    ui.add_space(8.0);
                    if secondary_button(ui, Some(Icon::Reload), t(locale, Key::ImapSyncNow), 28.0)
                        .clicked()
                    {
                        action = ImapModalAction::SyncNow;
                    }
                }
            });
            body.add_space(10.0);
            body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, Some(Icon::Close), t(locale, Key::Close), 28.0).clicked() {
                    action = ImapModalAction::Close;
                }
                ui.add_space(8.0);
                if primary_button(ui, t(locale, Key::ImapSaveSettings), 28.0).clicked() {
                    action = ImapModalAction::SaveSettings;
                }
            });
        });

    action
}
