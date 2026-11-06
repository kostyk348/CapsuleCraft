//! Vector canvas block renderer and element generator.
//!
//! The field is infinite: elements live in world coordinates and the
//! viewport (`CanvasView`) pans and zooms over them. This is the engine PR
//! of the v2.3 canvas plan — arrows/flowchart shapes and the page-mode
//! switch will be built on top of these transforms.

use egui::{
    pos2, vec2, Align, Align2, Color32, CursorIcon, FontFamily, FontId, Frame, Layout, Margin,
    PointerButton, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::theme;
use crate::{hud, now_nanos, text_button};

/// Zoom bounds shared by the wheel, the toolbar and the persisted view.
pub const CANVAS_MIN_ZOOM: f32 = 0.2;
pub const CANVAS_MAX_ZOOM: f32 = 4.0;

/// The viewport over the infinite field: `screen = world * zoom + offset`,
/// in coordinates relative to the canvas rect's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasView {
    pub offset: Vec2,
    pub zoom: f32,
}

impl CanvasView {
    pub fn identity() -> Self {
        Self {
            offset: Vec2::ZERO,
            zoom: 1.0,
        }
    }

    /// Reads `data.view` (see `to_json`), tolerating missing or corrupt data.
    pub fn from_data(data: Option<&Value>) -> Self {
        let Some(view) = data.and_then(|d| d.get("view")) else {
            return Self::identity();
        };
        let x = view.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let y = view.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let zoom = view
            .get("zoom")
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32;
        if !x.is_finite() || !y.is_finite() || !zoom.is_finite() || zoom <= 0.0 {
            return Self::identity();
        }
        Self {
            offset: vec2(x, y),
            zoom: zoom.clamp(CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({"x": self.offset.x, "y": self.offset.y, "zoom": self.zoom})
    }

    pub fn world_to_screen(&self, world: Pos2) -> Pos2 {
        pos2(
            world.x * self.zoom + self.offset.x,
            world.y * self.zoom + self.offset.y,
        )
    }

    pub fn screen_to_world(&self, local: Pos2) -> Pos2 {
        pos2(
            (local.x - self.offset.x) / self.zoom,
            (local.y - self.offset.y) / self.zoom,
        )
    }

    /// Zooms keeping the world point under `local` anchored to the screen.
    /// The factor is recomputed after clamping, so the anchor stays exact
    /// even when the zoom limit cuts the gesture short.
    pub fn zoom_at(&mut self, local: Pos2, factor: f32) {
        let factor = if factor.is_finite() && factor > 0.0 {
            factor
        } else {
            1.0
        };
        let zoom = (self.zoom * factor).clamp(CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
        let applied = zoom / self.zoom;
        self.offset = local.to_vec2() - (local.to_vec2() - self.offset) * applied;
        self.zoom = zoom;
    }

    /// Drag-pan: the content follows the cursor (graph view's convention).
    pub fn pan_by(&mut self, delta: Vec2) {
        self.offset += delta;
    }

    /// Wheel-pan: the page's scroll convention (wheel down moves the view
    /// further down the field, so the content visually moves up).
    pub fn pan_by_scroll(&mut self, delta: Vec2) {
        self.offset -= delta;
    }

    /// Zoom percentage for the toolbar pill.
    pub fn percent(&self) -> u32 {
        (self.zoom * 100.0).round() as u32
    }
}

/// World grid spacing whose screen size stays legible at any zoom: 40 world
/// units at zoom 1, one ladder step coarser when zoomed out, one finer when
/// zoomed in — the screen spacing then stays within ~28..=57 px.
pub fn canvas_grid_spacing(zoom: f32) -> f32 {
    let steps = zoom.max(0.0001).log2().round();
    40.0 / 2f32.powf(steps)
}

/// A view centering the elements' bounding box inside `canvas_size`
/// (fit-to-content); with an empty field it resets to identity.
pub fn canvas_fit_view(elements: &[Value], canvas_size: Vec2) -> CanvasView {
    let mut bbox: Option<Rect> = None;
    for element in elements {
        let rect = element_rect(element);
        bbox = Some(match bbox {
            Some(b) => b.union(rect),
            None => rect,
        });
    }
    let Some(bbox) = bbox else {
        return CanvasView::identity();
    };
    let pad = 20.0;
    let avail = (canvas_size - Vec2::splat(pad * 2.0)).max(Vec2::splat(40.0));
    let zoom = (avail.x / bbox.width().max(1.0))
        .min(avail.y / bbox.height().max(1.0))
        .clamp(CANVAS_MIN_ZOOM, CANVAS_MAX_ZOOM);
    CanvasView {
        offset: canvas_size / 2.0 - bbox.center().to_vec2() * zoom,
        zoom,
    }
}

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
/// a long session cannot grow the frame store without bound. The viewport is
/// deliberately not snapshotted: panning is not a content change.
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

/// The element rectangle in world coordinates, straight from the data.
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
    let mut view = CanvasView::from_data(block.get("data"));
    let mut view_changed = false;

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
                        view = CanvasView::identity();
                        view_changed = true;
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
            let pan_key = egui::Id::new(("canvas-pan", block_id.clone()));
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

            // --- the infinite field: wheel pans, Ctrl+wheel / pinch zooms ---
            // The wheel is claimed while the pointer is over the field:
            // zeroing the scroll delta here, during the content pass, means
            // the page's ScrollArea reads zero in its end pass and stays
            // still — the same hand-off egui's own nested scrollables use.
            // Ctrl+wheel needs no claim: egui routes it into zoom_delta and
            // never fills smooth_scroll_delta in the first place.
            if ui.rect_contains_pointer(canvas_rect) {
                let scroll = ui.input(|i| i.smooth_scroll_delta);
                if scroll != Vec2::ZERO {
                    view.pan_by_scroll(scroll);
                    ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
                    view_changed = true;
                }
                let zoom_factor = ui.input(|i| i.zoom_delta());
                if zoom_factor != 1.0 {
                    if let Some(hover) = ui.input(|i| i.pointer.hover_pos()) {
                        view.zoom_at(hover - canvas_rect.min.to_vec2(), zoom_factor);
                        view_changed = true;
                    }
                }
            }

            // --- grid in world space, one legible ladder step per zoom ---
            {
                let spacing = canvas_grid_spacing(view.zoom);
                let stroke = Stroke::new(1.0, theme::fade(theme::line(), 0.55));
                let first_x = ((-view.offset.x / view.zoom) / spacing).floor() * spacing;
                let first_y = ((-view.offset.y / view.zoom) / spacing).floor() * spacing;
                let cols = (canvas_rect.width() / (spacing * view.zoom)).ceil() as i32 + 1;
                let rows = (canvas_rect.height() / (spacing * view.zoom)).ceil() as i32 + 1;
                for i in 0..=cols {
                    let sx = canvas_rect.min.x + (first_x + i as f32 * spacing) * view.zoom
                        + view.offset.x;
                    painter.vline(sx, canvas_rect.y_range(), stroke);
                }
                for j in 0..=rows {
                    let sy = canvas_rect.min.y + (first_y + j as f32 * spacing) * view.zoom
                        + view.offset.y;
                    painter.hline(canvas_rect.x_range(), sy, stroke);
                }
            }

            let selection: Vec<String> = ui
                .data(|data| data.get_temp(selection_key))
                .unwrap_or_default();
            let mut next_selection = selection.clone();
            let marquee_origin: Option<Pos2> =
                ui.data(|data| data.get_temp(marquee_key)).unwrap_or(None);

            // --- background gestures: marquee (left), pan (middle/space) ---
            if canvas_resp.clicked() {
                next_selection.clear();
            }
            let space_held = ui.input(|i| i.key_down(egui::Key::Space));
            if space_held && ui.rect_contains_pointer(canvas_rect) {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if canvas_resp.drag_started_by(PointerButton::Middle)
                || (canvas_resp.drag_started_by(PointerButton::Primary) && space_held)
            {
                ui.data_mut(|data| data.insert_temp(pan_key, true));
            }
            let mut pan_mode: bool = ui.data(|data| data.get_temp(pan_key)).unwrap_or(false);
            if pan_mode
                && (canvas_resp.dragged_by(PointerButton::Middle)
                    || canvas_resp.dragged_by(PointerButton::Primary))
            {
                view.pan_by(canvas_resp.drag_delta());
                view_changed = true;
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            if canvas_resp.drag_stopped() {
                if pan_mode {
                    pan_mode = false;
                    ui.data_mut(|data| data.insert_temp(pan_key, false));
                } else {
                    ui.data_mut(|data| data.insert_temp(marquee_key, None::<Pos2>));
                }
            }

            // The marquee lives in world coordinates so it stays honest at
            // any zoom; it is drawn back on screen through the same transform.
            if canvas_resp.drag_started_by(PointerButton::Primary) && !pan_mode && !space_held {
                let local = canvas_resp
                    .interact_pointer_pos()
                    .unwrap_or(canvas_rect.min)
                    - canvas_rect.min.to_vec2();
                ui.data_mut(|data| data.insert_temp(marquee_key, Some(view.screen_to_world(local))));
            }
            let mut marquee_now: Option<Rect> = None;
            let mut marquee_done: Option<Rect> = None;
            if canvas_resp.dragged_by(PointerButton::Primary) && !pan_mode {
                if let Some(origin) = marquee_origin {
                    let local = canvas_resp
                        .interact_pointer_pos()
                        .unwrap_or(canvas_rect.min)
                        - canvas_rect.min.to_vec2();
                    let cursor_world = view.screen_to_world(local);
                    marquee_now = Some(Rect::from_min_max(
                        origin.min(cursor_world),
                        origin.max(cursor_world),
                    ));
                }
            } else if marquee_origin.is_some() && !pan_mode && canvas_resp.drag_stopped() {
                if let Some(origin) = marquee_origin {
                    let local = canvas_resp
                        .interact_pointer_pos()
                        .unwrap_or(canvas_rect.min)
                        - canvas_rect.min.to_vec2();
                    let cursor_world = view.screen_to_world(local);
                    let marquee = Rect::from_min_max(origin.min(cursor_world), origin.max(cursor_world));
                    let screen = Rect::from_min_max(
                        view.world_to_screen(marquee.min),
                        view.world_to_screen(marquee.max),
                    );
                    if screen.width() + screen.height() > 8.0 {
                        marquee_done = Some(marquee);
                    }
                }
            }
            if let Some(marquee) = marquee_now.or(marquee_done) {
                let screen = Rect::from_min_max(
                    canvas_rect.min + view.world_to_screen(marquee.min).to_vec2(),
                    canvas_rect.min + view.world_to_screen(marquee.max).to_vec2(),
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
            // reports the screen delta, the group applies it in world units.
            let mut group_delta: Option<Vec2> = None;
            for index in 0..elements.len() {
                let world_rect = element_rect(&elements[index]);
                let screen_rect = Rect::from_min_size(
                    canvas_rect.min + view.world_to_screen(world_rect.min).to_vec2(),
                    world_rect.size() * view.zoom,
                )
                .intersect(canvas_rect);
                if screen_rect.width() < 4.0 || screen_rect.height() < 4.0 {
                    continue;
                }
                let id = element_id(&elements[index]);
                let kind = elements[index]
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("sticky")
                    .to_string();
                let response = ui.interact(
                    screen_rect,
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
                    group_delta = Some(response.drag_delta() / view.zoom);
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
                        screen_rect,
                        if kind == "sticky" {
                            egui::CornerRadius::same((4.0 * view.zoom).round() as u8)
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
                        screen_rect,
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
                    screen_rect.min + vec2(8.0, 7.0) * view.zoom,
                    Align2::LEFT_TOP,
                    text,
                    FontId::new(
                        (11.5 * view.zoom).max(6.0),
                        FontFamily::Proportional,
                    ),
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
                    Align2::CENTER_CENTER,
                    t(locale, Key::CanvasEmpty),
                    FontId::new(11.5, FontFamily::Proportional),
                    theme::faint(),
                );
            }

            // --- viewport toolbar: zoom out / percent / in / fit ---
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                let center = pos2(canvas_rect.width() / 2.0, canvas_rect.height() / 2.0);
                if text_button(ui, None, "−", false).clicked() {
                    view.zoom_at(center, 1.0 / 1.25);
                    view_changed = true;
                }
                if text_button(ui, None, &format!("{}%", view.percent()), false).clicked() {
                    view.zoom_at(center, 1.0 / view.zoom);
                    view_changed = true;
                }
                if text_button(ui, None, "+", false).clicked() {
                    view.zoom_at(center, 1.25);
                    view_changed = true;
                }
                if text_button(ui, None, t(locale, Key::CanvasFit), false).clicked() {
                    view = canvas_fit_view(&elements, canvas_rect.size());
                    view_changed = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let _ = hud(ui, t(locale, Key::CanvasViewHint), theme::faint());
                });
            });

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
                data.insert_temp(pan_key, pan_mode);
            });
        });

    if changed {
        block["data"]["elements"] = Value::Array(elements);
    }
    if view_changed {
        block["data"]["view"] = view.to_json();
        changed = true;
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

    #[test]
    fn world_screen_round_trip() {
        let view = CanvasView {
            offset: vec2(37.0, -18.0),
            zoom: 2.5,
        };
        for p in [pos2(0.0, 0.0), pos2(12.0, -4.0), pos2(-30.0, 55.0)] {
            let screen = view.world_to_screen(p);
            assert!((view.screen_to_world(screen) - p).length() < 1e-4);
        }
        // Zoom 1 with zero offset is the identity the old boards rendered in.
        assert_eq!(
            CanvasView::identity().world_to_screen(pos2(5.0, 7.0)),
            pos2(5.0, 7.0)
        );
    }

    #[test]
    fn zoom_at_anchors_the_point_under_the_cursor() {
        let mut view = CanvasView {
            offset: vec2(10.0, 20.0),
            zoom: 1.0,
        };
        let local = pos2(80.0, 60.0);
        let world = view.screen_to_world(local);
        view.zoom_at(local, 2.0);
        assert!((view.zoom - 2.0).abs() < 1e-5);
        assert!((view.screen_to_world(local) - world).length() < 1e-4);
        // The clamp still anchors: zooming way in past the limit lands on
        // CANVAS_MAX_ZOOM with the same world point under the cursor.
        let mut view = CanvasView::identity();
        view.zoom_at(local, 1_000.0);
        assert_eq!(view.zoom, CANVAS_MAX_ZOOM);
        assert!((view.screen_to_world(local) - local).length() < 1e-4);
    }

    #[test]
    fn view_round_trips_through_json_with_sane_defaults() {
        let view = CanvasView {
            offset: vec2(-140.0, 88.0),
            zoom: 0.5,
        };
        assert_eq!(
            CanvasView::from_data(Some(&json!({ "view": view.to_json() }))),
            view
        );
        assert_eq!(CanvasView::from_data(None), CanvasView::identity());
        assert_eq!(CanvasView::from_data(Some(&json!({}))), CanvasView::identity());
        // Corrupt or dangerous values fall back or clamp.
        assert_eq!(
            CanvasView::from_data(Some(&json!({"view": {"zoom": -3.0}}))),
            CanvasView::identity()
        );
        assert_eq!(
            CanvasView::from_data(Some(&json!({"view": {"x": "nan", "y": {}, "zoom": 99.0}})))
                .zoom,
            CANVAS_MAX_ZOOM
        );
    }

    #[test]
    fn fit_view_centers_content_and_respects_bounds() {
        let elements = vec![
            element("a", 0.0, 0.0, 100.0, 80.0),
            element("b", 100.0, 0.0, 100.0, 80.0),
        ];
        let size = vec2(400.0, 300.0);
        let view = canvas_fit_view(&elements, size);
        // bbox 200×80, avail 360×260 → zoom = min(1.8, 3.25) = 1.8
        assert!((view.zoom - 1.8).abs() < 1e-4);
        let center = view.world_to_screen(pos2(100.0, 40.0));
        assert!((center - pos2(200.0, 150.0)).length() < 1e-4);
        assert_eq!(canvas_fit_view(&[], size), CanvasView::identity());
        // A huge board zooms out, but never past the floor.
        let big = vec![element("a", 0.0, 0.0, 100_000.0, 80.0)];
        assert_eq!(canvas_fit_view(&big, size).zoom, CANVAS_MIN_ZOOM);
    }

    #[test]
    fn grid_spacing_stays_legible_at_any_zoom() {
        for zoom in [0.2, 0.25, 0.5, 1.0, 1.7, 2.0, 4.0] {
            let spacing = canvas_grid_spacing(zoom);
            let on_screen = spacing * zoom;
            assert!(on_screen >= 24.0, "zoom {zoom}: {on_screen} px too dense");
            assert!(on_screen <= 64.0, "zoom {zoom}: {on_screen} px too sparse");
        }
    }
}
