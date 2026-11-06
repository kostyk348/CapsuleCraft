//! Raw MIME inspector: the exact bytes this page is stored as.
//!
//! Ported from `src/components/RawMimeInspector.tsx`. The web view can rely on
//! the browser's text selection; here the body is a selectable monospace label
//! inside a two-axis scroll area, and copying uses egui's clipboard.

use egui::{vec2, Align, FontId, Frame, Layout, RichText, ScrollArea, Ui};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mime::Capsule;
use crate::theme;
use crate::{ghost, hud};

use super::{empty_state, header, ViewAction};

pub fn draw(ui: &mut Ui, capsule: &Capsule, locale: Locale) -> ViewAction {
    let mut action = ViewAction::default();
    let raw = capsule.export_eml();
    let (headers, body) = split_message(&raw);

    header(
        ui,
        t(locale, Key::MimeTitle),
        &format!(
            "{} · {} {}",
            capsule.file_name,
            raw.len(),
            t(locale, Key::Bytes)
        ),
    );

    let (_, bar_rect) = super::toolbar(ui);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(super::VIEW_PAD, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let _ = hud(&mut bar, t(locale, Key::MimeHint), theme::faint());

    bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ghost(
            ui,
            Icon::Download,
            Some(".eml"),
            t(locale, Key::ExportEml),
            true,
            26.0,
        )
        .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(t(locale, Key::ExportEml))
                .set_file_name(&capsule.file_name)
                .add_filter("email", &["eml"])
                .save_file()
            {
                let _ = std::fs::write(path, &raw);
            }
        }
        ui.add_space(6.0);
        if ghost(
            ui,
            Icon::Copy,
            Some(t(locale, Key::CopyMime)),
            t(locale, Key::CopyMime),
            true,
            26.0,
        )
        .clicked()
        {
            ui.ctx().copy_text(raw.clone());
            action.toast = Some(t(locale, Key::Copied).to_string());
        }
    });

    if raw.trim().is_empty() {
        empty_state(
            ui,
            Icon::Code,
            t(locale, Key::MimeTitle),
            t(locale, Key::EmptyVaultBody),
        );
        return action;
    }

    ScrollArea::both()
        .id_salt("mime-body")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(14.0);
            ui.horizontal_top(|ui| {
                ui.add_space(super::VIEW_PAD);
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - super::VIEW_PAD);
                    ui.label(
                        RichText::new(t(locale, Key::Headers))
                            .family(theme::semibold())
                            .size(11.5)
                            .color(theme::muted()),
                    );
                    ui.add_space(6.0);
                    for (name, value) in &headers {
                        ui.horizontal_top(|ui| {
                            ui.add_sized(
                                vec2(150.0, 16.0),
                                egui::Label::new(
                                    RichText::new(name)
                                        .family(theme::mono_bold())
                                        .size(9.5)
                                        .color(theme::faint()),
                                )
                                .truncate(),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(value)
                                        .family(theme::mono())
                                        .size(10.5)
                                        .color(theme::ink()),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .selectable(true),
                            );
                        });
                    }
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new(t(locale, Key::Body))
                            .family(theme::semibold())
                            .size(11.5)
                            .color(theme::muted()),
                    );
                    ui.add_space(6.0);
                    Frame::new()
                        .fill(theme::sunken())
                        .corner_radius(theme::RADIUS)
                        .stroke(theme::hairline())
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(body.trim_start_matches(['\r', '\n']))
                                        .family(FontId::new(10.5, theme::mono()).family)
                                        .size(10.5)
                                        .color(theme::muted()),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .selectable(true),
                            );
                        });
                    ui.add_space(40.0);
                });
            });
        });

    let _ = action.view.take();
    action
}

/// Split a message into `(header pairs, body)`. Header continuation lines are
/// folded back onto the previous value, exactly as a mail client sees them.
pub fn split_message(raw: &str) -> (Vec<(String, String)>, String) {
    let normalized = raw.replace("\r\n", "\n");
    let (head, body) = match normalized.find("\n\n") {
        Some(index) => (&normalized[..index], &normalized[index + 2..]),
        None => (normalized.as_str(), ""),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    (headers, body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_continuation_lines() {
        let raw = "From: a@b\r\nSubject: one\r\n two\r\n\r\nbody";
        let (headers, body) = split_message(raw);
        assert_eq!(headers[1], ("Subject".into(), "one two".into()));
        assert_eq!(body, "body");
    }
}
