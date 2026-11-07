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

use super::{image::pick_image, image::texture_for, BlockOp};
use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::Attachment;
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
        let zoom = view.get("zoom").and_then(Value::as_f64).unwrap_or(1.0) as f32;
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

/// The viewport a reopened page starts from. The stored view wins, unless it
/// is still the identity and the content sits entirely off the first screen —
/// then a fit-to-content jump instead of an empty grid that reads as "the
/// drawing is gone". Any deliberate pan or zoom makes the view non-identity
/// and the jump never fires again.
pub fn canvas_open_view(elements: &[Value], stored: CanvasView, canvas_size: Vec2) -> CanvasView {
    if stored != CanvasView::identity() {
        return stored;
    }
    let Some(bbox) = content_bbox(elements) else {
        return stored;
    };
    let visible = Rect::from_min_max(
        stored.screen_to_world(pos2(0.0, 0.0)),
        stored.screen_to_world(pos2(canvas_size.x, canvas_size.y)),
    );
    if visible.intersects(bbox) {
        stored
    } else {
        canvas_fit_view(elements, canvas_size)
    }
}

/// Builds an element whose top-left sits at the world position `at`.
pub fn canvas_element_at(kind: &str, at: Pos2, locale: Locale) -> Value {
    let (width, height, text) = match kind {
        "text" => (180.0, 28.0, t(locale, Key::CanvasAddText).to_string()),
        "sticky" => (150.0, 90.0, t(locale, Key::CanvasAddSticky).to_string()),
        "diamond" => (120.0, 90.0, String::new()),
        "rounded" => (150.0, 60.0, String::new()),
        "page" => (170.0, 52.0, String::new()),
        "image" => (180.0, 120.0, String::new()),
        _ => (150.0, 90.0, String::new()),
    };
    json!({
        "id": format!("el_{}", now_nanos()),
        "type": kind,
        "x": at.x as f64,
        "y": at.y as f64,
        "width": width,
        "height": height,
        "text": text,
        "color": "",
    })
}

/// Where a toolbar-added node lands: the center of the visible field, so the
/// button's result is always on screen no matter how far the board was panned
/// or zoomed (a fixed spawn point near the world origin made additions vanish
/// whenever the viewport had moved away). A small cascade keeps consecutive
/// adds from stacking exactly on top of each other.
pub fn spawn_position(view: &CanvasView, canvas_rect: Option<Rect>, count: usize) -> Pos2 {
    let rect = canvas_rect.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, vec2(640.0, 360.0)));
    let center = view.screen_to_world(rect.center());
    let cascade = (count % 8) as f32 * 16.0;
    pos2(center.x + cascade, center.y + cascade)
}

/// The largest rect keeping `natural`'s aspect ratio that fits inside
/// `bounds`, centered. Canvas image nodes letterbox into their user-sized
/// node rect instead of stretching.
pub fn fit_inside(natural: Vec2, bounds: Rect) -> Rect {
    if !natural.is_finite() || natural.x <= 0.0 || natural.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width() / natural.x).min(bounds.height() / natural.y);
    Rect::from_center_size(bounds.center(), natural * scale)
}

/// Resolves an image element's `cid` to a picture attachment.
fn image_attachment<'a>(element: &Value, attachments: &'a [Attachment]) -> Option<&'a Attachment> {
    let cid = element
        .get("cid")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches("cid:");
    attachments
        .iter()
        .find(|attachment| attachment.cid == cid && attachment.mime_type.starts_with("image/"))
}

/// Bounding box of a freehand stroke, from its absolute world points.
fn ink_bbox(points: &[Pos2]) -> Rect {
    let Some(first) = points.first() else {
        return Rect::NOTHING;
    };
    let (mut min, mut max) = (*first, *first);
    for p in points.iter().skip(1) {
        min = min.min(*p);
        max = max.max(*p);
    }
    Rect::from_min_max(min, max)
}

fn round10(v: f32) -> f64 {
    ((v * 10.0).round() / 10.0) as f64
}

/// Builds an ink element: the bbox goes to x/y/width/height (so drag, marquee
/// and fit treat a stroke like any other element), the points are stored
/// relative to the bbox origin and rounded to 0.1 to keep the JSON compact.
fn ink_element(id: String, points_abs: &[Pos2]) -> Value {
    let bbox = ink_bbox(points_abs);
    let points: Vec<Value> = points_abs
        .iter()
        .map(|p| json!([round10(p.x - bbox.left()), round10(p.y - bbox.top()),]))
        .collect();
    json!({
        "id": id,
        "type": "ink",
        "x": round10(bbox.left()),
        "y": round10(bbox.top()),
        "width": round10(bbox.width().max(1.0)),
        "height": round10(bbox.height().max(1.0)),
        "points": points,
        "color": "",
        "text": "",
    })
}

/// Absolute world points of an ink element, in stroke order.
fn ink_points(element: &Value) -> Vec<Pos2> {
    let rect = element_rect(element);
    element
        .get("points")
        .and_then(Value::as_array)
        .map(|points| {
            points
                .iter()
                .filter_map(|p| {
                    let pair = p.as_array()?;
                    Some(
                        rect.min
                            + vec2(
                                pair.first()?.as_f64()? as f32,
                                pair.get(1)?.as_f64()? as f32,
                            ),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Undo-snapshot, spawn at the visible center, append, flag the change.
fn add_element(
    ui: &mut Ui,
    view: &CanvasView,
    rect_key: egui::Id,
    kind: &str,
    elements: &mut Vec<Value>,
    block_id: &str,
    locale: Locale,
) -> bool {
    push_undo(ui, block_id, elements);
    let rect = ui.data(|d| d.get_temp(rect_key));
    let at = spawn_position(view, rect, elements.len());
    elements.push(canvas_element_at(kind, at, locale));
    true
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
            edge_endpoints(elements, edge).is_some_and(|(a, b)| seg_dist(pos, a, b) <= tol)
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

pub fn draw_canvas_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &[Attachment],
    locale: Locale,
    op: &mut Option<BlockOp>,
) -> bool {
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
    let mut pen_on = block
        .get("data")
        .and_then(|data| data.get("pen"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut pen_changed = false;
    let rect_key = egui::Id::new(("canvas-rect", block_id.clone()));

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
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "text",
                            &mut elements,
                            &block_id,
                            locale,
                        );
                    }
                    if text_button(ui, Some(Icon::Doc), t(locale, Key::CanvasAddPage), false)
                        .clicked()
                    {
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "page",
                            &mut elements,
                            &block_id,
                            locale,
                        );
                    }
                    let image_button =
                        text_button(ui, Some(Icon::Image), t(locale, Key::CanvasAddImage), false);
                    let pixels: Vec<&Attachment> = attachments
                        .iter()
                        .filter(|attachment| attachment.mime_type.starts_with("image/"))
                        .collect();
                    let mut picked_cid: Option<String> = None;
                    let mut pick_from_file = false;
                    if !pixels.is_empty() {
                        // Images already in the capsule: pick one instead of
                        // stacking a second copy of the same file.
                        egui::Popup::menu(&image_button)
                            .frame(crate::popover_frame())
                            .align(egui::RectAlign::BOTTOM_END)
                            .show(|ui| {
                                ui.set_min_width(200.0);
                                if crate::menu_row(
                                    ui,
                                    Some(Icon::Plus),
                                    t(locale, Key::ImageChoose),
                                    None,
                                    false,
                                    true,
                                    false,
                                )
                                .clicked()
                                {
                                    pick_from_file = true;
                                }
                                for attachment in &pixels {
                                    if crate::menu_row(
                                        ui,
                                        Some(Icon::Image),
                                        &crate::util::truncate_chars(&attachment.name, 34),
                                        None,
                                        false,
                                        true,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        picked_cid = Some(attachment.cid.clone());
                                    }
                                }
                            });
                    } else if image_button.clicked() {
                        pick_from_file = true;
                    }
                    if pick_from_file {
                        if let Some((name, cid, mime, b64, size, ..)) = pick_image(locale) {
                            changed |= add_element(
                                ui,
                                &view,
                                rect_key,
                                "image",
                                &mut elements,
                                &block_id,
                                locale,
                            );
                            if let Some(element) = elements.last_mut() {
                                element["cid"] = json!(format!("cid:{cid}"));
                            }
                            *op = Some(BlockOp::AttachImage {
                                name,
                                cid,
                                mime,
                                b64,
                                size,
                            });
                        }
                    }
                    if let Some(cid) = picked_cid {
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "image",
                            &mut elements,
                            &block_id,
                            locale,
                        );
                        if let Some(element) = elements.last_mut() {
                            element["cid"] = json!(format!("cid:{cid}"));
                        }
                    }
                    // Freehand: while the pen is on, a drag on the empty
                    // field draws a stroke instead of pulling a marquee.
                    if text_button(ui, Some(Icon::Pen), t(locale, Key::CanvasDraw), pen_on)
                        .clicked()
                    {
                        pen_on = !pen_on;
                        pen_changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddRounded), false).clicked() {
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "rounded",
                            &mut elements,
                            &block_id,
                            locale,
                        );
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddDiamond), false).clicked() {
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "diamond",
                            &mut elements,
                            &block_id,
                            locale,
                        );
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddRect), false).clicked() {
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "rectangle",
                            &mut elements,
                            &block_id,
                            locale,
                        );
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddSticky), false).clicked() {
                        changed |= add_element(
                            ui,
                            &view,
                            rect_key,
                            "sticky",
                            &mut elements,
                            &block_id,
                            locale,
                        );
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
            let ink_key = egui::Id::new(("canvas-ink", block_id.clone()));
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
            // Remembered for the toolbar above: next frame's add-buttons spawn
            // the new node at this rect's center (see `spawn_position`).
            ui.data_mut(|d| d.insert_temp(rect_key, canvas_rect));
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

                // Keyboard navigation under the same guard: +/− zoom about
                // the center, Home or Ctrl+0 fits, arrows scroll. The keys
                // act only while the pointer rests on the field, so a node
                // editor keeps its typing untouched.
                let center = canvas_rect.size() / 2.0;
                let (zoom_in, zoom_out, fit, pan) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
                        i.key_pressed(egui::Key::Minus),
                        i.key_pressed(egui::Key::Home)
                            || (i.modifiers.ctrl && i.key_pressed(egui::Key::Num0)),
                        vec2(
                            (i.key_pressed(egui::Key::ArrowRight) as i32
                                - i.key_pressed(egui::Key::ArrowLeft) as i32)
                                as f32,
                            (i.key_pressed(egui::Key::ArrowDown) as i32
                                - i.key_pressed(egui::Key::ArrowUp) as i32)
                                as f32,
                        ) * 60.0,
                    )
                });
                if zoom_in {
                    view.zoom_at(center.to_pos2(), 1.25);
                    view_changed = true;
                }
                if zoom_out {
                    view.zoom_at(center.to_pos2(), 1.0 / 1.25);
                    view_changed = true;
                }
                if fit {
                    view = canvas_fit_view(&elements, canvas_rect.size());
                    view_changed = true;
                }
                if pan != Vec2::ZERO {
                    view.pan_by_scroll(pan);
                    view_changed = true;
                }
            }

            // A reopened page shows its saved (or identity) viewport; when
            // the content lives entirely off that first screen, jump to it
            // once instead of staring at an empty grid.
            let open_view = canvas_open_view(&elements, view, canvas_rect.size());
            if open_view != view {
                view = open_view;
                view_changed = true;
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
            if canvas_resp.drag_started_by(PointerButton::Primary)
                && !pan_mode
                && !space_held
                && !pen_on
            {
                let local = canvas_resp
                    .interact_pointer_pos()
                    .unwrap_or(canvas_rect.min)
                    - canvas_rect.min.to_vec2();
                ui.data_mut(|data| {
                    data.insert_temp(marquee_key, Some(view.screen_to_world(local)))
                });
            }
            let mut marquee_now: Option<Rect> = None;
            let mut marquee_done: Option<Rect> = None;
            if canvas_resp.dragged_by(PointerButton::Primary) && !pan_mode && !pen_on {
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
            } else if marquee_origin.is_some() && !pan_mode && !pen_on && canvas_resp.drag_stopped()
            {
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
            // --- freehand: collect the stroke while the pen drag lasts ---
            if pen_on && canvas_resp.drag_started_by(PointerButton::Primary) && !space_held {
                ui.data_mut(|data| data.insert_temp(ink_key, Vec::<[f32; 2]>::new()));
            }
            if pen_on && canvas_resp.dragged_by(PointerButton::Primary) {
                if let Some(local) = canvas_resp.interact_pointer_pos() {
                    let world = view.screen_to_world(local - canvas_rect.min.to_vec2());
                    let mut stroke = ui
                        .data(|data| data.get_temp::<Vec<[f32; 2]>>(ink_key))
                        .unwrap_or_default();
                    // Decimate: a point only when the cursor moved far enough
                    // on screen, so a slow wiggle does not bloat the JSON.
                    let far_enough = match stroke.last() {
                        Some(last) => (world - pos2(last[0], last[1])).length() * view.zoom > 1.5,
                        None => true,
                    };
                    if far_enough {
                        stroke.push([world.x, world.y]);
                        ui.data_mut(|data| data.insert_temp(ink_key, stroke));
                    }
                }
            }
            if pen_on && canvas_resp.drag_stopped() {
                let stroke: Vec<[f32; 2]> =
                    ui.data(|data| data.get_temp(ink_key)).unwrap_or_default();
                ui.data_mut(|data| data.remove_temp::<Vec<[f32; 2]>>(ink_key));
                if !stroke.is_empty() {
                    push_undo(ui, &block_id, &elements);
                    let points: Vec<Pos2> = stroke.iter().map(|p| pos2(p[0], p[1])).collect();
                    elements.push(ink_element(format!("el_{}", now_nanos()), &points));
                    next_selection.clear();
                    changed = true;
                }
            }
            if pen_on {
                // Live preview of the stroke being drawn.
                let draft: Vec<[f32; 2]> =
                    ui.data(|data| data.get_temp(ink_key)).unwrap_or_default();
                let screen: Vec<Pos2> = draft
                    .iter()
                    .map(|p| canvas_rect.min + view.world_to_screen(pos2(p[0], p[1])).to_vec2())
                    .collect();
                let width = (2.0 * view.zoom).max(1.0);
                if screen.len() > 1 {
                    painter.add(Shape::line(screen, Stroke::new(width, theme::ink())));
                } else if let Some(p) = screen.first() {
                    painter.circle_filled(*p, width, theme::ink());
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
                    // A picture opens the viewer; other nodes edit their text.
                    if kind == "image" {
                        if let Some(attachment) = image_attachment(&elements[index], attachments) {
                            *op = Some(BlockOp::OpenViewer(attachment.cid.clone()));
                        }
                    } else if kind != "ink" {
                        edit_id = Some(id.clone());
                        edit_wants_focus = true;
                    }
                }

                let is_selected = selection.contains(&id);
                let (fill, stroke) = match kind.as_str() {
                    "rectangle" | "diamond" | "rounded" => (theme::wash(), theme::line_strong()),
                    "text" => (Color32::TRANSPARENT, theme::fade(theme::line(), 0.0)),
                    "page" => (theme::wash(), theme::accent()),
                    "image" => (Color32::TRANSPARENT, theme::fade(theme::line(), 0.0)),
                    _ => (
                        Color32::from_rgb(45, 129, 255),
                        theme::fade(theme::accent_ink(), 0.4),
                    ),
                };
                if kind == "image" {
                    // Letterbox the picture into the node rect; an unresolved
                    // cid gets a quiet placeholder instead of a blank box.
                    let texture = image_attachment(&elements[index], attachments)
                        .and_then(|attachment| texture_for(ui, &attachment.cid, attachment));
                    match texture {
                        Some(handle) => {
                            let fit = fit_inside(handle.size_vec2(), screen_rect);
                            painter.image(
                                handle.id(),
                                fit,
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                        None => {
                            painter.rect(
                                screen_rect,
                                theme::RADIUS,
                                theme::wash(),
                                Stroke::new(1.0, theme::hairline().color),
                                egui::StrokeKind::Inside,
                            );
                            let missing = elements[index]
                                .get("cid")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim_start_matches("cid:")
                                .is_empty();
                            let label = t(
                                locale,
                                if missing {
                                    Key::ImageNoFile
                                } else {
                                    Key::ImageDecodeFailed
                                },
                            );
                            icons::paint(
                                &painter,
                                Rect::from_center_size(
                                    screen_rect.center() - vec2(0.0, 9.0 * view.zoom),
                                    vec2(16.0, 16.0),
                                ),
                                Icon::Image,
                                theme::muted(),
                            );
                            painter.text(
                                screen_rect.center() + vec2(0.0, 12.0 * view.zoom),
                                Align2::CENTER_CENTER,
                                label,
                                FontId::new((10.0 * view.zoom).max(6.0), FontFamily::Proportional),
                                theme::muted(),
                            );
                        }
                    }
                    if is_selected {
                        painter.rect_stroke(
                            screen_rect,
                            theme::RADIUS,
                            Stroke::new(1.6, theme::accent_ink()),
                            egui::StrokeKind::Inside,
                        );
                    }
                } else if kind == "ink" {
                    // Freehand stroke: a polyline through the element's
                    // points; a single-click stroke renders as a dot.
                    let mut screen: Vec<Pos2> = ink_points(&elements[index])
                        .iter()
                        .map(|p| canvas_rect.min + view.world_to_screen(*p).to_vec2())
                        .collect();
                    let width = (2.0 * view.zoom).max(1.0);
                    let color = if is_selected {
                        theme::accent_ink()
                    } else {
                        theme::ink()
                    };
                    if screen.len() > 1 {
                        painter.add(Shape::line(screen, Stroke::new(width, color)));
                    } else if let Some(p) = screen.pop() {
                        painter.circle_filled(p, width, color);
                    }
                } else if kind == "diamond" {
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
                            (screen_rect.height() * 0.5).min(18.0 * view.zoom) as u8
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
                if is_selected && !pan_mode && kind != "image" && kind != "ink" {
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
                        let local = port.interact_pointer_pos().unwrap_or(port_center)
                            - canvas_rect.min.to_vec2();
                        edge_draft = Some((id.clone(), view.screen_to_world(local)));
                    }
                    if port.dragged() {
                        if let Some(draft) = edge_draft.as_mut() {
                            let local = port.interact_pointer_pos().unwrap_or(port_center)
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
                        vec2(
                            label.len() as f32 * (font.size * 0.62) + 8.0,
                            font.size + 4.0,
                        ),
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
                if node_count >= 1 {
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
                        // Content and the viewport rect may reach past the
                        // frame while panned away; clipped so the map stays
                        // a neat window, not a spill.
                        let mm_painter = painter.with_clip_rect(mm_rect.expand(1.0));
                        for element in elements.iter().filter(|e| is_node(e)) {
                            let rect = element_rect(element);
                            let mm_node = Rect::from_min_max(to_mm(rect.min), to_mm(rect.max));
                            let color = if selection.contains(&element_id(element)) {
                                theme::accent_ink()
                            } else {
                                theme::fade(theme::ink(), 0.45)
                            };
                            mm_painter.rect_filled(mm_node, 1.0, color);
                        }
                        let visible_world = Rect::from_min_max(
                            view.screen_to_world(pos2(0.0, 0.0)),
                            view.screen_to_world(pos2(canvas_rect.width(), canvas_rect.height())),
                        );
                        mm_painter.rect_stroke(
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
    if pen_changed {
        block["data"]["pen"] = json!(pen_on);
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

    /// A reopened canvas with content off the first screen jumps to it once;
    /// content on screen and a deliberately moved view stay untouched.
    #[test]
    fn open_view_jumps_to_offscreen_content_once() {
        let far = vec![json!({
            "id": "n1", "type": "sticky", "x": 5000.0, "y": 5000.0,
            "width": 150.0, "height": 90.0, "label": "", "color": ""
        })];
        let opened = canvas_open_view(&far, CanvasView::identity(), vec2(800.0, 600.0));
        assert!(
            opened != CanvasView::identity(),
            "must fit off-screen content"
        );
        assert_eq!(
            canvas_open_view(&far, opened, vec2(800.0, 600.0)),
            opened,
            "a moved view is never re-fitted"
        );
        let near = vec![json!({
            "id": "n2", "type": "sticky", "x": 100.0, "y": 100.0,
            "width": 150.0, "height": 90.0, "label": "", "color": ""
        })];
        assert_eq!(
            canvas_open_view(&near, CanvasView::identity(), vec2(800.0, 600.0)),
            CanvasView::identity(),
            "on-screen content keeps the identity view"
        );
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
        let el = canvas_element_at("page", egui::pos2(24.0, 24.0), crate::i18n::Locale::En);
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
                    let mut op: Option<BlockOp> = None;
                    changed =
                        draw_canvas_block(ui, &mut block, &[], crate::i18n::Locale::En, &mut op);
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

#[cfg(test)]
mod spawn_tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
    }

    /// Regression: toolbar adds used to land at a fixed spot near the world
    /// origin, so after panning (or on a board whose saved view sits far
    /// away) the new node appeared off-screen — «добавил, а его не видно».
    /// Whatever the view, the spawn rect must cover the viewport center.
    #[test]
    fn spawn_lands_in_the_visible_field() {
        let views = [
            CanvasView::identity(),
            CanvasView {
                offset: vec2(-5000.0, -3000.0),
                zoom: 1.0,
            },
            CanvasView {
                offset: vec2(1200.0, 80.0),
                zoom: 0.5,
            },
            CanvasView {
                offset: vec2(-200.0, -200.0),
                zoom: 2.0,
            },
        ];
        for view in views {
            let viewport_world = Rect::from_min_max(
                view.screen_to_world(rect().min),
                view.screen_to_world(rect().max),
            );
            for count in 0..12 {
                let at = spawn_position(&view, Some(rect()), count);
                let el = canvas_element_at("sticky", at, crate::i18n::Locale::En);
                assert!(
                    viewport_world.intersects(element_rect(&el)),
                    "view {view:?} count {count}: spawn {at:?} is off-screen"
                );
            }
        }
    }

    /// Consecutive adds cascade instead of stacking exactly, but every
    /// cascade step stays within one node size of the center.
    #[test]
    fn consecutive_spawns_cascade_but_stay_close() {
        let view = CanvasView::identity();
        let mut previous: Option<Pos2> = None;
        for count in 0..8 {
            let at = spawn_position(&view, Some(rect()), count);
            assert!(
                (at - rect().center()).length() < 160.0,
                "count {count}: spawn {at:?} drifted too far from the center"
            );
            if let Some(prev) = previous {
                assert_ne!(at, prev, "count {count}: spawn stacked on the previous one");
            }
            previous = Some(at);
        }
    }

    /// After a real draw pass the toolbar must find last frame's canvas rect
    /// under the key it reads (the storage half of the hand-off).
    #[test]
    fn draw_pass_publishes_the_canvas_rect() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        let mut block = json!({
            "id": "spawn-canvas",
            "type": "interactive_canvas",
            "data": {"elements": [], "height": 400.0}
        });
        let mut out = ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("spawn-canvas-test"))
                .fixed_pos(egui::Pos2::ZERO)
                .show(ctx, |ui| {
                    let mut op: Option<BlockOp> = None;
                    let _ =
                        draw_canvas_block(ui, &mut block, &[], crate::i18n::Locale::En, &mut op);
                });
        });
        out.textures_delta.clear();
        let stored = ctx.data(|d| {
            d.get_temp::<Rect>(egui::Id::new(("canvas-rect", "spawn-canvas".to_string())))
        });
        let rect = stored.expect("canvas rect must be published for the toolbar");
        assert!((rect.height() - 400.0).abs() < 1.0, "rect {rect:?}");
    }

    /// The positioned factory honours `at` verbatim.
    #[test]
    fn factory_builds_at_the_given_position() {
        let el = canvas_element_at("sticky", egui::pos2(60.0, 60.0), crate::i18n::Locale::En);
        assert_eq!(el.get("x").and_then(Value::as_f64), Some(60.0));
        assert_eq!(el.get("type").and_then(Value::as_str), Some("sticky"));
    }
}

#[cfg(test)]
mod image_node_tests {
    use super::*;
    use base64::Engine as _;

    /// A small PNG built in-process: no magic constants, the codec under the
    /// hood is the same one the texture path uses.
    fn tiny_png_b64() -> String {
        let img = image::RgbaImage::from_pixel(8, 6, image::Rgba([200u8, 40, 40, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img.into())
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode png");
        base64::engine::general_purpose::STANDARD.encode(png.get_ref())
    }

    fn picture_attachment() -> crate::mime::Attachment {
        let b64 = tiny_png_b64();
        crate::mime::Attachment {
            name: "shot.png".into(),
            cid: "image-shot".into(),
            mime_type: "image/png".into(),
            content: String::new(),
            size_bytes: 64,
            raw_b64: Some(b64),
        }
    }

    /// The factory builds the default picture node: 180x120, no text.
    #[test]
    fn image_factory_defaults() {
        let el = canvas_element_at("image", pos2(10.0, 10.0), crate::i18n::Locale::En);
        assert_eq!(el.get("type").and_then(Value::as_str), Some("image"));
        assert_eq!(el.get("width").and_then(Value::as_f64), Some(180.0));
        assert_eq!(el.get("height").and_then(Value::as_f64), Some(120.0));
        assert_eq!(el.get("text").and_then(Value::as_str), Some(""));
    }

    /// cid resolution: prefix stripped, only image MIMEs match.
    #[test]
    fn image_attachment_resolves_by_cid() {
        let mut el = canvas_element_at("image", pos2(0.0, 0.0), crate::i18n::Locale::En);
        el["cid"] = json!("cid:image-shot");
        let good = picture_attachment();
        let attachments = vec![
            good,
            crate::mime::Attachment {
                name: "notes.txt".into(),
                cid: "image-shot".into(),
                mime_type: "text/plain".into(),
                content: "decoy".into(),
                size_bytes: 5,
                raw_b64: None,
            },
        ];
        let found = image_attachment(&el, &attachments).expect("resolves");
        assert_eq!(found.mime_type, "image/png");
        el["cid"] = json!("cid:missing");
        assert!(image_attachment(&el, &attachments).is_none());
    }

    /// Letterbox: aspect kept, centered, never exceeds the bounds.
    #[test]
    fn fit_inside_letterboxes() {
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0));
        // Wide picture: capped by the bounds' width, vertically centered.
        let fit = fit_inside(vec2(400.0, 100.0), bounds);
        assert_eq!(fit.width(), 200.0);
        assert_eq!(fit.height(), 50.0);
        assert!((fit.center().y - 50.0).abs() < 0.01);
        // Tall picture: capped by the bounds' height.
        let fit = fit_inside(vec2(50.0, 200.0), bounds);
        assert_eq!(fit.width(), 25.0);
        assert_eq!(fit.height(), 100.0);
        // Degenerate natural size: fall back to the whole bounds.
        assert_eq!(fit_inside(vec2(0.0, 0.0), bounds), bounds);
        // Aspect survives.
        let fit = fit_inside(vec2(300.0, 150.0), bounds);
        assert!((fit.width() / fit.height() - 2.0).abs() < 0.01);
    }

    /// Drawing a board with a resolved picture must not panic headlessly,
    /// and neither must the placeholder path (cid set, attachment missing).
    #[test]
    fn image_nodes_draw_headlessly() {
        let ctx = egui::Context::default();
        for cid in ["cid:image-shot", "cid:missing"] {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            };
            let mut block = json!({
                "id": "img-canvas",
                "type": "interactive_canvas",
                "data": {
                    "elements": [
                        {"id":"im1","type":"image","x":20.0,"y":20.0,"width":180.0,
                         "height":120.0,"text":"","color":"","cid":cid}
                    ]
                }
            });
            let attachment = picture_attachment();
            let mut op: Option<BlockOp> = None;
            let mut out = ctx.run_pass(raw, |ctx| {
                egui::Area::new(egui::Id::new("img-canvas-test"))
                    .fixed_pos(egui::Pos2::ZERO)
                    .show(ctx, |ui| {
                        let _ = draw_canvas_block(
                            ui,
                            &mut block,
                            std::slice::from_ref(&attachment),
                            crate::i18n::Locale::En,
                            &mut op,
                        );
                    });
            });
            out.textures_delta.clear();
        }
    }
}

#[cfg(test)]
mod ink_tests {
    use super::*;

    /// The bbox spans the stroke's points; empty and single-point strokes
    /// degrade sanely.
    #[test]
    fn ink_bbox_spans_points() {
        let rect = ink_bbox(&[pos2(10.0, 30.0), pos2(40.0, 5.0), pos2(25.0, 60.0)]);
        assert_eq!(rect.min, pos2(10.0, 5.0));
        assert_eq!(rect.max, pos2(40.0, 60.0));
        assert_eq!(ink_bbox(&[]), Rect::NOTHING);
        let single = ink_bbox(&[pos2(3.0, 4.0)]);
        assert_eq!(single.min, pos2(3.0, 4.0));
        assert_eq!(single.max, pos2(3.0, 4.0));
    }

    /// The element keeps the bbox in x/y/width/height and the points
    /// relative to its origin; reconstructing the absolute points from the
    /// element matches the input (0.1 rounding tolerance).
    #[test]
    fn ink_element_stores_relative_points() {
        let pts = vec![pos2(10.0, 30.0), pos2(40.0, 5.0), pos2(25.0, 60.0)];
        let el = ink_element("el_1".into(), &pts);
        assert_eq!(el.get("type").and_then(Value::as_str), Some("ink"));
        assert_eq!(el.get("x").and_then(Value::as_f64), Some(10.0));
        assert_eq!(el.get("y").and_then(Value::as_f64), Some(5.0));
        assert_eq!(el.get("width").and_then(Value::as_f64), Some(30.0));
        assert_eq!(el.get("height").and_then(Value::as_f64), Some(55.0));
        let back = ink_points(&el);
        assert_eq!(back.len(), 3);
        for (a, b) in pts.iter().zip(&back) {
            assert!((a.x - b.x).abs() < 0.06, "x: {a} vs {b}");
            assert!((a.y - b.y).abs() < 0.06, "y: {a} vs {b}");
        }
    }

    /// A single-click stroke gets a 1x1 bbox; the point stays at the origin.
    #[test]
    fn ink_dot_element() {
        let el = ink_element("el_2".into(), &[pos2(7.0, 9.0)]);
        assert_eq!(el.get("width").and_then(Value::as_f64), Some(1.0));
        assert_eq!(el.get("height").and_then(Value::as_f64), Some(1.0));
        assert_eq!(ink_points(&el), vec![pos2(7.0, 9.0)]);
    }

    /// Ink elements are plain JSON: a serialize/parse cycle keeps the stroke.
    #[test]
    fn ink_element_survives_json_roundtrip() {
        let el = ink_element("el_3".into(), &[pos2(0.0, 0.0), pos2(15.5, 20.25)]);
        let parsed: Value = serde_json::from_str(&el.to_string()).expect("valid json");
        assert_eq!(ink_points(&parsed).len(), 2);
    }

    /// A board with strokes (polyline and dot, pen flag on and off) draws
    /// headlessly without panicking.
    #[test]
    fn ink_draws_headlessly() {
        let ctx = egui::Context::default();
        for pen in [false, true] {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            };
            let mut block = json!({
                "id": "ink-canvas",
                "type": "interactive_canvas",
                "data": {
                    "pen": pen,
                    "elements": [
                        {"id":"s1","type":"ink","x":10.0,"y":10.0,"width":80.0,
                         "height":40.0,"color":"","text":"",
                         "points":[[0.0,0.0],[40.0,20.0],[80.0,0.0]]},
                        {"id":"d1","type":"ink","x":5.0,"y":5.0,"width":1.0,
                         "height":1.0,"color":"","text":"","points":[[0.0,0.0]]}
                    ]
                }
            });
            let mut op: Option<BlockOp> = None;
            let mut out = ctx.run_pass(raw, |ctx| {
                egui::Area::new(egui::Id::new("ink-canvas-test"))
                    .fixed_pos(egui::Pos2::ZERO)
                    .show(ctx, |ui| {
                        let _ = draw_canvas_block(
                            ui,
                            &mut block,
                            &[],
                            crate::i18n::Locale::En,
                            &mut op,
                        );
                    });
            });
            out.textures_delta.clear();
        }
    }
}
