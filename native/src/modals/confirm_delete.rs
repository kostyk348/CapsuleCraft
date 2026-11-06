//! Confirmation modal dialog for deleting pages.

use egui::{vec2, Align, Label, Layout, Rect, RichText, Sense, Stroke, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::theme;
use crate::{primary_button, secondary_button};

pub enum ConfirmDeleteAction {
    None,
    Confirm,
    Cancel,
}

/// `title`/`body` select between the page and the block confirmation.
pub fn draw_confirm_delete(
    show: bool,
    title: String,
    body_text: String,
    locale: Locale,
    ctx: &egui::Context,
) -> ConfirmDeleteAction {
    if !show {
        return ConfirmDeleteAction::None;
    }

    let mut action = ConfirmDeleteAction::None;

    egui::Area::new(egui::Id::new("confirm-delete"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));
            let panel = Rect::from_center_size(rect.center(), vec2(380.0, 150.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::panel(),
                Stroke::new(1.0, theme::line_strong()),
                egui::StrokeKind::Inside,
            );
            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(20.0))
                    .layout(Layout::top_down(Align::Min)),
            );
            let _ = locale;
            body.label(
                RichText::new(title)
                    .family(theme::medium())
                    .size(14.0)
                    .color(theme::ink()),
            );
            body.add_space(6.0);
            body.add(
                Label::new(
                    RichText::new(body_text)
                        .size(12.0)
                        .color(theme::muted()),
                )
                .wrap(),
            );
            body.add_space(14.0);
            body.horizontal(|ui| {
                if primary_button(ui, t(locale, Key::Delete), 28.0).clicked() {
                    action = ConfirmDeleteAction::Confirm;
                }
                if secondary_button(ui, None, t(locale, Key::Cancel), 28.0).clicked() {
                    action = ConfirmDeleteAction::Cancel;
                }
            });
        });

    action
}
