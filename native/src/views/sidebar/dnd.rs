//! Drag-and-drop helpers for sidebar note reordering and reparenting.

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, Ui};

use crate::i18n::{t, Key, Locale};
use crate::mime::Capsule;
use crate::theme;
use crate::truncate_chars;
use crate::vault::PageTree;
use crate::ShellAction;

/// Promote-to-root drop zone at the bottom of the page list.
pub fn draw_root_drop_zone(
    ui: &mut Ui,
    tree: &PageTree,
    dragged: usize,
    drop_target: &mut Option<Option<String>>,
    locale: Locale,
) {
    ui.add_space(8.0);
    let (root_drop_rect, root_drop_resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let is_already_root = tree.parent_of(dragged).is_none();
    // `hovered()` is suppressed for every widget while a drag is in flight
    // (egui: "while dragging or clicking something, nothing else is
    // hovered") — the drop zone must look at the pointer position instead.
    let hovered = root_drop_resp.contains_pointer() && !is_already_root;

    ui.painter().rect_stroke(
        root_drop_rect,
        theme::RADIUS,
        Stroke::new(
            1.0,
            if hovered {
                theme::accent()
            } else {
                theme::fade(theme::line(), 0.4)
            },
        ),
        egui::StrokeKind::Inside,
    );

    if hovered {
        ui.painter().rect_filled(
            root_drop_rect,
            theme::RADIUS,
            theme::fade(theme::accent(), 0.1),
        );
        *drop_target = Some(None);
    }

    let text = t(locale, Key::MoveToRoot);
    ui.painter().text(
        root_drop_rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::new(11.0, theme::mono()),
        if hovered {
            theme::accent()
        } else {
            theme::faint()
        },
    );
}

/// Floating drag preview badge that follows the pointer.
pub fn draw_drag_preview(ui: &Ui, capsule: &Capsule) {
    if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("page-drag-preview"),
        ));
        let preview_rect = Rect::from_min_size(
            pos2(pointer_pos.x + 12.0, pointer_pos.y + 12.0),
            vec2(140.0, 24.0),
        );
        painter.rect_filled(preview_rect, theme::RADIUS, theme::wash_selected());
        painter.rect_stroke(
            preview_rect,
            theme::RADIUS,
            Stroke::new(1.0, theme::accent()),
            egui::StrokeKind::Inside,
        );
        painter.text(
            pos2(preview_rect.left() + 8.0, preview_rect.center().y),
            Align2::LEFT_CENTER,
            truncate_chars(&capsule.subject, 18),
            FontId::new(11.0, theme::medium()),
            Color32::WHITE,
        );
    }
}

/// Finalize a drop on mouse button release.
pub fn handle_drag_release(
    ui: &Ui,
    dragged_page: &mut Option<usize>,
    drop_target: Option<Option<String>>,
    action: &mut ShellAction,
) {
    if let Some(dragged) = *dragged_page {
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(target) = drop_target {
                action.reparent = Some((dragged, target));
            }
            *dragged_page = None;
        }
    }
}
