use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Ui};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::theme;
use crate::views::View;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobileNavAction {
    #[default]
    None,
    ToggleSidebar,
    SwitchView(View),
    OpenSearch,
}

pub fn draw_mobile_bottom_nav(
    ui: &mut Ui,
    current_view: View,
    _sidebar_collapsed: bool,
    locale: Locale,
) -> MobileNavAction {
    let mut action = MobileNavAction::None;
    let total_rect = ui.max_rect();
    let num_items = 5.0;
    let item_width = total_rect.width() / num_items;
    let item_height = total_rect.height();

    // Top hairline stroke separating bottom bar from central panel
    ui.painter().hline(
        total_rect.x_range(),
        total_rect.top(),
        theme::hairline(),
    );

    let items = [
        (
            Icon::Doc,
            t(locale, Key::NavPage),
            matches!(current_view, View::Page),
            0,
        ),
        (
            Icon::Graph,
            t(locale, Key::NavGraph),
            matches!(current_view, View::Graph),
            1,
        ),
        (
            Icon::Board,
            t(locale, Key::NavPlanning),
            matches!(current_view, View::Planning),
            2,
        ),
        (
            Icon::Mail,
            t(locale, Key::NavThreads),
            matches!(current_view, View::Threads),
            3,
        ),
        (
            Icon::Search,
            t(locale, Key::NavSearch),
            false,
            4,
        ),
    ];

    for (icon, label, is_active, idx) in items {
        let x_min = total_rect.left() + idx as f32 * item_width;
        let rect = Rect::from_min_size(pos2(x_min, total_rect.top()), vec2(item_width, item_height));
        let resp = ui.allocate_rect(rect, Sense::click());

        let is_hovered = resp.hovered();
        let (icon_color, text_color) = if is_active {
            (theme::accent(), theme::accent())
        } else if is_hovered {
            (theme::ink(), theme::ink())
        } else {
            (theme::muted(), theme::muted())
        };

        if is_active {
            // Subtle active pill highlight
            let pill_rect = Rect::from_center_size(
                pos2(rect.center().x, rect.top() + 18.0),
                vec2(item_width.min(48.0), 28.0),
            );
            ui.painter().rect_filled(pill_rect, 6.0, theme::wash());
        }

        // Draw icon
        let icon_box = Rect::from_center_size(
            pos2(rect.center().x, rect.top() + 16.0),
            vec2(18.0, 18.0),
        );
        icons::paint(ui.painter(), icon_box, icon, icon_color);

        // Draw text label
        ui.painter().text(
            pos2(rect.center().x, rect.top() + 34.0),
            Align2::CENTER_CENTER,
            label,
            FontId::new(10.0, theme::display()),
            text_color,
        );

        if resp.clicked() {
            match idx {
                0 => {
                    if current_view == View::Page {
                        action = MobileNavAction::ToggleSidebar;
                    } else {
                        action = MobileNavAction::SwitchView(View::Page);
                    }
                }
                1 => action = MobileNavAction::SwitchView(View::Graph),
                2 => action = MobileNavAction::SwitchView(View::Planning),
                3 => action = MobileNavAction::SwitchView(View::Threads),
                4 => action = MobileNavAction::OpenSearch,
                _ => {}
            }
        }
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_nav_renders_headlessly_on_mobile_width() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(380.0, 800.0))),
            ..Default::default()
        };
        let mut action = MobileNavAction::None;
        let mut out = ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("bottom-nav-test"))
                .fixed_pos(pos2(0.0, 752.0))
                .show(ctx, |ui| {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(Rect::from_min_size(pos2(0.0, 752.0), vec2(380.0, 48.0))),
                    );
                    action = draw_mobile_bottom_nav(&mut child, View::Page, true, Locale::Ru);
                });
        });
        out.textures_delta.clear();
        assert_eq!(action, MobileNavAction::None);
    }

    #[test]
    fn bottom_nav_click_switches_view() {
        let ctx = egui::Context::default();
        let target_pos = pos2(114.0, 776.0);
        let mut raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(380.0, 800.0))),
            ..Default::default()
        };
        let mut action = MobileNavAction::None;

        let mut step = |raw: egui::RawInput| -> MobileNavAction {
            let mut act = MobileNavAction::None;
            let mut out = ctx.run_ui(raw, |ui| {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(pos2(0.0, 752.0), vec2(380.0, 48.0))),
                );
                act = draw_mobile_bottom_nav(&mut child, View::Page, true, Locale::Ru);
            });
            out.textures_delta.clear();
            act
        };

        // 1. Move pointer
        raw.events.push(egui::Event::PointerMoved(target_pos));
        step(raw.clone());

        // 2. Press
        raw.events.push(egui::Event::PointerButton {
            pos: target_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        step(raw.clone());

        // 3. Release
        raw.events.push(egui::Event::PointerButton {
            pos: target_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        action = step(raw.clone());

        assert_eq!(action, MobileNavAction::SwitchView(View::Graph));
    }
}
