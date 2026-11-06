//! Co-Thinker interactive modal dialog.

use egui::{
    vec2, Align, Frame, Label, Layout, Margin, Rect, RichText, Sense, Stroke, TextEdit, UiBuilder,
};

use crate::cothinker::CoThinkMode;
use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::theme;
use crate::{ghost, primary_button, secondary_button};

pub enum CoThinkModalAction {
    None,
    Close,
    Run,
}

pub fn draw_cothink_modal(
    show: bool,
    mode: &mut CoThinkMode,
    focus: &mut String,
    locale: Locale,
    ctx: &egui::Context,
) -> CoThinkModalAction {
    if !show {
        return CoThinkModalAction::None;
    }

    let mut action = CoThinkModalAction::None;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = CoThinkModalAction::Close;
    }

    egui::Area::new(egui::Id::new("cothink-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            if resp.clicked() {
                action = CoThinkModalAction::Close;
            }
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(540.0, 390.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::panel(),
                Stroke::new(1.0, theme::line_strong()),
                egui::StrokeKind::Inside,
            );

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(22.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            body.horizontal(|ui| {
                ui.label(
                    RichText::new(t(locale, Key::CoThinkTitle))
                        .family(theme::medium())
                        .size(15.0)
                        .color(theme::ink()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, Icon::Close, None, t(locale, Key::Close), true, 26.0).clicked() {
                        action = CoThinkModalAction::Close;
                    }
                });
            });
            body.add_space(4.0);
            body.add(
                Label::new(
                    RichText::new(t(locale, Key::CoThinkSubtitle))
                        .size(12.0)
                        .color(theme::muted()),
                )
                .wrap(),
            );
            body.add_space(14.0);

            let modes = [
                (CoThinkMode::Critique, Key::Critique, Key::CritiqueDesc),
                (CoThinkMode::Synthesis, Key::Synthesis, Key::SynthesisDesc),
                (
                    CoThinkMode::CounterArg,
                    Key::CounterArg,
                    Key::CounterArgDesc,
                ),
                (
                    CoThinkMode::DataInsight,
                    Key::DataInsight,
                    Key::DataInsightDesc,
                ),
            ];

            for (candidate, title_key, desc_key) in modes {
                let active = *mode == candidate;
                let (fill, stroke_color, text_color) = if active {
                    (theme::wash_selected(), theme::accent(), theme::accent_ink())
                } else {
                    (theme::sunken(), theme::hairline().color, theme::ink())
                };

                let frame = Frame::new()
                    .fill(fill)
                    .corner_radius(theme::RADIUS)
                    .stroke(Stroke::new(1.0, stroke_color))
                    .inner_margin(Margin {
                        left: 10,
                        right: 10,
                        top: 6,
                        bottom: 6,
                    });

                let resp = frame
                    .show(&mut body, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(t(locale, title_key))
                                    .family(theme::medium())
                                    .size(13.0)
                                    .color(text_color),
                            );
                            ui.label(
                                RichText::new(format!("— {}", t(locale, desc_key)))
                                    .size(11.5)
                                    .color(theme::muted()),
                            );
                        });
                    })
                    .response;

                if resp.interact(Sense::click()).clicked() {
                    *mode = candidate;
                }
                body.add_space(6.0);
            }

            body.add_space(8.0);
            body.label(
                RichText::new(t(locale, Key::CoThinkFocus))
                    .size(12.0)
                    .color(theme::muted()),
            );
            body.add_space(4.0);
            let text_edit = TextEdit::singleline(focus)
                .hint_text(t(locale, Key::CoThinkFocusPlaceholder))
                .desired_width(body.available_width());
            body.add(text_edit);

            body.add_space(14.0);
            body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, Some(Icon::Close), t(locale, Key::Cancel), 28.0).clicked() {
                    action = CoThinkModalAction::Close;
                }
                ui.add_space(8.0);
                if primary_button(ui, t(locale, Key::CoThinkRun), 28.0).clicked() {
                    action = CoThinkModalAction::Run;
                }
            });
        });

    action
}
