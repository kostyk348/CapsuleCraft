//! Appearance settings modal: theme presets, accent colour, interface scale.
//!
//! Obsidian-style: pick a full theme, layer a custom accent over it, size the
//! type. Every change applies immediately (the egui visuals are re-applied)
//! and persists to `native-appearance.json`.

use egui::{vec2, Align, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::theme::{self, ThemeDef};
use crate::{primary_button, secondary_button};

/// Colour choices for the accent row. The first chip is "reset to theme".
const ACCENTS: [Color32; 8] = [
    Color32::from_rgb(0x2d, 0x81, 0xff),
    Color32::from_rgb(0xe5, 0x48, 0x4d),
    Color32::from_rgb(0x30, 0xa4, 0x6c),
    Color32::from_rgb(0xf5, 0xa6, 0x23),
    Color32::from_rgb(0x8e, 0x4e, 0xc6),
    Color32::from_rgb(0x12, 0xa5, 0x94),
    Color32::from_rgb(0xd6, 0x40, 0x9f),
    Color32::from_rgb(0xb8, 0xb8, 0xb8),
];

pub enum AppearanceAction {
    None,
    Close,
}

pub fn draw_appearance(show: bool, locale: Locale, ctx: &egui::Context) -> AppearanceAction {
    if !show {
        return AppearanceAction::None;
    }

    let mut action = AppearanceAction::None;

    egui::Area::new(egui::Id::new("appearance"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(560.0, 540.0));
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

            body.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, None, "✕", 24.0).clicked() {
                        action = AppearanceAction::Close;
                    }
                    ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                        ui.label(
                            RichText::new(t(locale, Key::AppearanceTitle))
                                .family(theme::semibold())
                                .size(16.0)
                                .color(theme::ink()),
                        );
                    });
                });
            });
            body.add_space(10.0);

            // ---- Theme presets -------------------------------------------
            body.label(
                RichText::new(t(locale, Key::AppearanceTheme))
                    .family(theme::medium())
                    .size(13.0)
                    .color(theme::muted()),
            );
            body.add_space(6.0);
            let themes: &[ThemeDef] = theme::preset_list();
            let current = theme::preset().id;
            let columns = 2;
            for chunk in themes.chunks(columns) {
                body.horizontal(|ui| {
                    for preset in chunk {
                        let selected = preset.id == current;
                        let (card, response) = ui.allocate_exact_size(
                            vec2(250.0, 54.0),
                            Sense::click(),
                        );
                        let stroke = if selected {
                            Stroke::new(2.0, theme::accent_color())
                        } else {
                            Stroke::new(1.0, theme::line())
                        };
                        ui.painter().rect(
                            card,
                            6.0,
                            preset.bg,
                            stroke,
                            egui::StrokeKind::Inside,
                        );
                        // Swatches: page, controls, accent of THAT theme.
                        for (i, colour) in [preset.controls, preset.sunken, preset.accent]
                            .iter()
                            .enumerate()
                        {
                            let chip = Rect::from_center_size(
                                card.min + vec2(26.0 + i as f32 * 22.0, 27.0),
                                vec2(16.0, 16.0),
                            );
                            ui.painter().rect_filled(chip, 4.0, *colour);
                        }
                        ui.painter().text(
                            card.min + vec2(96.0, 27.0),
                            egui::Align2::LEFT_CENTER,
                            preset.label,
                            egui::FontId::proportional(13.0),
                            if selected {
                                theme::ink_strong()
                            } else {
                                theme::muted()
                            },
                        );
                        if response.clicked() {
                            theme::set_preset(preset.id);
                            theme::persist();
                            theme::apply(ctx);
                        }
                    }
                });
                body.add_space(4.0);
            }
            body.add_space(12.0);

            // ---- Accent colour -------------------------------------------
            body.label(
                RichText::new(t(locale, Key::AppearanceAccent))
                    .family(theme::medium())
                    .size(13.0)
                    .color(theme::muted()),
            );
            body.add_space(6.0);
            body.horizontal(|ui| {
                let overridden = {
                    // The override is private; expose "is default" by comparing.
                    theme::accent_color() != theme::preset().accent
                };
                // First chip: reset to the theme's own accent.
                let (rect, response) =
                    ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                ui.painter().rect(
                    rect,
                    5.0,
                    theme::panel(),
                    Stroke::new(
                        if overridden { 2.0 } else { 1.0 },
                        if overridden {
                            theme::accent_color()
                        } else {
                            theme::line_strong()
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "×",
                    egui::FontId::proportional(15.0),
                    theme::muted(),
                );
                if response.clicked() {
                    theme::set_accent(None);
                    theme::persist();
                    theme::apply(ctx);
                }
                ui.add_space(2.0);
                for colour in ACCENTS {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                    let selected = theme::accent_color() == colour;
                    ui.painter().rect_filled(rect, 5.0, colour);
                    if selected {
                        ui.painter().rect(
                            rect.expand(2.0),
                            7.0,
                            Color32::TRANSPARENT,
                            Stroke::new(2.0, theme::accent_color()),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.clicked() {
                        theme::set_accent(Some(colour));
                        theme::persist();
                        theme::apply(ctx);
                    }
                    ui.add_space(2.0);
                }
            });
            body.add_space(12.0);

            // ---- Interface scale ------------------------------------------
            body.label(
                RichText::new(t(locale, Key::AppearanceScale))
                    .family(theme::medium())
                    .size(13.0)
                    .color(theme::muted()),
            );
            body.add_space(6.0);
            let mut scale = theme::font_scale();
            let slider = egui::Slider::new(&mut scale, 0.8..=1.4)
                .step_by(0.05)
                .show_value(false);
            let slider = slider.custom_formatter(|v, _| format!("{} %", (v * 100.0).round()));
            let response = body.add(slider);
            if response.changed() {
                theme::set_font_scale(scale);
                theme::persist();
                theme::apply(ctx);
            }
            body.add_space(14.0);

            body.with_layout(egui::Layout::right_to_left(Align::Min), |ui| {
                if primary_button(ui, t(locale, Key::AppearanceDone), 28.0).clicked() {
                    action = AppearanceAction::Close;
                }
            });
        });

    action
}
