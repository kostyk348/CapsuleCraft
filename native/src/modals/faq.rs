//! Built-in FAQ modal dialog.

use egui::{vec2, Align, Label, Layout, Rect, RichText, ScrollArea, Sense, Stroke, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::primary_button;
use crate::theme;

pub fn draw_faq_modal(show: bool, locale: Locale, ctx: &egui::Context) -> bool {
    if !show {
        return false;
    }

    let mut close = false;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }

    egui::Area::new(egui::Id::new("faq-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            if resp.clicked() {
                close = true;
            }
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(720.0, 620.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::PANEL,
                Stroke::new(1.0, theme::LINE_STRONG),
                egui::StrokeKind::Inside,
            );

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(24.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            body.label(
                RichText::new(t(locale, Key::FaqTitle))
                    .family(theme::medium())
                    .size(19.0)
                    .color(theme::INK),
            );
            body.add_space(2.0);
            body.add(
                Label::new(
                    RichText::new(t(locale, Key::FaqSubtitle))
                        .size(12.0)
                        .color(theme::MUTED),
                )
                .wrap(),
            );
            body.add_space(12.0);

            let pairs = [
                (Key::FaqQ1, Key::FaqA1),
                (Key::FaqQ2, Key::FaqA2),
                (Key::FaqQ3, Key::FaqA3),
                (Key::FaqQ4, Key::FaqA4),
                (Key::FaqQ5, Key::FaqA5),
                (Key::FaqQ6, Key::FaqA6),
                (Key::FaqQ7, Key::FaqA7),
                (Key::FaqQ8, Key::FaqA8),
            ];
            ScrollArea::vertical()
                .max_height(panel.height() - 175.0)
                .auto_shrink([false, false])
                .show(&mut body, |ui| {
                    for (question, answer) in pairs {
                        ui.label(
                            RichText::new(t(locale, question))
                                .family(theme::medium())
                                .size(13.5)
                                .color(theme::INK),
                        );
                        ui.add_space(3.0);
                        ui.add(
                            Label::new(
                                RichText::new(t(locale, answer))
                                    .size(12.5)
                                    .color(theme::MUTED),
                            )
                            .wrap(),
                        );
                        ui.add_space(12.0);
                    }
                });

            body.add_space(6.0);
            if primary_button(&mut body, t(locale, Key::FaqCloseShort), 30.0).clicked() {
                close = true;
            }
        });

    close
}
