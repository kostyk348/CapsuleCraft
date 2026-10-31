//! Vector canvas block renderer and element generator.

use egui::{
    vec2, Align, Color32, FontFamily, FontId, Frame, Layout, Margin, Rect, Sense, Stroke, Ui,
};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::theme;
use crate::{hud, now_nanos, text_button};

pub fn canvas_element(kind: &str, index: usize, locale: Locale) -> Value {
    let offset = (index % 6) as f64 * 18.0;
    json!({
        "id": format!("el_{}", now_nanos()),
        "type": kind,
        "x": 24.0 + offset,
        "y": 24.0 + offset,
        "width": if kind == "text" { 180.0 } else { 150.0 },
        "height": if kind == "text" { 28.0 } else { 90.0 },
        "text": match kind {
            "rectangle" => String::new(),
            "text" => t(locale, Key::CanvasAddText).to_string(),
            _ => t(locale, Key::CanvasAddSticky).to_string(),
        },
        "color": "",
    })
}

pub fn draw_canvas_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    let block_id = block
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("canvas")
        .to_string();
    let mut elements: Vec<Value> = block
        .get("data")
        .and_then(|data| data.get("elements"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let _ = hud(ui, t(locale, Key::BlockCanvas), theme::FAINT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, None, t(locale, Key::CanvasReset), false).clicked() {
                        elements.clear();
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddText), false).clicked() {
                        elements.push(canvas_element("text", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddRect), false).clicked() {
                        elements.push(canvas_element("rectangle", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddSticky), false).clicked() {
                        elements.push(canvas_element("sticky", elements.len(), locale));
                        changed = true;
                    }
                });
            });
            ui.add_space(8.0);

            let (canvas_rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
            let painter = ui.painter_at(canvas_rect);
            painter.rect_filled(canvas_rect, theme::RADIUS, theme::BG);
            painter.rect_stroke(
                canvas_rect,
                theme::RADIUS,
                theme::hairline(),
                egui::StrokeKind::Inside,
            );

            let selected_id: Option<String> =
                ui.data(|data| data.get_temp(egui::Id::new(("canvas-selected", block_id.clone()))));
            let mut next_selected = selected_id.clone();

            for element in elements.iter_mut() {
                let x = element.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let y = element.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let width = element
                    .get("width")
                    .and_then(Value::as_f64)
                    .unwrap_or(140.0) as f32;
                let height = element
                    .get("height")
                    .and_then(Value::as_f64)
                    .unwrap_or(80.0) as f32;
                let id = element
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let kind = element
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("sticky")
                    .to_string();
                let rect = Rect::from_min_size(canvas_rect.min + vec2(x, y), vec2(width, height))
                    .intersect(canvas_rect);
                if rect.width() < 4.0 || rect.height() < 4.0 {
                    continue;
                }
                let response = ui.interact(
                    rect,
                    egui::Id::new(("canvas-element", block_id.clone(), id.clone())),
                    Sense::click_and_drag(),
                );
                if response.dragged() {
                    let delta = response.drag_delta();
                    element["x"] = json!(x + delta.x);
                    element["y"] = json!(y + delta.y);
                    changed = true;
                }
                if response.clicked() {
                    next_selected = Some(id.clone());
                }

                let is_selected = selected_id.as_deref() == Some(id.as_str());
                let (fill, stroke) = match kind.as_str() {
                    "rectangle" => (theme::WASH, theme::LINE_STRONG),
                    "text" => (Color32::TRANSPARENT, theme::fade(theme::LINE, 0.0)),
                    _ => (
                        Color32::from_rgb(45, 129, 255),
                        theme::fade(theme::ACCENT_INK, 0.4),
                    ),
                };
                if kind != "text" {
                    painter.rect(
                        rect,
                        if kind == "sticky" {
                            egui::CornerRadius::same(4)
                        } else {
                            theme::RADIUS
                        },
                        fill,
                        Stroke::new(
                            if is_selected { 1.6 } else { 1.0 },
                            if is_selected {
                                theme::ACCENT_INK
                            } else {
                                stroke
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                } else if is_selected {
                    painter.rect_stroke(
                        rect,
                        theme::RADIUS,
                        Stroke::new(1.0, theme::ACCENT_INK),
                        egui::StrokeKind::Inside,
                    );
                }
                let text = element
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                painter.text(
                    rect.min + vec2(8.0, 7.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::new(11.5, FontFamily::Proportional),
                    if kind == "sticky" {
                        theme::ACCENT_INK
                    } else {
                        theme::INK
                    },
                );
            }

            if elements.is_empty() {
                painter.text(
                    canvas_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t(locale, Key::CanvasEmpty),
                    FontId::new(11.5, FontFamily::Proportional),
                    theme::FAINT,
                );
            }

            if let Some(selected) = &next_selected {
                let selected = selected.clone();
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    if text_button(ui, Some(Icon::Trash), t(locale, Key::CanvasDelete), true)
                        .clicked()
                    {
                        elements.retain(|element| {
                            element.get("id").and_then(Value::as_str) != Some(selected.as_str())
                        });
                        next_selected = None;
                        changed = true;
                    }
                    let _ = hud(ui, t(locale, Key::CanvasHint), theme::FAINT);
                });
            } else {
                ui.add_space(4.0);
                let _ = hud(ui, t(locale, Key::CanvasHint), theme::FAINT);
            }

            ui.data_mut(|data| {
                data.insert_temp(egui::Id::new(("canvas-selected", block_id)), next_selected)
            });
        });

    if changed {
        block["data"]["elements"] = Value::Array(elements);
    }
    changed
}
