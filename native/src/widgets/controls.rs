use std::sync::Arc;

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Label,
    Layout, Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextFormat, Ui, UiBuilder,
};

use crate::format_date_long;
use crate::human_bytes;
use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::Capsule;
use crate::theme;
use crate::truncate_chars;
pub fn caps(
    ui: &Ui,
    text: &str,
    size: f32,
    family: FontFamily,
    spacing: f32,
    color: Color32,
) -> Arc<Galley> {
    let format = egui::TextFormat {
        font_id: FontId::new(size, family),
        color,
        extra_letter_spacing: spacing,
        ..Default::default()
    };
    ui.painter()
        .layout_job(egui::text::LayoutJob::single_section(
            text.to_uppercase(),
            format,
        ))
}

pub fn hud(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = caps(
        ui,
        text,
        theme::HUD_SIZE,
        theme::mono_bold(),
        theme::HUD_SPACING,
        color,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, color);
    response
}

pub fn chevron(ui: &mut Ui, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(9.0, 9.0)),
        Icon::ChevronRight,
        color,
    );
    response
}

pub fn crumb_button(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::new(12.0, FontFamily::Proportional),
        theme::MUTED,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(4.0, 2.0), Sense::click());
    let ink = if response.hovered() {
        theme::INK
    } else {
        theme::MUTED
    };
    ui.painter().galley_with_override_text_color(
        pos2(rect.left() + 2.0, rect.top() + 1.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn dot_sep(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(4.0, 10.0), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 1.2, theme::fade(theme::FAINT, 0.8));
}

pub fn tag_chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), FontId::new(9.5, theme::mono()), color);
    let size = galley.size() + vec2(14.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        theme::RADIUS,
        theme::fade(color, 0.07),
        Stroke::new(1.0, theme::fade(color, 0.3)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(pos2(rect.left() + 7.0, rect.top() + 3.0), galley, color);
    response
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(28.0, 28.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hovered = response.hovered() && enabled;
    if hovered || active {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = if !enabled {
        theme::fade(theme::DIM, 0.4)
    } else if hovered || active {
        theme::INK
    } else {
        theme::DIM
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
        icon,
        ink,
    );
    let response = response.on_hover_text(tip);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn ghost(
    ui: &mut Ui,
    icon: Icon,
    label: Option<&str>,
    tip: &str,
    enabled: bool,
    height: f32,
) -> Response {
    let galley = label.map(|label| {
        ui.painter().layout_no_wrap(
            label.to_string(),
            FontId::new(12.0, theme::medium()),
            theme::INK,
        )
    });
    let width = match &galley {
        Some(galley) => 10.0 + 15.0 + 6.0 + galley.size().x + 10.0,
        None => 30.0,
    };
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, height),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hovered = response.hovered() && enabled;
    if hovered {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = match (enabled, hovered) {
        (false, _) => theme::fade(theme::DIM, 0.4),
        (true, true) => theme::INK,
        (true, false) => theme::DIM,
    };
    match galley {
        Some(galley) => {
            let icon_rect = Rect::from_center_size(
                pos2(rect.left() + 10.0 + 7.5, rect.center().y),
                vec2(14.0, 14.0),
            );
            icons::paint(ui.painter(), icon_rect, icon, ink);
            ui.painter().galley_with_override_text_color(
                pos2(
                    icon_rect.right() + 6.0,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                ink,
            );
        }
        None => {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
                icon,
                ink,
            );
        }
    }
    let response = response.on_hover_text(tip);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn text_button(ui: &mut Ui, icon: Option<Icon>, label: &str, active: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::new(11.5, theme::medium()),
        theme::MUTED,
    );
    let width = 8.0 + if icon.is_some() { 14.0 + 6.0 } else { 0.0 } + galley.size().x + 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    let hovered = response.hovered();
    if hovered || active {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = if hovered { theme::INK } else { theme::MUTED };
    let mut x = rect.left() + 8.0;
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(13.0, 13.0)),
            icon,
            ink,
        );
        x += 20.0;
    }
    ui.painter().galley_with_override_text_color(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary_button(ui: &mut Ui, label: &str, height: f32) -> Response {
    let galley = caps(ui, label, 10.0, theme::mono_bold(), 1.2, theme::INVERT_FG);
    let width = galley.size().x + 36.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        theme::RADIUS,
        if hovered {
            theme::fade(theme::INVERT_BG, 0.9)
        } else {
            theme::INVERT_BG
        },
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme::INVERT_FG,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn secondary_button(ui: &mut Ui, icon: Option<Icon>, label: &str, height: f32) -> Response {
    let galley = caps(ui, label, 10.0, theme::mono_bold(), 1.2, theme::INK);
    let icon_w = if icon.is_some() { 13.0 + 8.0 } else { 0.0 };
    let width = galley.size().x + icon_w + 36.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hovered = response.hovered();
    let (fill, stroke, ink) = if hovered {
        (theme::INVERT_BG, Stroke::NONE, theme::INVERT_FG)
    } else {
        (
            Color32::TRANSPARENT,
            Stroke::new(1.0, theme::LINE_STRONG),
            theme::INK,
        )
    };
    ui.painter()
        .rect(rect, theme::RADIUS, fill, stroke, egui::StrokeKind::Inside);
    let content_w = icon_w + galley.size().x;
    let mut x = rect.center().x - content_w / 2.0;
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x + 6.5, rect.center().y), vec2(13.0, 13.0)),
            icon,
            ink,
        );
        x += icon_w;
    }
    ui.painter().galley_with_override_text_color(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn checkbox(ui: &mut Ui, done: &mut bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    let hovered = response.hovered();
    let fill = if *done {
        theme::ACCENT
    } else if hovered {
        theme::WASH_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if *done {
        Stroke::NONE
    } else {
        Stroke::new(
            1.0,
            if hovered {
                theme::INK
            } else {
                theme::LINE_STRONG
            },
        )
    };
    ui.painter()
        .rect(rect, theme::RADIUS, fill, stroke, egui::StrokeKind::Inside);
    if *done {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(11.0, 11.0)),
            Icon::Check,
            Color32::WHITE,
        );
    }
    if response.clicked() {
        *done = !*done;
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn status_pill(ui: &mut Ui, status: &str) -> Response {
    let (label, color) = match status {
        "doing" => ("DOING", theme::ACCENT_INK),
        "done" => ("DONE", theme::FAINT),
        _ => ("TO DO", theme::MUTED),
    };
    let galley = caps(ui, label, 8.0, theme::mono_bold(), 0.9, color);
    let width = 8.0 + 5.0 + 5.0 + galley.size().x + 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        theme::RADIUS,
        if hovered {
            theme::WASH_HOVER
        } else {
            theme::WASH
        },
        Stroke::new(1.0, theme::fade(color, if hovered { 0.5 } else { 0.25 })),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .circle_filled(pos2(rect.left() + 9.0, rect.center().y), 2.0, color);
    ui.painter().galley(
        pos2(rect.left() + 17.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
        .on_hover_text("Task status and actions")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn popover_frame() -> Frame {
    Frame::new()
        .fill(theme::CONTROLS)
        .stroke(theme::hairline())
        .corner_radius(theme::RADIUS)
        .shadow(theme::shadow())
        .inner_margin(Margin::same(4))
}

pub fn menu_header(ui: &mut Ui, text: &str) {
    let galley = caps(
        ui,
        text,
        theme::HUD_SIZE,
        theme::mono_bold(),
        theme::LABEL_SPACING,
        theme::fade(theme::INK, 0.5),
    );
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().galley(
        pos2(rect.left() + 7.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::INK,
    );
}

pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, theme::hairline());
}

#[allow(clippy::too_many_arguments)]
pub fn menu_row(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    selected: bool,
    enabled: bool,
    danger: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 30.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH_HOVER);
    }
    let ink = if !enabled {
        theme::FAINT
    } else if danger {
        theme::BAD
    } else {
        theme::INK
    };
    let mut x = rect.left() + 8.0;
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0)),
            icon,
            ink,
        );
        x += 22.0;
    }
    ui.painter().text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(12.5, theme::medium()),
        ink,
    );
    if let Some(shortcut) = shortcut {
        ui.painter().text(
            pos2(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::new(9.0, theme::mono()),
            theme::FAINT,
        );
    }
    if selected {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(12.0, 12.0)),
            Icon::Check,
            theme::INK,
        );
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn info_popover(ui: &mut Ui, capsule: &Capsule) {
    ui.set_width(342.0);
    ui.add_space(2.0);
    menu_header(ui, "Page info");
    info_row(ui, "Message-ID", &capsule.message_id, true);
    info_row(ui, "File", &capsule.file_name, true);
    info_row(ui, "Date", &format_date_long(capsule), false);
    info_row(ui, "From", &capsule.from, false);
    info_row(ui, "To", &capsule.to, false);
    info_row(
        ui,
        "Format",
        if capsule.native {
            "CapsuleCraft canvas · writable"
        } else {
            "Imported email · read-only"
        },
        false,
    );
    if !capsule.tags.is_empty() {
        info_row(ui, "Tags", &capsule.tags.join(" · "), false);
    }
    if capsule.attachments.is_empty() {
        info_row(ui, "Attachments", "None", false);
    } else {
        info_row(
            ui,
            "Attachments",
            &format!("{} files", capsule.attachments.len()),
            false,
        );
        for attachment in &capsule.attachments {
            ui.label(
                RichText::new(format!(
                    "{}  {}",
                    attachment.name,
                    human_bytes(attachment.size_bytes)
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::MUTED),
            );
        }
    }
    ui.add_space(6.0);
}

pub fn info_row(ui: &mut Ui, label: &str, value: &str, mono: bool) {
    ui.add_space(8.0);
    let _ = hud(ui, label, theme::FAINT);
    ui.add_space(3.0);
    let rich = if mono {
        RichText::new(value)
            .family(theme::mono())
            .size(10.5)
            .color(theme::INK)
    } else {
        RichText::new(value).size(11.5).color(theme::INK)
    };
    ui.add(Label::new(rich).wrap().selectable(true));
}
