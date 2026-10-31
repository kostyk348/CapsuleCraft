//! Toast notification overlay renderer.

use std::time::{Duration, Instant};

use egui::{pos2, vec2, Rect, Stroke};

use crate::theme;

pub fn draw_toast(toast: &mut Option<(String, Instant)>, ctx: &egui::Context) {
    let Some((text, at)) = toast.clone() else {
        return;
    };
    let elapsed = at.elapsed();
    if elapsed > Duration::from_millis(2600) {
        *toast = None;
        return;
    }
    let alpha = if elapsed > Duration::from_millis(2100) {
        (1.0 - (elapsed.as_millis() as f32 - 2100.0) / 500.0).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let screen = ctx.content_rect();
    let galley = ctx.fonts_mut(|fonts| {
        fonts.layout_no_wrap(text, egui::FontId::new(12.0, theme::medium()), theme::INK)
    });
    let size = galley.size() + vec2(28.0, 18.0);
    let rect = Rect::from_min_size(
        pos2(
            screen.center().x - size.x / 2.0,
            screen.bottom() - size.y - 34.0,
        ),
        size,
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("toast"),
    ));
    painter.rect(
        rect,
        theme::RADIUS,
        theme::fade(theme::PANEL, alpha),
        Stroke::new(1.0, theme::fade(theme::LINE_STRONG, alpha)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.min + vec2(14.0, 9.0),
        galley,
        theme::fade(theme::INK, alpha),
    );
    ctx.request_repaint();
}
