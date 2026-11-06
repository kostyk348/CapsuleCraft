//! Vector canvas block renderer and element generator.

use egui::{
    pos2, vec2, Align, Color32, FontFamily, FontId, Frame, Layout, Margin, Rect, Sense, Stroke, Ui,
    Vec2,
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

fn undo_key(block_id: &str) -> egui::Id {
    egui::Id::new(("canvas-undo", block_id.to_string()))
}

/// Snapshot the current element list so the next undo returns here. Capped so
/// a long session cannot grow the frame store without bound.
fn push_undo(ui: &mut Ui, block_id: &str, elements: &[Value]) {
    ui.data_mut(|data| {
        let stack: &mut Vec<Vec<Value>> = data.get_temp_mut_or(undo_key(block_id), Vec::new());
        stack.push(elements.to_vec());
        if stack.len() > 40 {
            stack.remove(0);
        }
    });
}

fn pop_undo(ui: &mut Ui, block_id: &str) -> Option<Vec<Value>> {
    ui.data_mut(|data| {
        let stack: &mut Vec<Vec<Value>> = data.get_temp_mut_or(undo_key(block_id), Vec::new());
        stack.pop()
    })
}

/// The element rectangle in canvas-local coordinates, straight from the data.
fn element_rect(element: &Value) -> Rect {
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
    Rect::from_min_size(pos2(x, y), vec2(width, height))
}

fn element_id(element: &Value) -> String {
    element
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Ids of elements whose rect intersects the marquee, in element order.
fn element_ids_in_rect(elements: &[Value], marquee: Rect) -> Vec<String> {
    elements
        .iter()
        .filter(|element| element_rect(element).intersects(marquee))
        .map(element_id)
        .collect()
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
        .fill(theme::sunken())
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let _ = hud(ui, t(locale, Key::BlockCanvas), theme::faint());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, None, t(locale, Key::CanvasReset), false).clicked() {
                        push_undo(ui, &block_id, &elements);
                        elements.clear();
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddText), false).clicked() {
                        push_undo(ui, &block_id, &elements);
                        elements.push(canvas_element("text", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddRect), false).clicked() {
                        push_undo(ui, &block_id, &elements);
                        elements.push(canvas_element("rectangle", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddSticky), false).clicked() {
                        push_undo(ui, &block_id, &elements);
                        elements.push(canvas_element("sticky", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, Some(Icon::Undo), t(locale, Key::CanvasUndo), false)
                        .clicked()
                    {
                        if let Some(previous) = pop_undo(ui, &block_id) {
                            elements = previous;
                            changed = true;
                        }
                    }
                });
            });
            ui.add_space(8.0);

            let selection_key = egui::Id::new(("canvas-selection", block_id.clone()));
            let marquee_key = egui::Id::new(("canvas-marquee", block_id.clone()));
            let (canvas_rect, canvas_resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::click_and_drag());
            let painter = ui.painter_at(canvas_rect);
            painter.rect_filled(canvas_rect, theme::RADIUS, theme::bg());
            painter.rect_stroke(
                canvas_rect,
                theme::RADIUS,
                theme::hairline(),
                egui::StrokeKind::Inside,
            );

            let selection: Vec<String> = ui
                .data(|data| data.get_temp(selection_key))
                .unwrap_or_default();
            let mut next_selection = selection.clone();
            let marquee_origin: Option<egui::Pos2> =
                ui.data(|data| data.get_temp(marquee_key)).unwrap_or(None);

            // Empty-canvas click clears the selection; empty-canvas drag is a
            // marquee. Elements registered below sit on top and claim their
            // own drags, so the two never fight.
            if canvas_resp.clicked() {
                next_selection.clear();
            }
            if canvas_resp.drag_started() {
                let local = canvas_resp
                    .interact_pointer_pos()
                    .unwrap_or(canvas_rect.min)
                    - canvas_rect.min.to_vec2();
                ui.data_mut(|data| data.insert_temp(marquee_key, Some(local)));
            }
            let mut marquee_now: Option<Rect> = None;
            let mut marquee_done: Option<Rect> = None;
            if canvas_resp.dragged() {
                if let Some(origin) = marquee_origin {
                    let local = canvas_resp
                        .interact_pointer_pos()
                        .unwrap_or(canvas_rect.min)
                        - canvas_rect.min.to_vec2();
                    marquee_now = Some(Rect::from_min_max(origin.min(local), origin.max(local)));
                }
            } else if canvas_resp.drag_stopped() {
                if let Some(origin) = marquee_origin {
                    let local = canvas_resp
                        .interact_pointer_pos()
                        .unwrap_or(canvas_rect.min)
                        - canvas_rect.min.to_vec2();
                    let marquee = Rect::from_min_max(origin.min(local), origin.max(local));
                    if marquee.width() + marquee.height() > 8.0 {
                        marquee_done = Some(marquee);
                    }
                    ui.data_mut(|data| data.insert_temp(marquee_key, None::<egui::Pos2>));
                }
            }
            if let Some(marquee) = marquee_now.or(marquee_done) {
                let screen = Rect::from_min_max(
                    canvas_rect.min + marquee.min.to_vec2(),
                    canvas_rect.min + marquee.max.to_vec2(),
                );
                painter.rect_filled(
                    screen.intersect(canvas_rect),
                    0.0,
                    theme::fade(theme::accent(), 0.08),
                );
                painter.rect_stroke(
                    screen.intersect(canvas_rect),
                    0.0,
                    Stroke::new(1.0, theme::fade(theme::accent_ink(), 0.7)),
                    egui::StrokeKind::Inside,
                );
            }
            if let Some(marquee) = marquee_done {
                next_selection = element_ids_in_rect(&elements, marquee);
            }

            // One drag gesture moves the whole selection: the grabbed element
            // reports the delta, the group applies it in a second pass.
            let mut group_delta: Option<Vec2> = None;
            for index in 0..elements.len() {
                let (x, y) = {
                    let rect = element_rect(&elements[index]);
                    (rect.min.x, rect.min.y)
                };
                let id = element_id(&elements[index]);
                let kind = elements[index]
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("sticky")
                    .to_string();
                let rect = Rect::from_min_size(
                    canvas_rect.min + vec2(x, y),
                    element_rect(&elements[index]).size(),
                )
                .intersect(canvas_rect);
                if rect.width() < 4.0 || rect.height() < 4.0 {
                    continue;
                }
                let response = ui.interact(
                    rect,
                    egui::Id::new(("canvas-element", block_id.clone(), id.clone())),
                    Sense::click_and_drag(),
                );
                if response.drag_started() {
                    push_undo(ui, &block_id, &elements);
                    if !next_selection.contains(&id) {
                        next_selection = vec![id.clone()];
                    }
                }
                if response.dragged() {
                    group_delta = Some(response.drag_delta());
                }
                if response.clicked() {
                    next_selection = vec![id.clone()];
                }

                let is_selected = selection.contains(&id);
                let (fill, stroke) = match kind.as_str() {
                    "rectangle" => (theme::wash(), theme::line_strong()),
                    "text" => (Color32::TRANSPARENT, theme::fade(theme::line(), 0.0)),
                    _ => (
                        Color32::from_rgb(45, 129, 255),
                        theme::fade(theme::accent_ink(), 0.4),
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
                                theme::accent_ink()
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
                        Stroke::new(1.0, theme::accent_ink()),
                        egui::StrokeKind::Inside,
                    );
                }
                let text = elements[index]
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
                        theme::accent_ink()
                    } else {
                        theme::ink()
                    },
                );
            }
            if let Some(delta) = group_delta {
                let affected = next_selection.clone();
                for element in elements.iter_mut() {
                    if affected.contains(&element_id(element)) {
                        let rect = element_rect(element);
                        element["x"] = json!(rect.min.x + delta.x);
                        element["y"] = json!(rect.min.y + delta.y);
                        changed = true;
                    }
                }
            }

            if elements.is_empty() {
                painter.text(
                    canvas_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t(locale, Key::CanvasEmpty),
                    FontId::new(11.5, FontFamily::Proportional),
                    theme::faint(),
                );
            }

            if !next_selection.is_empty() {
                let doomed = next_selection.clone();
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    if text_button(ui, Some(Icon::Trash), t(locale, Key::CanvasDelete), true)
                        .clicked()
                    {
                        push_undo(ui, &block_id, &elements);
                        elements.retain(|element| !doomed.contains(&element_id(element)));
                        next_selection.clear();
                        changed = true;
                    }
                    let _ = hud(ui, t(locale, Key::CanvasHint), theme::faint());
                });
            } else {
                ui.add_space(4.0);
                let _ = hud(ui, t(locale, Key::CanvasHint), theme::faint());
            }

            ui.data_mut(|data| {
                data.insert_temp(selection_key, next_selection);
            });
        });

    if changed {
        block["data"]["elements"] = Value::Array(elements);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
        json!({"id": id, "type": "sticky", "x": x, "y": y, "width": w, "height": h})
    }

    #[test]
    fn marquee_selects_intersecting_elements() {
        let elements = vec![
            element("a", 10.0, 10.0, 50.0, 40.0),
            element("b", 200.0, 200.0, 50.0, 40.0),
            element("c", 25.0, 30.0, 50.0, 40.0), // overlaps a
        ];
        let marquee = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        let ids = element_ids_in_rect(&elements, marquee);
        assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn tiny_marquee_selects_nothing() {
        let elements = vec![element("a", 10.0, 10.0, 50.0, 40.0)];
        let marquee = Rect::from_min_max(pos2(400.0, 250.0), pos2(401.0, 251.0));
        assert!(element_ids_in_rect(&elements, marquee).is_empty());
    }

    #[test]
    fn marquee_touching_an_edge_counts() {
        // An element whose rect only touches the marquee edge is inside —
        // `intersects` is inclusive, which matches what the eye expects.
        let elements = vec![element("a", 0.0, 0.0, 50.0, 40.0)];
        let marquee = Rect::from_min_max(pos2(50.0, 0.0), pos2(90.0, 30.0));
        assert_eq!(element_ids_in_rect(&elements, marquee), vec!["a"]);
    }

    #[test]
    fn element_rect_reads_data_or_falls_back() {
        assert_eq!(
            element_rect(&element("a", 3.0, 4.0, 50.0, 40.0)),
            Rect::from_min_size(pos2(3.0, 4.0), vec2(50.0, 40.0))
        );
        let empty = json!({"id": "z"});
        assert_eq!(
            element_rect(&empty),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(140.0, 80.0))
        );
    }
}
