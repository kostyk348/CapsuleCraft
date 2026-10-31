//! Co-Thinker / AI opponent block renderer.

use egui::{Align, FontFamily, FontId, Frame, Layout, Margin, TextEdit, Ui};
use serde_json::{json, Value};

use crate::hud;
use crate::i18n::{t, Key, Locale};
use crate::theme;

pub fn draw_opponent_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 11,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let _ = hud(ui, t(locale, Key::CounterArg), theme::BADGE_BETA);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confidence = block
                        .get("data")
                        .and_then(|data| data.get("confidence"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.5);
                    let _ = hud(
                        ui,
                        &format!(
                            "{} {}%",
                            t(locale, Key::Confidence),
                            (confidence * 100.0).round() as i64
                        ),
                        theme::FAINT,
                    );
                });
            });
            ui.add_space(6.0);

            let mut criticism = block
                .get("data")
                .and_then(|data| data.get("criticism"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let rows = criticism.lines().count().max(2);
            let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                crate::links::layout(ui, buffer.as_str(), 13.5, wrap)
            };
            let response = ui.add(
                TextEdit::multiline(&mut criticism)
                    .font(FontId::new(13.5, FontFamily::Proportional))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .layouter(&mut layouter)
                    .hint_text(t(locale, Key::Critique)),
            );
            if response.changed() {
                block["data"]["criticism"] = json!(criticism);
                changed = true;
            }

            ui.add_space(8.0);
            let mut claim = block
                .get("data")
                .and_then(|data| data.get("sourceClaim"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let response = ui.add(
                TextEdit::singleline(&mut claim)
                    .font(FontId::new(11.5, FontFamily::Proportional))
                    .text_color(theme::MUTED)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .hint_text(t(locale, Key::SourceClaim)),
            );
            if response.changed() {
                block["data"]["sourceClaim"] = json!(claim);
                changed = true;
            }

            ui.add_space(4.0);
            let mut confidence = block
                .get("data")
                .and_then(|data| data.get("confidence"))
                .and_then(Value::as_f64)
                .unwrap_or(0.5) as f32;
            if ui
                .add(
                    egui::Slider::new(&mut confidence, 0.0..=1.0)
                        .show_value(false)
                        .trailing_fill(true),
                )
                .changed()
            {
                block["data"]["confidence"] = json!(confidence as f64);
                changed = true;
            }
            ui.add_space(4.0);
            let _ = hud(ui, t(locale, Key::HeuristicNotice), theme::FAINT);
        });
    changed
}
