//! Undecorated-window chrome: title-bar dragging, Windows-style caption
//! buttons and edge resizing.

use egui::{vec2, Color32, Rect, Response, Sense, Stroke, Ui};

use crate::theme;

/// How far in from the window's edges a drag resizes.
const RESIZE_EDGE: f32 = 5.0;
/// Caption buttons are 46 px wide, like Windows' own.
const BUTTON_WIDTH: f32 = 46.0;
const CLOSE_RED: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);

pub const TITLEBAR_HEIGHT: f32 = 32.0;
pub const WORKSPACE_STRIP_HEIGHT: f32 = 34.0;

pub fn custom_caption_buttons() -> bool {
    !cfg!(target_os = "macos")
}

pub fn leading_inset() -> f32 {
    if cfg!(target_os = "macos") {
        76.0
    } else {
        0.0
    }
}

/// Dragging empty title-bar space moves the window; double-click maximizes.
pub fn title_bar_drag(ui: &mut Ui) {
    let r = ui.max_rect();
    let rect = Rect::from_min_max(
        r.min + vec2(RESIZE_EDGE, RESIZE_EDGE),
        r.max - vec2(RESIZE_EDGE, 0.0),
    );
    let resp = ui.interact(
        rect,
        ui.scope_id().with("title-bar-drag"),
        Sense::click_and_drag(),
    );
    if resp.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// Dragging a specific title-bar rectangle moves the window; double-click maximizes.
pub fn title_bar_drag_rect(ui: &mut Ui, rect: Rect) {
    if rect.width() <= 10.0 || rect.height() <= 10.0 {
        return;
    }
    let edge_rect = Rect::from_min_max(rect.min + vec2(0.0, RESIZE_EDGE), rect.max);
    let resp = ui.interact(
        edge_rect,
        ui.scope_id().with("title-bar-drag-rect"),
        Sense::click_and_drag(),
    );
    if resp.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Minimize, maximize/restore and close, for a right-to-left layout.
pub fn caption_buttons(ui: &mut Ui, height: f32) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    if caption_button(ui, Caption::Close, height).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let size = if maximized {
        Caption::Restore
    } else {
        Caption::Maximize
    };
    if caption_button(ui, size, height).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if caption_button(ui, Caption::Minimize, height).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

fn caption_button(ui: &mut Ui, button: Caption, height: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(BUTTON_WIDTH, height), Sense::hover());
    let right = if button == Caption::Close {
        RESIZE_EDGE
    } else {
        0.0
    };
    let hit = Rect::from_min_max(
        rect.min + vec2(0.0, RESIZE_EDGE),
        rect.max - vec2(right, 0.0),
    );
    let resp = ui
        .interact(
            hit,
            ui.scope_id().with(("caption", button as u8)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::Default);
    let (fill, ink) = match (button, resp.hovered()) {
        (Caption::Close, true) => (CLOSE_RED, Color32::WHITE),
        (_, true) => (theme::WASH_HOVER, theme::INK_STRONG),
        (_, false) => (Color32::TRANSPARENT, theme::MUTED),
    };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, fill);
    let c = rect.center();
    let stroke = Stroke::new(1.0, ink);
    let s = 5.0;
    let label = match button {
        Caption::Minimize => {
            p.hline(c.x - s..=c.x + s, c.y, stroke);
            "Minimize"
        }
        Caption::Maximize => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(2.0 * s, 2.0 * s)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            "Maximize"
        }
        Caption::Restore => {
            let front =
                Rect::from_min_size(c + vec2(-s, -s + 2.0), vec2(2.0 * s - 2.0, 2.0 * s - 2.0));
            p.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
            p.hline(c.x - s + 2.0..=c.x + s, c.y - s, stroke);
            p.vline(c.x + s, c.y - s..=c.y + s - 2.0, stroke);
            "Restore"
        }
        Caption::Close => {
            p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
            p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
            "Close"
        }
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp.on_hover_text(label)
}

/// A hairline round the window and resizing from its edges.
pub fn window_edges(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    use egui::CursorIcon as C;
    let (maximized, fullscreen) = ctx.input(|i| {
        (
            i.viewport().maximized.unwrap_or(false),
            i.viewport().fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen || !custom_caption_buttons() {
        return;
    }
    let r = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("window-border"),
    ))
    .rect_stroke(r, 0.0, theme::hairline(), egui::StrokeKind::Inside);
    let Some(p) = ctx
        .input(|i| i.pointer.hover_pos())
        .filter(|p| r.contains(*p))
    else {
        return;
    };
    let near = |d: f32| d < RESIZE_EDGE;
    let corner = |d: f32| d < 3.0 * RESIZE_EDGE;
    let (left, right, top, bottom) = (
        p.x - r.left(),
        r.right() - p.x,
        p.y - r.top(),
        r.bottom() - p.y,
    );
    let hit = if (near(top) && corner(left)) || (near(left) && corner(top)) {
        Some((D::NorthWest, C::ResizeNorthWest))
    } else if (near(top) && corner(right)) || (near(right) && corner(top)) {
        Some((D::NorthEast, C::ResizeNorthEast))
    } else if (near(bottom) && corner(left)) || (near(left) && corner(bottom)) {
        Some((D::SouthWest, C::ResizeSouthWest))
    } else if (near(bottom) && corner(right)) || (near(right) && corner(bottom)) {
        Some((D::SouthEast, C::ResizeSouthEast))
    } else if near(top) {
        Some((D::North, C::ResizeNorth))
    } else if near(bottom) {
        Some((D::South, C::ResizeSouth))
    } else if near(left) {
        Some((D::West, C::ResizeWest))
    } else if near(right) {
        Some((D::East, C::ResizeEast))
    } else {
        None
    };
    if let Some((direction, cursor)) = hit {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}
