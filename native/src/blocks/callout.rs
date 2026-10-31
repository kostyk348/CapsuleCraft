//! Callout / note block renderer with clean viewing and in-place editing.

use egui::{vec2, Color32, FontFamily, FontId, Frame, Margin, Rect, RichText, Sense, TextEdit, Ui};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::links;
use crate::theme;

pub fn draw_callout_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    let block_id = block.get("id").and_then(Value::as_str).unwrap_or("blk");
    let editing_id = ui.scope_id().with(("callout-editing", block_id));
    let mut is_editing: bool = ui.data(|d| d.get_temp(editing_id).unwrap_or(false));

    let tone = block
        .get("data")
        .and_then(|data| data.get("tone"))
        .and_then(Value::as_str)
        .unwrap_or("blue");
    let tone_color = match tone {
        "emerald" => Color32::from_rgb(52, 211, 153),
        "amber" => theme::BADGE_BETA,
        "purple" => Color32::from_rgb(167, 139, 250),
        "rose" => Color32::from_rgb(251, 113, 133),
        _ => theme::ACCENT,
    };

    let inner = Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 11,
            bottom: 11,
        })
        .show(ui, |ui| {
            let mut title = block
                .get("data")
                .and_then(|data| data.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();

            ui.horizontal(|ui| {
                if is_editing {
                    let title_response = ui.add(
                        TextEdit::singleline(&mut title)
                            .font(FontId::new(13.5, theme::semibold()))
                            .text_color(theme::INK)
                            .frame(Frame::NONE)
                            .margin(Margin::same(0))
                            .desired_width(ui.available_width() - 30.0)
                            .hint_text(t(locale, Key::BlockCallout)),
                    );
                    if title_response.changed() {
                        block["data"]["title"] = json!(title);
                        changed = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("✓").size(12.0).color(theme::ACCENT))
                                .fill(Color32::TRANSPARENT),
                        )
                        .clicked()
                    {
                        is_editing = false;
                        ui.data_mut(|d| d.insert_temp(editing_id, false));
                    }
                } else if !title.is_empty() {
                    ui.label(
                        RichText::new(&title)
                            .font(FontId::new(13.5, theme::semibold()))
                            .color(tone_color),
                    );
                }
            });
            ui.add_space(4.0);

            let mut content = block
                .get("data")
                .and_then(|data| data.get("content"))
                .and_then(Value::as_str)
                .or_else(|| {
                    block
                        .get("data")
                        .and_then(|data| data.get("text"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("")
                .to_string();

            if is_editing {
                let rows = content.lines().count().max(2);
                let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                    links::layout(ui, buffer.as_str(), 13.5, wrap)
                };
                let content_response = ui.add(
                    TextEdit::multiline(&mut content)
                        .font(FontId::new(13.5, FontFamily::Proportional))
                        .text_color(theme::MUTED)
                        .frame(Frame::NONE)
                        .margin(Margin::same(0))
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows)
                        .layouter(&mut layouter)
                        .hint_text("Write a note…"),
                );
                if content_response.changed() {
                    block["data"]["content"] = json!(content);
                    changed = true;
                }
            } else {
                let wrap_width = ui.available_width();
                let click_area = ui
                    .vertical(|ui| {
                        if content.trim().is_empty() {
                            ui.label(
                                RichText::new("Empty note (double click to edit)…")
                                    .size(12.0)
                                    .color(theme::FAINT),
                            );
                        }
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                ui.add_space(4.0);
                                continue;
                            }

                            if trimmed.starts_with("### ") {
                                ui.add_space(5.0);
                                ui.label(
                                    RichText::new(&trimmed[4..])
                                        .font(FontId::new(13.5, theme::semibold()))
                                        .color(theme::INK),
                                );
                            } else if trimmed.starts_with("## ") {
                                ui.add_space(7.0);
                                ui.label(
                                    RichText::new(&trimmed[3..])
                                        .font(FontId::new(15.0, theme::semibold()))
                                        .color(theme::INK),
                                );
                            } else if trimmed.starts_with("# ") {
                                ui.add_space(9.0);
                                ui.label(
                                    RichText::new(&trimmed[2..])
                                        .font(FontId::new(17.0, theme::semibold()))
                                        .color(theme::INK),
                                );
                            } else if let Some(dot_pos) = trimmed.find(". ") {
                                if trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit())
                                    && dot_pos <= 3
                                {
                                    let num = &trimmed[..dot_pos];
                                    let rest = trimmed[dot_pos + 2..].trim();
                                    ui.horizontal(|ui| {
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new(format!("{num}."))
                                                .font(FontId::new(11.5, theme::mono()))
                                                .color(tone_color),
                                        );
                                        let galley =
                                            links::layout_clean(ui, rest, 13.0, wrap_width - 24.0);
                                        ui.label(galley);
                                    });
                                } else {
                                    let galley = links::layout_clean(ui, line, 13.0, wrap_width);
                                    ui.label(galley);
                                }
                            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                                let rest = trimmed[2..].trim();
                                ui.horizontal(|ui| {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("•")
                                            .font(FontId::new(13.0, theme::semibold()))
                                            .color(tone_color),
                                    );
                                    let galley =
                                        links::layout_clean(ui, rest, 13.0, wrap_width - 20.0);
                                    ui.label(galley);
                                });
                            } else {
                                let galley = links::layout_clean(ui, line, 13.0, wrap_width);
                                ui.label(galley);
                            }
                        }
                    })
                    .response
                    .interact(Sense::click());

                if click_area.double_clicked() {
                    is_editing = true;
                    ui.data_mut(|d| d.insert_temp(editing_id, true));
                }
            }
        });

    ui.painter().rect_filled(
        Rect::from_min_size(
            inner.response.rect.min,
            vec2(2.0, inner.response.rect.height()),
        ),
        0.0,
        tone_color,
    );
    changed
}
