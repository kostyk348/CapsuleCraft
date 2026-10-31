//! Rich text editing helpers for native egui:
//! - Hotkeys interception (`Ctrl+B`, `Ctrl+I`, `Ctrl+K`)
//! - Floating bubble toolbar for text selections (Notion-style)
//! - Markdown token wrapping & unwrapping with nested style support
//! - Smart transform detection (`# `, `## `, `[] `, `> `)

use egui::{
    pos2, vec2, Area, Color32, CornerRadius, Frame, Margin, Order, Rect, RichText, Stroke, Ui,
};

use crate::i18n::Locale;
use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmartTransform {
    Heading(u8, String),
    Tasks(String),
    Callout(String),
    Code(String),
}

/// Detect if text starts with a markdown shortcut prefix on line 1.
pub fn detect_smart_transform(text: &str) -> Option<SmartTransform> {
    let first_line = text.lines().next().unwrap_or(text);
    let trimmed = first_line.trim_start();
    if trimmed.starts_with("### ") {
        Some(SmartTransform::Heading(3, trimmed[4..].to_string()))
    } else if trimmed.starts_with("## ") {
        Some(SmartTransform::Heading(2, trimmed[3..].to_string()))
    } else if trimmed.starts_with("# ") {
        Some(SmartTransform::Heading(1, trimmed[2..].to_string()))
    } else if trimmed.starts_with("[] ") {
        Some(SmartTransform::Tasks(trimmed[3..].to_string()))
    } else if trimmed.starts_with("- [ ] ") || trimmed.starts_with("* [ ] ") {
        Some(SmartTransform::Tasks(trimmed[6..].to_string()))
    } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("* [x] ") {
        Some(SmartTransform::Tasks(trimmed[6..].to_string()))
    } else if trimmed.starts_with("> ") {
        Some(SmartTransform::Callout(trimmed[2..].to_string()))
    } else if trimmed.starts_with("```") {
        Some(SmartTransform::Code(trimmed[3..].to_string()))
    } else {
        None
    }
}

/// Helper to wrap or unwrap selection in `text` with prefix and suffix tokens.
/// Seamlessly handles nested styles: e.g. toggling `**` inside `**_word_**`
/// cleanly removes the `**` without damaging `_word_`.
/// Returns the new `(start, end)` selection bounds.
pub fn toggle_wrap(
    text: &mut String,
    mut start: usize,
    mut end: usize,
    prefix: &str,
    suffix: &str,
) -> (usize, usize) {
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    // Snap to valid UTF-8 boundaries
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }

    let p_len = prefix.len();
    let s_len = suffix.len();

    // 1. Nested combo unwrap: text is `**_..._**` or `_**...**_`
    if (end - start) >= 6 {
        let sel = &text[start..end];
        if prefix == "**" && suffix == "**" {
            if sel.starts_with("**_") && sel.ends_with("_**") {
                let inner = sel[3..sel.len() - 3].to_string();
                let replacement = format!("_{inner}_");
                text.replace_range(start..end, &replacement);
                return (start, start + replacement.len());
            }
            if sel.starts_with("_**") && sel.ends_with("**_") {
                let inner = sel[3..sel.len() - 3].to_string();
                let replacement = format!("_{inner}_");
                text.replace_range(start..end, &replacement);
                return (start, start + replacement.len());
            }
        } else if prefix == "_" && suffix == "_" {
            if sel.starts_with("**_") && sel.ends_with("_**") {
                let inner = sel[3..sel.len() - 3].to_string();
                let replacement = format!("**{inner}**");
                text.replace_range(start..end, &replacement);
                return (start, start + replacement.len());
            }
            if sel.starts_with("_**") && sel.ends_with("**_") {
                let inner = sel[3..sel.len() - 3].to_string();
                let replacement = format!("**{inner}**");
                text.replace_range(start..end, &replacement);
                return (start, start + replacement.len());
            }
        }
    }

    // 2. Direct match inside selection
    if (end - start) >= (p_len + s_len)
        && text[start..end].starts_with(prefix)
        && text[start..end].ends_with(suffix)
    {
        let inner = text[start + p_len..end - s_len].to_string();
        text.replace_range(start..end, &inner);
        return (start, start + inner.len());
    }

    // 3. Match immediately surrounding selection
    if start >= p_len && (end + s_len) <= text.len() {
        if &text[start - p_len..start] == prefix && &text[end..end + s_len] == suffix {
            text.drain(end..end + s_len);
            text.drain(start - p_len..start);
            return (start - p_len, end - p_len);
        }
    }

    // 4. Wrap selection
    let selected = text[start..end].to_string();
    let replacement = format!("{prefix}{selected}{suffix}");
    text.replace_range(start..end, &replacement);
    (start, start + replacement.len())
}

/// Convert character indices to UTF-8 byte indices in `s`.
pub fn char_range_to_byte_range(s: &str, char_start: usize, char_end: usize) -> (usize, usize) {
    let mut byte_start = s.len();
    let mut byte_end = s.len();
    let mut char_count = 0;

    for (b_idx, _) in s.char_indices() {
        if char_count == char_start {
            byte_start = b_idx;
        }
        if char_count == char_end {
            byte_end = b_idx;
            break;
        }
        char_count += 1;
    }

    if char_start >= char_count {
        byte_start = s.len();
    }
    if char_end >= char_count {
        byte_end = s.len();
    }

    (byte_start.min(byte_end), byte_start.max(byte_end))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SavedSelection {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub b_start: usize,
    pub b_end: usize,
}

impl SavedSelection {
    pub fn new(rect: Rect, b_start: usize, b_end: usize) -> Self {
        Self {
            min_x: rect.min.x,
            min_y: rect.min.y,
            max_x: rect.max.x,
            max_y: rect.max.y,
            b_start,
            b_end,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_min_max(pos2(self.min_x, self.min_y), pos2(self.max_x, self.max_y))
    }
}

pub fn toolbar_rect(ui: &Ui, menu_id: egui::Id) -> Option<Rect> {
    ui.data(|d| d.get_temp(menu_id.with("rect")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BubbleAction {
    ToggleBold,
    ToggleItalic,
    ToggleWikiLink,
    TransformBlock(&'static str),
}

/// Draw a floating Notion-style bubble toolbar above `selection_rect`.
pub fn draw_bubble_toolbar(
    ui: &Ui,
    menu_id: egui::Id,
    selection_rect: Rect,
    _locale: Locale,
) -> Option<BubbleAction> {
    let mut action = None;

    let bubble_pos = pos2(
        selection_rect.center().x - 120.0,
        selection_rect.min.y - 42.0,
    );

    let area_resp = Area::new(menu_id)
        .order(Order::Foreground)
        .fixed_pos(bubble_pos)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(theme::CONTROLS)
                .corner_radius(CornerRadius::same(6))
                .stroke(Stroke::new(1.0, Color32::from_rgb(60, 66, 75)))
                .shadow(theme::shadow())
                .inner_margin(Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
                    let bubble_width = 240.0;
                    ui.set_width(bubble_width);

                    ui.horizontal(|ui| {
                        // Bold button
                        let bold_btn = ui.add(
                            egui::Button::new(
                                RichText::new("B")
                                    .font(egui::FontId::new(13.0, theme::semibold()))
                                    .color(theme::INK),
                            )
                            .min_size(vec2(26.0, 24.0))
                            .fill(Color32::TRANSPARENT),
                        );
                        if bold_btn.clicked() {
                            action = Some(BubbleAction::ToggleBold);
                        }

                        // Italic button
                        let italic_btn = ui.add(
                            egui::Button::new(
                                RichText::new("I")
                                    .italics()
                                    .font(egui::FontId::new(13.0, egui::FontFamily::Proportional))
                                    .color(theme::INK),
                            )
                            .min_size(vec2(26.0, 24.0))
                            .fill(Color32::TRANSPARENT),
                        );
                        if italic_btn.clicked() {
                            action = Some(BubbleAction::ToggleItalic);
                        }

                        // Link button
                        let link_btn = ui.add(
                            egui::Button::new(
                                RichText::new("[[ ]]")
                                    .font(egui::FontId::new(11.0, theme::mono()))
                                    .color(theme::ACCENT_INK),
                            )
                            .min_size(vec2(32.0, 24.0))
                            .fill(Color32::TRANSPARENT),
                        );
                        if link_btn.clicked() {
                            action = Some(BubbleAction::ToggleWikiLink);
                        }

                        ui.add_space(2.0);
                        // Vertical divider
                        let sep_rect = Rect::from_min_size(
                            pos2(ui.cursor().min.x, ui.cursor().min.y + 4.0),
                            vec2(1.0, 16.0),
                        );
                        ui.painter()
                            .rect_filled(sep_rect, 0.0, Color32::from_rgb(60, 66, 75));
                        ui.add_space(3.0);

                        // Dropdown for "Turn into"
                        let dropdown_key = menu_id.with("dropdown-open");
                        let mut dropdown_open: bool =
                            ui.data(|d| d.get_temp(dropdown_key).unwrap_or(false));

                        let transform_btn = ui.add(
                            egui::Button::new(
                                RichText::new(if dropdown_open {
                                    "Type ▲"
                                } else {
                                    "Type ▼"
                                })
                                .font(egui::FontId::new(11.5, theme::medium()))
                                .color(theme::MUTED),
                            )
                            .min_size(vec2(52.0, 24.0))
                            .fill(Color32::TRANSPARENT),
                        );
                        if transform_btn.clicked() {
                            dropdown_open = !dropdown_open;
                            ui.data_mut(|d| d.insert_temp(dropdown_key, dropdown_open));
                        }

                        if dropdown_open {
                            ui.add_space(4.0);
                            ui.painter().line_segment(
                                [
                                    pos2(ui.cursor().min.x - 2.0, ui.cursor().min.y),
                                    pos2(ui.cursor().min.x + bubble_width - 8.0, ui.cursor().min.y),
                                ],
                                Stroke::new(1.0, Color32::from_rgb(48, 54, 61)),
                            );
                            ui.add_space(3.0);

                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                                if menu_item(ui, "H1", "Heading 1") {
                                    action = Some(BubbleAction::TransformBlock("heading_1"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "H2", "Heading 2") {
                                    action = Some(BubbleAction::TransformBlock("heading_2"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "H3", "Heading 3") {
                                    action = Some(BubbleAction::TransformBlock("heading_3"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "[ ]", "To-Do List") {
                                    action = Some(BubbleAction::TransformBlock("todo_list"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "[!]", "Callout Note") {
                                    action = Some(BubbleAction::TransformBlock("callout"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "{}", "Code Snippet") {
                                    action = Some(BubbleAction::TransformBlock("code_repl"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                            });
                        }
                    });
                });
        });

    ui.data_mut(|d| d.insert_temp(menu_id.with("rect"), area_resp.response.rect));

    action
}

fn menu_item(ui: &mut Ui, badge: &str, label: &str) -> bool {
    let resp = ui.add(
        egui::Button::new(
            RichText::new(format!("{badge:<4} {label}"))
                .font(egui::FontId::new(11.5, theme::medium()))
                .color(theme::INK),
        )
        .fill(Color32::TRANSPARENT)
        .min_size(vec2(160.0, 20.0)),
    );
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_transform_detection() {
        assert_eq!(
            detect_smart_transform("# Hello World"),
            Some(SmartTransform::Heading(1, "Hello World".to_string()))
        );
        assert_eq!(
            detect_smart_transform("## Subheading"),
            Some(SmartTransform::Heading(2, "Subheading".to_string()))
        );
        assert_eq!(
            detect_smart_transform("### Third level"),
            Some(SmartTransform::Heading(3, "Third level".to_string()))
        );
        assert_eq!(
            detect_smart_transform("[] New task"),
            Some(SmartTransform::Tasks("New task".to_string()))
        );
        assert_eq!(
            detect_smart_transform("- [ ] Another task"),
            Some(SmartTransform::Tasks("Another task".to_string()))
        );
        assert_eq!(
            detect_smart_transform("> A callout quote"),
            Some(SmartTransform::Callout("A callout quote".to_string()))
        );
        assert_eq!(
            detect_smart_transform("```rust"),
            Some(SmartTransform::Code("rust".to_string()))
        );
        assert_eq!(detect_smart_transform("Plain text without prefix"), None);
    }

    #[test]
    fn test_toggle_wrap_bold() {
        let mut text = "hello world".to_string();
        // Wrap "world"
        let (s, e) = toggle_wrap(&mut text, 6, 11, "**", "**");
        assert_eq!(text, "hello **world**");
        assert_eq!(&text[s..e], "**world**");

        // Unwrap "world"
        let (s2, e2) = toggle_wrap(&mut text, s, e, "**", "**");
        assert_eq!(text, "hello world");
        assert_eq!(&text[s2..e2], "world");
    }

    #[test]
    fn test_toggle_wrap_nested_bold_italic() {
        let mut text = "hello world".to_string();
        // 1. Wrap bold
        let (s, e) = toggle_wrap(&mut text, 6, 11, "**", "**");
        assert_eq!(text, "hello **world**");

        // 2. Wrap italic inside/over bold
        let (s, e) = toggle_wrap(&mut text, s, e, "_", "_");
        assert_eq!(text, "hello _**world**_");

        // 3. Unwrap bold: should strip ** and leave _world_
        let (s, e) = toggle_wrap(&mut text, s, e, "**", "**");
        assert_eq!(text, "hello _world_");
        assert_eq!(&text[s..e], "_world_");

        // 4. Unwrap italic: should strip _ and leave world
        let (s, e) = toggle_wrap(&mut text, s, e, "_", "_");
        assert_eq!(text, "hello world");
        assert_eq!(&text[s..e], "world");
    }

    #[test]
    fn test_toggle_wrap_cyrillic() {
        let mut text = "привет мир".to_string();
        let mir_start = "привет ".len();
        let mir_end = text.len();
        let (s, e) = toggle_wrap(&mut text, mir_start, mir_end, "**", "**");
        assert_eq!(text, "привет **мир**");

        let (s2, e2) = toggle_wrap(&mut text, s, e, "**", "**");
        assert_eq!(text, "привет мир");
        assert_eq!(&text[s2..e2], "мир");
    }
}
