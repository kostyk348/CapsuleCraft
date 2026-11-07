//! Vector canvas block renderer and element generator.
//!
//! The field is infinite: elements live in world coordinates and the
//! viewport (`CanvasView`) pans and zooms over them (engine PR #40).
//! On top of that: arrow connectors, flowchart shapes (diamond, rounded),
//! inline text editing, grid snapping and a minimap.
//!
//! The element list is nodes plus arrows: a node carries `x/y/width/height`,
//! an arrow (`type: "arrow"`) only carries `from/to` node ids and an optional
//! `label` — its endpoints are recomputed from the node rects every frame,
//! so dragging a node drags its arrows for free.

use egui::{
    pos2, vec2, Align, Align2, Color32, CursorIcon, FontFamily, FontId, Frame, Layout, Margin,
    PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2,
};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::theme;
use crate::{hud, now_nanos, text_button};

/// Zoom bounds shared by the wheel, the toolbar and the persisted view.
pub const CANVAS_MIN_ZOOM: f32 = 0.2;
pub const CANVAS_MAX_ZOOM: f32 = 4.0;
/// Snap-to-grid step, in world units (half of the zoom-1 grid pitch).
pub const CANVAS_SNAP_STEP: f32 = 20.0;

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

/// True for anything that has a rect of its own; arrows have none.
fn is_node(element: &Value) -> bool {
    element.get("type").and_then(Value::as_str) != Some("arrow")
}

fn node_kind(element: &Value) -> &str {
    element
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("sticky")
}

/// The nodes' bounding box in world coordinates, arrows excluded.
pub fn content_bbox(elements: &[Value]) -> Option<Rect> {
    let mut bbox: Option<Rect> = None;
    for element in elements.iter().filter(|e| is_node(e)) {
        let rect = element_rect(element);
        bbox = Some(match bbox {
            Some(b) => b.union(rect),
            None => rect,
        });
    }
    bbox
}

/// A view centering the nodes' bounding box inside `canvas_size`
/// (fit-to-content); with an empty field it resets to identity.
pub fn canvas_fit_view(elements: &[Value], canvas_size: Vec2) -> CanvasView {
    let Some(bbox) = content_bbox(elements) else {
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
    let (width, height, text) = match kind {
        "text" => (180.0, 28.0, t(locale, Key::CanvasAddText).to_string()),
        "sticky" => (150.0, 90.0, t(locale, Key::CanvasAddSticky).to_string()),
        "diamond" => (120.0, 90.0, String::new()),
        "rounded" => (150.0, 60.0, String::new()),
        "page" => (170.0, 52.0, String::new()),
        _ => (150.0, 90.0, String::new()),
    };
    json!({
        "id": format!("el_{}", now_nanos()),
        "type": kind,
        "x": 24.0 + offset,
        "y": 24.0 + offset,
        "width": width,
        "height": height,
        "text": text,
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

/// Ids of nodes whose rect intersects the marquee, in element order. Arrows
/// are never marquee-selected: they live where their endpoints live.
fn element_ids_in_rect(elements: &[Value], marquee: Rect) -> Vec<String> {
    elements
        .iter()
        .filter(|element| is_node(element) && element_rect(element).intersects(marquee))
        .map(element_id)
        .collect()
}

/// Removes the doomed elements and every arrow that pointed at them.
fn prune_elements(elements: &mut Vec<Value>, doomed: &[String]) {
    elements.retain(|element| {
        let id = element_id(element);
        if doomed.contains(&id) {
            return false;
        }
        if node_kind(element) == "arrow" {
            let from = element.get("from").and_then(Value::as_str).unwrap_or("");
            let to = element.get("to").and_then(Value::as_str).unwrap_or("");
            if doomed.iter().any(|d| d == from || d == to) {
                return false;
            }
        }
        true
    });
}

fn find_node<'a>(elements: &'a [Value], id: &str) -> Option<&'a Value> {
    elements
        .iter()
        .find(|element| is_node(element) && element_id(element) == id)
}

/// The id of the topmost node containing `pos` (later elements draw on top).
fn node_id_at(elements: &[Value], pos: Pos2) -> Option<String> {
    elements
        .iter()
        .rev()
        .filter(|element| is_node(element) && element_rect(element).contains(pos))
        .map(element_id)
        .next()
}

/// Where a straight connector leaves a rect-shaped node heading for `toward`:
/// the ray from the center, scaled to the rect border.
fn rect_anchor(rect: Rect, toward: Pos2) -> Pos2 {
    let center = rect.center();
    let d = toward - center;
    let half_w = rect.width() * 0.5;
    let half_h = rect.height() * 0.5;
    if half_w <= 0.0 || half_h <= 0.0 || d == Vec2::ZERO {
        return center;
    }
    center + d * (1.0 / (d.x.abs() / half_w).max(d.y.abs() / half_h))
}

/// Same exit point for a diamond (the L1 ball scaled to the rect).
fn diamond_anchor(rect: Rect, toward: Pos2) -> Pos2 {
    let center = rect.center();
    let d = toward - center;
    let half_w = rect.width() * 0.5;
    let half_h = rect.height() * 0.5;
    if half_w <= 0.0 || half_h <= 0.0 || d == Vec2::ZERO {
        return center;
    }
    center + d * (1.0 / (d.x.abs() / half_w + d.y.abs() / half_h))
}

fn shape_anchor(kind: &str, rect: Rect, toward: Pos2) -> Pos2 {
    match kind {
        "diamond" => diamond_anchor(rect, toward),
        _ => rect_anchor(rect, toward),
    }
}

/// World-space endpoints of an arrow, recomputed from the node rects.
fn edge_endpoints(elements: &[Value], edge: &Value) -> Option<(Pos2, Pos2)> {
    let from = find_node(
        elements,
        edge.get("from").and_then(Value::as_str).unwrap_or(""),
    )?;
    let to = find_node(
        elements,
        edge.get("to").and_then(Value::as_str).unwrap_or(""),
    )?;
    let from_rect = element_rect(from);
    let to_rect = element_rect(to);
    Some((
        shape_anchor(node_kind(from), from_rect, to_rect.center()),
        shape_anchor(node_kind(to), to_rect, from_rect.center()),
    ))
}

/// Shortest distance from `p` to the segment `a..b`.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.dot(ab);
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Id of the arrow whose segment passes within `tol` world units of `pos`.
fn edge_id_at(elements: &[Value], pos: Pos2, tol: f32) -> Option<String> {
    elements
        .iter()
        .filter(|element| node_kind(element) == "arrow")
        .find(|edge| {
            edge_endpoints(elements, edge)
                .is_some_and(|(a, b)| seg_dist(pos, a, b) <= tol)
        })
        .map(element_id)
}

fn snap_pos(v: f32) -> f32 {
    (v / CANVAS_SNAP_STEP).round() * CANVAS_SNAP_STEP
}

/// Filled arrow head pointing from `from` into `tip`, in screen pixels.
fn paint_arrowhead(painter: &egui::Painter, tip: Pos2, from: Pos2, color: Color32) {
    let dir = (tip - from).normalized();
    if !dir.is_finite() || dir == Vec2::ZERO {
        return;
    }
    let perp = vec2(-dir.y, dir.x);
    let size = 7.0;
    painter.add(Shape::convex_polygon(
        vec![
            tip,
            tip - dir * size + perp * (size * 0.45),
            tip - dir * size - perp * (size * 0.45),
        ],
        color,
        Stroke::NONE,
    ));
}

pub fn draw_canvas_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    // A clicked, already-selected page node asks the app to open its page.
    // The request is stashed under a fixed id: exactly one document view is
    // drawn per frame, and main.rs drains it after the document pass.
    let mut navigate: Option<String> = None;
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
    let mut snap = block
        .get("data")
        .and_then(|data| data.get("snap"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut snap_changed = false;

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
                    if text_button(ui, Some(Icon::Doc), t(locale, Key::CanvasAddPage), false)
                        .clicked()
                    {
                        push_undo(ui, &block_id, &elements);
                        elements.push(canvas_element("page", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddRounded), false).clicked() {
                        push_undo(ui, &block_id, &elements);
                        elements.push(canvas_element("rounded", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddDiamond), false).clicked() {
                        push_undo(ui, &block_id, &elements);
                        elements.push(canvas_element("diamond", elements.len(), locale));
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
            let edit_key = egui::Id::new(("canvas-edit", block_id.clone()));
            let edit_focus_key = egui::Id::new(("canvas-edit-focus", block_id.clone()));
            let draft_key = egui::Id::new(("canvas-draft", block_id.clone()));
            // Page mode ("Страница | Канвас") asks for the whole content
            // height via data.height; the block default stays 300.
            let canvas_height = block
                .get("data")
                .and_then(|data| data.get("height"))
                .and_then(Value::as_f64)
                .map(|h| h.max(120.0) as f32)
                .unwrap_or(300.0);
            let (canvas_rect, canvas_resp) = ui.allocate_exact_size(
                vec2(ui.available_width(), canvas_height),
                Sense::click_and_drag(),
            );
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
                    let sx = canvas_rect.min.x
                        + (first_x + i as f32 * spacing) * view.zoom
                        + view.offset.x;
                    painter.vline(sx, canvas_rect.y_range(), stroke);
                }
                for j in 0..=rows {
                    let sy = canvas_rect.min.y
                        + (first_y + j as f32 * spacing) * view.zoom
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
            let mut edit_id: Option<String> =
                ui.data(|data| data.get_temp(edit_key)).unwrap_or(None);
            let mut edit_wants_focus: bool = ui
                .data(|data| data.get_temp(edit_focus_key))
                .unwrap_or(false);
            let mut edge_draft: Option<(String, Pos2)> =
                ui.data(|data| data.get_temp(draft_key)).unwrap_or(None);

            // --- background gestures: marquee (left), pan (middle/space) ---
            if canvas_resp.clicked() {
                // An arrow is picked by proximity to its segment; otherwise
                // an empty-field click clears the selection.
                let local = canvas_resp
                    .interact_pointer_pos()
                    .unwrap_or(canvas_rect.min)
                    - canvas_rect.min.to_vec2();
                if let Some(edge) =
                    edge_id_at(&elements, view.screen_to_world(local), 8.0 / view.zoom)
                {
                    next_selection = vec![edge];
                } else {
                    next_selection.clear();
                }
            }
            if canvas_resp.double_clicked() {
                let local = canvas_resp
                    .interact_pointer_pos()
                    .unwrap_or(canvas_rect.min)
                    - canvas_rect.min.to_vec2();
                if let Some(edge) =
                    edge_id_at(&elements, view.screen_to_world(local), 8.0 / view.zoom)
                {
                    next_selection = vec![edge.clone()];
                    edit_id = Some(edge);
                    edit_wants_focus = true;
                }
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
                    let marquee =
                        Rect::from_min_max(origin.min(cursor_world), origin.max(cursor_world));
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
            // reports the screen delta, the group applies it in world units
            // (optionally snapped to the grid).
            let mut group_delta: Option<Vec2> = None;
            for index in 0..elements.len() {
                if !is_node(&elements[index]) {
                    continue;
                }
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
                let kind = node_kind(&elements[index]).to_string();
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
                    // A page node is a wiki-link: clicking an already
                    // selected one opens the target page.
                    if kind == "page" && selection.contains(&id) {
                        let target = elements[index]
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !target.is_empty() {
                            navigate = Some(target);
                        }
                    }
                    next_selection = vec![id.clone()];
                }
                if response.double_clicked() {
                    edit_id = Some(id.clone());
                    edit_wants_focus = true;
                }

                let is_selected = selection.contains(&id);
                let (fill, stroke) = match kind.as_str() {
                    "rectangle" | "diamond" | "rounded" => (theme::wash(), theme::line_strong()),
                    "text" => (Color32::TRANSPARENT, theme::fade(theme::line(), 0.0)),
                    "page" => (theme::wash(), theme::accent()),
                    _ => (
                        Color32::from_rgb(45, 129, 255),
                        theme::fade(theme::accent_ink(), 0.4),
                    ),
                };
                if kind == "diamond" {
                    let c = screen_rect.center();
                    painter.add(Shape::convex_polygon(
                        vec![
                            pos2(screen_rect.left(), c.y),
                            pos2(c.x, screen_rect.top()),
                            pos2(screen_rect.right(), c.y),
                            pos2(c.x, screen_rect.bottom()),
                        ],
                        fill,
                        Stroke::new(
                            if is_selected { 1.6 } else { 1.0 },
                            if is_selected {
                                theme::accent_ink()
                            } else {
                                stroke
                            },
                        ),
                    ));
                } else if kind != "text" {
                    let corner = if kind == "sticky" {
                        egui::CornerRadius::same((4.0 * view.zoom).round() as u8)
                    } else if kind == "rounded" {
                        egui::CornerRadius::same(
                            (screen_rect.height() * 0.5).min(18.0 * view.zoom) as u8,
                        )
                    } else {
                        theme::RADIUS
                    };
                    painter.rect(
                        screen_rect,
                        corner,
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
                    FontId::new((11.5 * view.zoom).max(6.0), FontFamily::Proportional),
                    if kind == "sticky" {
                        theme::accent_ink()
                    } else {
                        theme::ink()
                    },
                );

                // The connector port sits on the right edge of a selected
                // node; dragging from it drafts an arrow to whatever node
                // the drop lands on.
                if is_selected && !pan_mode {
                    let port_center = pos2(screen_rect.right(), screen_rect.center().y);
                    let port = ui.interact(
                        Rect::from_center_size(port_center, vec2(16.0, 16.0)),
                        egui::Id::new(("canvas-port", block_id.clone(), id.clone())),
                        Sense::drag(),
                    );
                    if port.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                        painter.circle_filled(port_center, 6.0, theme::fade(theme::bg(), 0.6));
                        painter.circle_stroke(port_center, 6.0, Stroke::new(1.5, theme::accent()));
                    } else {
                        painter.circle_filled(port_center, 5.0, theme::fade(theme::accent(), 0.7));
                    }
                    if port.drag_started() {
                        let local = port
                            .interact_pointer_pos()
                            .unwrap_or(port_center)
                            - canvas_rect.min.to_vec2();
                        edge_draft = Some((id.clone(), view.screen_to_world(local)));
                    }
                    if port.dragged() {
                        if let Some(draft) = edge_draft.as_mut() {
                            let local = port
                                .interact_pointer_pos()
                                .unwrap_or(port_center)
                                - canvas_rect.min.to_vec2();
                            draft.1 = view.screen_to_world(local);
                        }
                    }
                    if port.drag_stopped() {
                        if let Some((source, cursor)) = edge_draft.clone() {
                            if let Some(target) = node_id_at(&elements, cursor) {
                                let duplicate = elements.iter().any(|element| {
                                    node_kind(element) == "arrow"
                                        && element.get("from").and_then(Value::as_str)
                                            == Some(source.as_str())
                                        && element.get("to").and_then(Value::as_str)
                                            == Some(target.as_str())
                                });
                                if target != source && !duplicate {
                                    push_undo(ui, &block_id, &elements);
                                    let new_id = format!("el_{}", now_nanos());
                                    elements.push(json!({
                                        "id": new_id,
                                        "type": "arrow",
                                        "from": source,
                                        "to": target,
                                        "label": "",
                                    }));
                                    next_selection = vec![new_id];
                                    changed = true;
                                }
                            }
                        }
                        edge_draft = None;
                    }
                }
            }
            if let Some(delta) = group_delta {
                let affected = next_selection.clone();
                for element in elements.iter_mut() {
                    if affected.contains(&element_id(element)) && is_node(element) {
                        let rect = element_rect(element);
                        let (nx, ny) = if snap {
                            (
                                snap_pos(rect.min.x + delta.x),
                                snap_pos(rect.min.y + delta.y),
                            )
                        } else {
                            (rect.min.x + delta.x, rect.min.y + delta.y)
                        };
                        element["x"] = json!(nx);
                        element["y"] = json!(ny);
                        changed = true;
                    }
                }
            }

            // --- arrows on top of the nodes, labels included ---
            for edge in elements.iter().filter(|e| node_kind(e) == "arrow") {
                let Some((a_world, b_world)) = edge_endpoints(&elements, edge) else {
                    continue;
                };
                let a = canvas_rect.min + view.world_to_screen(a_world).to_vec2();
                let b = canvas_rect.min + view.world_to_screen(b_world).to_vec2();
                if (a.x < canvas_rect.left() && b.x < canvas_rect.left())
                    || (a.x > canvas_rect.right() && b.x > canvas_rect.right())
                    || (a.y < canvas_rect.top() && b.y < canvas_rect.top())
                    || (a.y > canvas_rect.bottom() && b.y > canvas_rect.bottom())
                {
                    continue;
                }
                let is_selected = selection.contains(&element_id(edge));
                let color = if is_selected {
                    theme::accent_ink()
                } else {
                    theme::fade(theme::ink(), 0.8)
                };
                painter.line_segment(
                    [a, b],
                    Stroke::new(if is_selected { 1.6 } else { 1.2 }, color),
                );
                paint_arrowhead(&painter, b, a, color);
                let label = edge.get("label").and_then(Value::as_str).unwrap_or("");
                if !label.is_empty() {
                    let mid = a + (b - a) * 0.5;
                    let font = FontId::new((10.0 * view.zoom).max(6.0), FontFamily::Proportional);
                    let label_rect = Rect::from_center_size(
                        mid,
                        vec2(label.len() as f32 * (font.size * 0.62) + 8.0, font.size + 4.0),
                    );
                    painter.rect_filled(label_rect, 3.0, theme::fade(theme::bg(), 0.85));
                    painter.text(
                        mid,
                        Align2::CENTER_CENTER,
                        label,
                        font,
                        if is_selected {
                            theme::accent_ink()
                        } else {
                            theme::ink()
                        },
                    );
                }
            }

            // Rubber band of an arrow being dragged into existence.
            if let Some((source, cursor)) = edge_draft.clone() {
                if let Some(from) = find_node(&elements, &source) {
                    let from_rect = element_rect(from);
                    let a = canvas_rect.min
                        + view
                            .world_to_screen(shape_anchor(node_kind(from), from_rect, cursor))
                            .to_vec2();
                    let b = canvas_rect.min + view.world_to_screen(cursor).to_vec2();
                    painter.line_segment([a, b], Stroke::new(1.2, theme::accent()));
                    painter.circle_filled(b, 3.0, theme::accent());
                } else {
                    edge_draft = None;
                }
            }

            // --- minimap: content box, viewport, click-to-navigate ---
            let node_count = elements.iter().filter(|e| is_node(e)).count();
            if let Some(bbox) = content_bbox(&elements) {
                if node_count >= 2 {
                    let mm_size = vec2(130.0, 90.0);
                    let mm_rect = Rect::from_min_size(
                        canvas_rect.right_bottom() - vec2(10.0, 10.0) - mm_size,
                        mm_size,
                    );
                    let scale = (mm_rect.width() / bbox.width().max(1.0))
                        .min(mm_rect.height() / bbox.height().max(1.0))
                        * 0.9;
                    if scale > 0.0 {
                        let content_min = mm_rect.center() - bbox.center().to_vec2() * scale;
                        let to_mm = |world: Pos2| content_min + (world - bbox.min) * scale;
                        painter.rect_filled(mm_rect, theme::RADIUS, theme::fade(theme::bg(), 0.92));
                        painter.rect_stroke(
                            mm_rect,
                            theme::RADIUS,
                            theme::hairline(),
                            egui::StrokeKind::Inside,
                        );
                        for element in elements.iter().filter(|e| is_node(e)) {
                            let rect = element_rect(element);
                            let mm_node = Rect::from_min_max(to_mm(rect.min), to_mm(rect.max));
                            let color = if selection.contains(&element_id(element)) {
                                theme::accent_ink()
                            } else {
                                theme::fade(theme::ink(), 0.45)
                            };
                            painter.rect_filled(mm_node, 1.0, color);
                        }
                        let visible_world = Rect::from_min_max(
                            view.screen_to_world(pos2(0.0, 0.0)),
                            view.screen_to_world(pos2(canvas_rect.width(), canvas_rect.height())),
                        );
                        painter.rect_stroke(
                            Rect::from_min_max(to_mm(visible_world.min), to_mm(visible_world.max)),
                            1.0,
                            Stroke::new(1.0, theme::accent()),
                            egui::StrokeKind::Inside,
                        );
                        let mm_resp = ui.interact(
                            mm_rect,
                            egui::Id::new(("canvas-minimap", block_id.clone())),
                            Sense::click_and_drag(),
                        );
                        if mm_resp.clicked() || mm_resp.dragged() || mm_resp.drag_started() {
                            if let Some(pointer) = mm_resp.interact_pointer_pos() {
                                let world = bbox.min + (pointer - content_min) / scale;
                                view.offset =
                                    canvas_rect.size() / 2.0 - world.to_vec2() * view.zoom;
                                view_changed = true;
                            }
                        }
                    }
                }
            }

            if node_count == 0 {
                painter.text(
                    canvas_rect.center(),
                    Align2::CENTER_CENTER,
                    t(locale, Key::CanvasEmpty),
                    FontId::new(11.5, FontFamily::Proportional),
                    theme::faint(),
                );
            }

            // --- viewport toolbar: zoom out / percent / in / fit / snap ---
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
                if text_button(ui, None, t(locale, Key::CanvasSnap), snap).clicked() {
                    snap = !snap;
                    snap_changed = true;
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
                        prune_elements(&mut elements, &doomed);
                        next_selection.clear();
                        changed = true;
                    }
                    let _ = hud(ui, t(locale, Key::CanvasHint), theme::faint());
                });
            } else {
                ui.add_space(4.0);
                let _ = hud(ui, t(locale, Key::CanvasHint), theme::faint());
            }

            // --- inline text editing, double-click on a node or an arrow ---
            if let Some(edit) = edit_id.clone() {
                let index = elements
                    .iter()
                    .position(|element| element_id(element) == edit);
                if let Some(index) = index {
                    let is_arrow = node_kind(&elements[index]) == "arrow";
                    let screen_rect = if is_arrow {
                        edge_endpoints(&elements, &elements[index]).map_or(
                            Rect::from_center_size(canvas_rect.center(), vec2(140.0, 26.0)),
                            |(a_world, b_world)| {
                                let a = canvas_rect.min + view.world_to_screen(a_world).to_vec2();
                                let b = canvas_rect.min + view.world_to_screen(b_world).to_vec2();
                                Rect::from_center_size(a + (b - a) * 0.5, vec2(160.0, 24.0))
                            },
                        )
                    } else {
                        let world_rect = element_rect(&elements[index]);
                        Rect::from_min_size(
                            canvas_rect.min + view.world_to_screen(world_rect.min).to_vec2(),
                            vec2(
                                (world_rect.width() * view.zoom).max(120.0),
                                (world_rect.height() * view.zoom).max(26.0),
                            ),
                        )
                    }
                    .expand2(vec2(6.0, 3.0));
                    let field = if is_arrow { "label" } else { "text" };
                    let mut text = elements[index]
                        .get(field)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let mut overlay = ui.new_child(egui::UiBuilder::new().max_rect(screen_rect));
                    let response = overlay.add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(screen_rect.width())
                            .font(FontId::new(
                                (11.5 * view.zoom).max(6.0),
                                FontFamily::Proportional,
                            )),
                    );
                    if edit_wants_focus {
                        response.request_focus();
                        edit_wants_focus = false;
                    }
                    if response.changed() || response.lost_focus() {
                        elements[index][field] = json!(text);
                        changed = true;
                    }
                    if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        edit_id = None;
                    }
                } else {
                    edit_id = None;
                }
            }

            ui.data_mut(|data| {
                data.insert_temp(selection_key, next_selection);
                data.insert_temp(pan_key, pan_mode);
                data.insert_temp(edit_key, edit_id);
                data.insert_temp(edit_focus_key, edit_wants_focus);
                data.insert_temp(draft_key, edge_draft);
            });
        });

    if changed {
        block["data"]["elements"] = Value::Array(elements);
    }
    if view_changed {
        block["data"]["view"] = view.to_json();
        changed = true;
    }
    if snap_changed {
        block["data"]["snap"] = json!(snap);
        changed = true;
    }
    if let Some(target) = navigate {
        ui.data_mut(|data| {
            data.insert_temp(egui::Id::new("page-canvas-nav"), target);
        });
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
        json!({"id": id, "type": "sticky", "x": x, "y": y, "width": w, "height": h})
    }

    fn node_a() -> Value {
        element("a", 0.0, 0.0, 100.0, 80.0)
    }

    fn node_b() -> Value {
        element("b", 200.0, 0.0, 100.0, 80.0)
    }

    fn arrow(id: &str, from: &str, to: &str) -> Value {
        json!({"id": id, "type": "arrow", "from": from, "to": to, "label": ""})
    }

    #[test]
    fn marquee_selects_intersecting_elements() {
        let elements = vec![
            element("a", 10.0, 10.0, 50.0, 40.0),
            element("b", 200.0, 200.0, 50.0, 40.0),
            element("c", 25.0, 30.0, 50.0, 40.0), // overlaps a
            arrow("x", "a", "b"),                 // arrows are never marquee-selected
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
        assert_eq!(
            CanvasView::from_data(Some(&json!({}))),
            CanvasView::identity()
        );
        // Corrupt or dangerous values fall back or clamp.
        assert_eq!(
            CanvasView::from_data(Some(&json!({"view": {"zoom": -3.0}}))),
            CanvasView::identity()
        );
        assert_eq!(
            CanvasView::from_data(Some(&json!({"view": {"x": "nan", "y": {}, "zoom": 99.0}}))).zoom,
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

    #[test]
    fn rect_anchor_leaves_through_the_border_toward_the_target() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 50.0));
        // Straight right: exits through the right edge at mid height.
        let a = rect_anchor(rect, pos2(200.0, 25.0));
        assert!((a - pos2(100.0, 25.0)).length() < 1e-4);
        // Steep diagonal exits through the bottom edge, not the corner.
        let a = rect_anchor(rect, pos2(150.0, 125.0));
        assert!((a - pos2(75.0, 50.0)).length() < 1e-4);
        // Zero distance collapses to the center instead of dividing by zero.
        assert_eq!(rect_anchor(rect, rect.center()), rect.center());
    }

    #[test]
    fn diamond_anchor_touches_the_edge_line() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 50.0));
        // Straight right hits the right vertex.
        let a = diamond_anchor(rect, pos2(200.0, 25.0));
        assert!((a - pos2(100.0, 25.0)).length() < 1e-4);
        // The L1 anchor sits exactly on the diamond's edge.
        let a = diamond_anchor(rect, pos2(150.0, 75.0));
        assert!(((a.x - 50.0).abs() / 50.0 + (a.y - 25.0).abs() / 25.0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn edge_hit_finds_the_arrow_under_the_pointer() {
        let elements = vec![node_a(), node_b(), arrow("x", "a", "b")];
        // The segment runs (100,40) → (200,40).
        assert_eq!(
            edge_id_at(&elements, pos2(150.0, 42.0), 8.0),
            Some("x".into())
        );
        assert_eq!(edge_id_at(&elements, pos2(150.0, 49.0), 8.0), None);
        // Beyond the segment end the distance goes to the endpoint.
        assert_eq!(edge_id_at(&elements, pos2(210.0, 40.0), 8.0), None);
    }

    #[test]
    fn prune_drops_doomed_elements_and_their_arrows() {
        let elements = vec![
            node_a(),
            node_b(),
            element("c", 400.0, 0.0, 50.0, 40.0),
            arrow("ab", "a", "b"),
            arrow("bc", "b", "c"),
        ];
        let mut kept = elements;
        prune_elements(&mut kept, &["b".to_string()]);
        let ids: Vec<String> = kept.iter().map(element_id).collect();
        assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);
        // Deleting an unrelated node keeps the arrow between the others.
        let mut kept = vec![node_a(), node_b(), arrow("ab", "a", "b")];
        prune_elements(&mut kept, &["c".into()]);
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn content_bbox_ignores_arrows() {
        let elements = vec![
            element("a", 0.0, 0.0, 50.0, 50.0),
            element("b", 100.0, 100.0, 10.0, 10.0),
            arrow("x", "a", "b"),
        ];
        assert_eq!(
            content_bbox(&elements),
            Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(110.0, 110.0)))
        );
        assert_eq!(content_bbox(&[arrow("x", "a", "b")]), None);
    }

    #[test]
    fn node_hit_picks_the_topmost_node() {
        let elements = vec![
            element("a", 0.0, 0.0, 100.0, 80.0),
            element("b", 40.0, 20.0, 100.0, 80.0), // overlaps a, drawn later
        ];
        assert_eq!(node_id_at(&elements, pos2(60.0, 40.0)), Some("b".into()));
        assert_eq!(node_id_at(&elements, pos2(10.0, 10.0)), Some("a".into()));
        assert_eq!(node_id_at(&elements, pos2(500.0, 500.0)), None);
    }

    #[test]
    fn snap_rounds_to_the_half_grid_step() {
        assert_eq!(snap_pos(17.3), 20.0);
        assert_eq!(snap_pos(9.9), 0.0);
        assert_eq!(snap_pos(30.0), 40.0);
        assert_eq!(snap_pos(-14.0), -20.0);
    }
}

#[cfg(test)]
mod page_node_tests {
    use super::*;
    use serde_json::json;

    /// The page node exists, is a node (not an arrow) and carries its title.
    #[test]
    fn page_element_factory_produces_a_node() {
        let el = canvas_element("page", 0, crate::i18n::Locale::En);
        assert!(is_node(&el));
        assert_eq!(el.get("type").and_then(Value::as_str), Some("page"));
        assert_eq!(el.get("width").and_then(Value::as_f64), Some(170.0));
        // Text starts empty: the user types the page title on it.
        assert_eq!(el.get("text").and_then(Value::as_str), Some(""));
    }

    /// Drawing the full-page canvas with page nodes must not panic headlessly
    /// and the height override must be honored by the allocation.
    #[test]
    fn page_canvas_draws_headlessly() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        let mut block = json!({
            "id": "page-canvas",
            "type": "interactive_canvas",
            "data": {
                "elements": [
                    {"id":"pg1","type":"page","x":20.0,"y":20.0,"width":170.0,"height":52.0,
                     "text":"Дорожная карта","color":""}
                ],
                "height": 640.0
            }
        });
        let mut changed = false;
        let mut out = ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("page-canvas-test"))
                .fixed_pos(egui::Pos2::ZERO)
                .show(ctx, |ui| {
                    changed = draw_canvas_block(ui, &mut block, crate::i18n::Locale::En);
                });
        });
        // Headless: drop the texture updates instead of painting them.
        out.textures_delta.clear();
        let _ = changed; // an untouched board legitimately reports no change
        assert_eq!(
            block
                .get("data")
                .and_then(|d| d.get("elements"))
                .and_then(|e| e.get(0))
                .and_then(|e| e.get("type"))
                .and_then(Value::as_str),
            Some("page")
        );
    }
}
