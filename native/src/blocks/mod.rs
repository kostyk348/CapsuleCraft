//! Block system and renderers for Capsule notes.

#![allow(unused_imports)]

pub mod audio;
pub mod callout;
pub mod canvas;
pub mod code;
pub mod grid;
pub mod opponent;
pub mod todo;

pub use audio::draw_audio_block;
pub use callout::draw_callout_block;
pub use canvas::{canvas_element, draw_canvas_block};
pub use code::draw_code_block;
pub use grid::{draw_grid_block, parse_csv, to_csv};
pub use opponent::draw_opponent_block;
pub use todo::draw_todo_block;

use egui::{
    pos2, vec2, Align, FontFamily, FontId, Frame, Label, Layout, Margin, Rect, RichText, Sense,
    TextEdit, Ui,
};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::links;
use crate::mime::{block_data_text, block_type, new_block_id, set_block_text, Attachment};
use crate::theme;
use crate::{ghost, hud, menu_header, menu_row, menu_separator, popover_frame, GUTTER};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockOp {
    Up,
    Down,
    Delete,
    SetLevel(u8),
    TransformTo(BlockKind),
    InsertBelow(BlockKind, String),
    SplitParagraph {
        before: String,
        target_kind: BlockKind,
        target_level: Option<u8>,
        target_text: String,
        after: String,
    },
    /// A block produced a brand-new MIME attachment (picked file or mic
    /// recording); the document view pushes it into `capsule.attachments` —
    /// block renderers only get an immutable slice, so they ask instead.
    AttachAudio {
        name: String,
        cid: String,
        mime: String,
        b64: String,
        size: usize,
    },
}

pub fn transform_block(block: &mut Value, target: BlockKind, locale: Locale) {
    let mut text = block_data_text(block);
    if text.is_empty() {
        if let Some(content) = block
            .get("data")
            .and_then(|d| d.get("content"))
            .and_then(Value::as_str)
        {
            text = content.to_string();
        } else if let Some(code) = block
            .get("data")
            .and_then(|d| d.get("code"))
            .and_then(Value::as_str)
        {
            text = code.to_string();
        } else if let Some(items) = block
            .get("data")
            .and_then(|d| d.get("items"))
            .and_then(Value::as_array)
        {
            let item_texts: Vec<&str> = items
                .iter()
                .filter_map(|i| i.get("text").and_then(Value::as_str))
                .collect();
            text = item_texts.join("\n");
        }
    }
    let id = block
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("blk")
        .to_string();
    *block = match target {
        BlockKind::Text => json!({
            "id": id,
            "type": "paragraph",
            "data": { "text": text }
        }),
        BlockKind::Heading => json!({
            "id": id,
            "type": "heading",
            "data": { "level": 2, "text": text }
        }),
        BlockKind::Tasks => json!({
            "id": id,
            "type": "todo_list",
            "data": {
                "title": "",
                "items": if text.is_empty() {
                    vec![]
                } else {
                    vec![json!({
                        "id": "task_0",
                        "text": text,
                        "done": false,
                        "status": "todo"
                    })]
                }
            }
        }),
        BlockKind::Callout => json!({
            "id": id,
            "type": "callout",
            "data": {
                "title": t(locale, Key::BlockCallout),
                "content": text,
                "tone": "emerald"
            }
        }),
        BlockKind::Code => json!({
            "id": id,
            "type": "code_repl",
            "data": {
                "language": "text",
                "code": text
            }
        }),
        _ => return,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Heading,
    Tasks,
    Callout,
    Code,
    Grid,
    Audio,
    Canvas,
    Opponent,
}

impl BlockKind {
    pub fn block(self, locale: Locale) -> Value {
        match self {
            BlockKind::Text => json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": "" }
            }),
            BlockKind::Heading => json!({
                "id": new_block_id("heading"),
                "type": "heading",
                "data": { "level": 2, "text": "" }
            }),
            BlockKind::Tasks => json!({
                "id": new_block_id("todo_list"),
                "type": "todo_list",
                "data": { "title": t(locale, Key::BlockTodo), "items": [] }
            }),
            BlockKind::Callout => json!({
                "id": new_block_id("callout"),
                "type": "callout",
                "data": {
                    "title": t(locale, Key::BlockCallout),
                    "content": "",
                    "icon": "info",
                    "tone": "blue"
                }
            }),
            BlockKind::Code => json!({
                "id": new_block_id("code_repl"),
                "type": "code_repl",
                "data": { "language": "javascript", "code": "" }
            }),
            BlockKind::Grid => json!({
                "id": new_block_id("data_grid_view"),
                "type": "data_grid_view",
                "data": { "source_cid": "", "view": "table" }
            }),
            BlockKind::Audio => json!({
                "id": new_block_id("audio_note"),
                "type": "audio_note",
                "data": {
                    "duration": "0:00",
                    "transcript": "",
                    "waveform": [4, 9, 14, 7, 17, 11, 5, 13, 8, 16, 6, 12],
                    "author": t(locale, Key::NativeBadge).to_lowercase()
                }
            }),
            BlockKind::Canvas => json!({
                "id": new_block_id("interactive_canvas"),
                "type": "interactive_canvas",
                "data": { "scene_cid": "", "elements": [] }
            }),
            BlockKind::Opponent => json!({
                "id": new_block_id("ai_opponent_block"),
                "type": "ai_opponent_block",
                "data": { "criticism": "", "sourceClaim": "", "confidence": 0.5 }
            }),
        }
    }
}

pub fn add_block_menu(ui: &mut Ui, locale: Locale) -> Option<BlockKind> {
    let response = ghost(
        ui,
        Icon::Plus,
        Some(t(locale, Key::AddBlock)),
        t(locale, Key::AddBlockHint),
        true,
        30.0,
    );
    let mut chosen = None;
    egui::Popup::menu(&response)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(200.0);
            menu_header(ui, t(locale, Key::AddBlock));
            let items = [
                (Icon::Doc, Key::BlockParagraph, BlockKind::Text, None),
                (Icon::Doc, Key::BlockHeading, BlockKind::Heading, Some("H2")),
                (Icon::Check, Key::BlockTodo, BlockKind::Tasks, None),
                (Icon::Info, Key::BlockCallout, BlockKind::Callout, None),
                (Icon::Code, Key::BlockCode, BlockKind::Code, None),
                (Icon::Table, Key::BlockGrid, BlockKind::Grid, None),
                (Icon::Audio, Key::BlockAudio, BlockKind::Audio, None),
                (Icon::Board, Key::BlockCanvas, BlockKind::Canvas, None),
                (Icon::Command, Key::BlockAi, BlockKind::Opponent, None),
            ];
            for (icon, key, kind, hint) in items {
                if menu_row(ui, Some(icon), t(locale, key), hint, false, true, false).clicked() {
                    chosen = Some(kind);
                }
            }
        });
    chosen
}

pub fn draw_block_row(
    ui: &mut Ui,
    block: &mut Value,
    index: usize,
    block_count: usize,
    attachments: &mut [Attachment],
    locale: Locale,
) -> (bool, Option<BlockOp>) {
    let kind = block_type(block).to_string();
    let mut changed = false;
    let mut op = None;
    let row_top = ui.cursor().min;

    let response = ui
        .allocate_ui_with_layout(
            vec2(ui.available_width(), 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(GUTTER);
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width());
                        changed =
                            draw_block_content(ui, block, &kind, attachments, locale, &mut op);
                    });
                });
            },
        )
        .response;

    let row_rect = response.rect;
    let hovered = ui.rect_contains_pointer(row_rect);
    let handle_rect = Rect::from_min_size(pos2(row_top.x + 1.0, row_top.y + 3.0), vec2(20.0, 18.0));
    let handle = ui
        .interact(
            handle_rect,
            ui.scope_id().with(("block-menu", index)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(t(locale, Key::AddBlockHint));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(handle_rect.center(), vec2(13.0, 13.0)),
        Icon::Grip,
        if hovered || handle.hovered() {
            theme::MUTED
        } else {
            theme::fade(theme::FAINT, 0.45)
        },
    );

    egui::Popup::menu(&handle)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(178.0);
            if kind == "heading" {
                let level = block
                    .get("data")
                    .and_then(|data| data.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(1, 3);
                menu_header(ui, t(locale, Key::HeadingLevel));
                for candidate in 1..=3 {
                    if menu_row(
                        ui,
                        None,
                        &format!("H{candidate}"),
                        None,
                        level == candidate as u64,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        op = Some(BlockOp::SetLevel(candidate));
                    }
                }
                menu_separator(ui);
            } else {
                menu_header(ui, &kind.replace('_', " "));
            }

            // Turn into...
            menu_header(ui, t(locale, Key::TurnInto));
            if menu_row(
                ui,
                Some(Icon::Doc),
                t(locale, Key::BlockParagraph),
                None,
                false,
                kind != "paragraph",
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::TransformTo(BlockKind::Text));
            }
            if menu_row(
                ui,
                Some(Icon::Command),
                t(locale, Key::BlockHeading),
                None,
                false,
                kind != "heading",
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::TransformTo(BlockKind::Heading));
            }
            if menu_row(
                ui,
                Some(Icon::Check),
                t(locale, Key::BlockTodo),
                None,
                false,
                kind != "todo_list",
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::TransformTo(BlockKind::Tasks));
            }
            if menu_row(
                ui,
                Some(Icon::Info),
                t(locale, Key::BlockCallout),
                None,
                false,
                kind != "callout",
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::TransformTo(BlockKind::Callout));
            }
            if menu_row(
                ui,
                Some(Icon::Code),
                t(locale, Key::BlockCode),
                None,
                false,
                kind != "code_repl",
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::TransformTo(BlockKind::Code));
            }
            menu_separator(ui);

            if menu_row(
                ui,
                Some(Icon::ArrowUp),
                t(locale, Key::MoveUp),
                None,
                false,
                index > 0,
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::Up);
            }
            if menu_row(
                ui,
                Some(Icon::ArrowDown),
                t(locale, Key::MoveDown),
                None,
                false,
                index + 1 < block_count,
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::Down);
            }
            menu_separator(ui);
            if menu_row(
                ui,
                Some(Icon::Trash),
                t(locale, Key::DeleteBlock),
                None,
                false,
                true,
                true,
            )
            .clicked()
            {
                op = Some(BlockOp::Delete);
            }
        });

    (changed, op)
}

pub fn draw_block_content(
    ui: &mut Ui,
    block: &mut Value,
    kind: &str,
    attachments: &mut [Attachment],
    locale: Locale,
    op: &mut Option<BlockOp>,
) -> bool {
    match kind {
        "heading" => {
            let level = block
                .get("data")
                .and_then(|data| data.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 3);
            let size = match level {
                1 => 26.0,
                2 => 21.0,
                _ => 17.0,
            };
            let mut text = block_data_text(block);
            let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                links::layout(ui, buffer.as_str(), size, wrap)
            };
            let output = TextEdit::multiline(&mut text)
                .font(FontId::new(size, theme::display()))
                .text_color(theme::INK)
                .frame(Frame::NONE)
                .margin(Margin::same(0))
                .desired_width(f32::INFINITY)
                .desired_rows(1)
                .layouter(&mut layouter)
                .hint_text(t(locale, Key::PlaceholderHeading))
                .show(ui);
            let response = output.response.response;

            // Enter key split: text before newline stays in heading, text after becomes new paragraph below
            if text.contains('\n') || text.contains('\r') {
                let mut lines = text.splitn(2, |c| c == '\n' || c == '\r');
                let first = lines.next().unwrap_or("").to_string();
                let rest = lines.next().unwrap_or("").to_string();
                set_block_text(block, first);
                *op = Some(BlockOp::InsertBelow(BlockKind::Text, rest));
                return true;
            }

            let has_focus = response.has_focus();
            let sel_key = response.id.with("saved-selection");

            if has_focus {
                if let Some(cursor_range) = output.cursor_range {
                    let p_idx = cursor_range.primary.index;
                    let s_idx = cursor_range.secondary.index;
                    if p_idx != s_idx {
                        let c_min: usize = p_idx.min(s_idx).into();
                        let c_max: usize = p_idx.max(s_idx).into();
                        let (b_start, b_end) =
                            crate::rich_text::char_range_to_byte_range(&text, c_min, c_max);
                        if b_start < b_end && b_end <= text.len() {
                            let r1 = output.galley.pos_from_cursor(cursor_range.primary);
                            let r2 = output.galley.pos_from_cursor(cursor_range.secondary);
                            let sel_rect = r1.union(r2).translate(output.galley_pos.to_vec2());
                            ui.data_mut(|d| {
                                d.insert_temp(
                                    sel_key,
                                    crate::rich_text::SavedSelection::new(sel_rect, b_start, b_end),
                                );
                            });
                        }
                    } else {
                        ui.data_mut(|d| d.remove_temp::<crate::rich_text::SavedSelection>(sel_key));
                    }
                }
            }

            let saved_selection =
                ui.data(|d| d.get_temp::<crate::rich_text::SavedSelection>(sel_key));
            let bubble_id = response.id.with("floating-bubble-menu");
            let bubble_rect = crate::rich_text::toolbar_rect(ui, bubble_id);

            if has_focus {
                if let Some(sel) = saved_selection {
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
                        let (s, e) = crate::rich_text::toggle_wrap(
                            &mut text,
                            sel.b_start,
                            sel.b_end,
                            "**",
                            "**",
                        );
                        set_block_text(block, text);
                        ui.data_mut(|d| {
                            d.insert_temp(
                                sel_key,
                                crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                            )
                        });
                        return true;
                    }
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I)) {
                        let (s, e) = crate::rich_text::toggle_wrap(
                            &mut text,
                            sel.b_start,
                            sel.b_end,
                            "_",
                            "_",
                        );
                        set_block_text(block, text);
                        ui.data_mut(|d| {
                            d.insert_temp(
                                sel_key,
                                crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                            )
                        });
                        return true;
                    }
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
                        let (s, e) = crate::rich_text::toggle_wrap(
                            &mut text,
                            sel.b_start,
                            sel.b_end,
                            "[[",
                            "]]",
                        );
                        set_block_text(block, text);
                        ui.data_mut(|d| {
                            d.insert_temp(
                                sel_key,
                                crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                            )
                        });
                        return true;
                    }
                }
            }

            if let Some(sel) = saved_selection {
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                let hovering_bubble =
                    pointer_pos.map_or(false, |p| bubble_rect.map_or(false, |r| r.contains(p)));

                if has_focus || hovering_bubble {
                    if let Some(bubble_action) =
                        crate::rich_text::draw_bubble_toolbar(ui, bubble_id, sel.rect(), locale)
                    {
                        match bubble_action {
                            crate::rich_text::BubbleAction::ToggleBold => {
                                let (s, e) = crate::rich_text::toggle_wrap(
                                    &mut text,
                                    sel.b_start,
                                    sel.b_end,
                                    "**",
                                    "**",
                                );
                                set_block_text(block, text);
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        sel_key,
                                        crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                                    )
                                });
                                return true;
                            }
                            crate::rich_text::BubbleAction::ToggleItalic => {
                                let (s, e) = crate::rich_text::toggle_wrap(
                                    &mut text,
                                    sel.b_start,
                                    sel.b_end,
                                    "_",
                                    "_",
                                );
                                set_block_text(block, text);
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        sel_key,
                                        crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                                    )
                                });
                                return true;
                            }
                            crate::rich_text::BubbleAction::ToggleWikiLink => {
                                let (s, e) = crate::rich_text::toggle_wrap(
                                    &mut text,
                                    sel.b_start,
                                    sel.b_end,
                                    "[[",
                                    "]]",
                                );
                                set_block_text(block, text);
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        sel_key,
                                        crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                                    )
                                });
                                return true;
                            }
                            crate::rich_text::BubbleAction::TransformBlock(t_name) => {
                                let target = match t_name {
                                    "heading_1" => BlockKind::Heading,
                                    "heading_2" => BlockKind::Heading,
                                    "heading_3" => BlockKind::Heading,
                                    "todo_list" => BlockKind::Tasks,
                                    "callout" => BlockKind::Callout,
                                    "code_repl" => BlockKind::Code,
                                    _ => BlockKind::Text,
                                };
                                let target_level = match t_name {
                                    "heading_1" => Some(1),
                                    "heading_2" => Some(2),
                                    "heading_3" => Some(3),
                                    _ => None,
                                };
                                *op = Some(BlockOp::TransformTo(target));
                                if let Some(lvl) = target_level {
                                    block["data"]["level"] = json!(lvl);
                                }
                                ui.data_mut(|d| {
                                    d.remove_temp::<crate::rich_text::SavedSelection>(sel_key)
                                });
                                return true;
                            }
                        }
                    }
                }
            }

            if response.changed() {
                set_block_text(block, text);
                true
            } else {
                false
            }
        }
        "paragraph" => {
            let mut text = block_data_text(block);
            let rows = text.lines().count().max(1);
            let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                links::layout(ui, buffer.as_str(), 15.5, wrap)
            };
            let output = TextEdit::multiline(&mut text)
                .font(FontId::new(15.5, FontFamily::Proportional))
                .text_color(theme::INK)
                .frame(Frame::NONE)
                .margin(Margin::same(0))
                .desired_width(f32::INFINITY)
                .desired_rows(rows)
                .layouter(&mut layouter)
                .hint_text(t(locale, Key::PlaceholderParagraph))
                .show(ui);
            let response = output.response.response;

            // Smart markdown prefix transform check:
            if let Some(transform) = crate::rich_text::detect_smart_transform(&text) {
                match transform {
                    crate::rich_text::SmartTransform::Heading(level, rest) => {
                        block["type"] = json!("heading");
                        block["data"] = json!({ "level": level, "text": rest });
                        return true;
                    }
                    crate::rich_text::SmartTransform::Tasks(rest) => {
                        block["type"] = json!("todo_list");
                        block["data"] = json!({
                            "title": "",
                            "items": [
                                { "id": crate::mime::new_block_id("task"), "text": rest, "done": false, "status": "todo" }
                            ]
                        });
                        return true;
                    }
                    crate::rich_text::SmartTransform::Callout(rest) => {
                        block["type"] = json!("callout");
                        block["data"] = json!({
                            "title": t(locale, Key::BlockCallout),
                            "content": rest,
                            "tone": "emerald"
                        });
                        return true;
                    }
                    crate::rich_text::SmartTransform::Code(rest) => {
                        block["type"] = json!("code_repl");
                        block["data"] = json!({ "language": "text", "code": rest });
                        return true;
                    }
                }
            }

            let has_focus = response.has_focus();
            let sel_key = response.id.with("saved-selection");

            if has_focus {
                if let Some(cursor_range) = output.cursor_range {
                    let p_idx = cursor_range.primary.index;
                    let s_idx = cursor_range.secondary.index;
                    if p_idx != s_idx {
                        let c_min: usize = p_idx.min(s_idx).into();
                        let c_max: usize = p_idx.max(s_idx).into();
                        let (b_start, b_end) =
                            crate::rich_text::char_range_to_byte_range(&text, c_min, c_max);
                        if b_start < b_end && b_end <= text.len() {
                            let r1 = output.galley.pos_from_cursor(cursor_range.primary);
                            let r2 = output.galley.pos_from_cursor(cursor_range.secondary);
                            let sel_rect = r1.union(r2).translate(output.galley_pos.to_vec2());
                            ui.data_mut(|d| {
                                d.insert_temp(
                                    sel_key,
                                    crate::rich_text::SavedSelection::new(sel_rect, b_start, b_end),
                                );
                            });
                        }
                    } else {
                        ui.data_mut(|d| d.remove_temp::<crate::rich_text::SavedSelection>(sel_key));
                    }
                }
            }

            let saved_selection =
                ui.data(|d| d.get_temp::<crate::rich_text::SavedSelection>(sel_key));
            let bubble_id = response.id.with("floating-bubble-menu");
            let bubble_rect = crate::rich_text::toolbar_rect(ui, bubble_id);

            if has_focus {
                if let Some(sel) = saved_selection {
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
                        let (s, e) = crate::rich_text::toggle_wrap(
                            &mut text,
                            sel.b_start,
                            sel.b_end,
                            "**",
                            "**",
                        );
                        set_block_text(block, text);
                        ui.data_mut(|d| {
                            d.insert_temp(
                                sel_key,
                                crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                            )
                        });
                        return true;
                    }
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I)) {
                        let (s, e) = crate::rich_text::toggle_wrap(
                            &mut text,
                            sel.b_start,
                            sel.b_end,
                            "_",
                            "_",
                        );
                        set_block_text(block, text);
                        ui.data_mut(|d| {
                            d.insert_temp(
                                sel_key,
                                crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                            )
                        });
                        return true;
                    }
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
                        let (s, e) = crate::rich_text::toggle_wrap(
                            &mut text,
                            sel.b_start,
                            sel.b_end,
                            "[[",
                            "]]",
                        );
                        set_block_text(block, text);
                        ui.data_mut(|d| {
                            d.insert_temp(
                                sel_key,
                                crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                            )
                        });
                        return true;
                    }
                }
            }

            if let Some(sel) = saved_selection {
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                let hovering_bubble =
                    pointer_pos.map_or(false, |p| bubble_rect.map_or(false, |r| r.contains(p)));

                if has_focus || hovering_bubble {
                    if let Some(bubble_action) =
                        crate::rich_text::draw_bubble_toolbar(ui, bubble_id, sel.rect(), locale)
                    {
                        match bubble_action {
                            crate::rich_text::BubbleAction::ToggleBold => {
                                let (s, e) = crate::rich_text::toggle_wrap(
                                    &mut text,
                                    sel.b_start,
                                    sel.b_end,
                                    "**",
                                    "**",
                                );
                                set_block_text(block, text);
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        sel_key,
                                        crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                                    )
                                });
                                return true;
                            }
                            crate::rich_text::BubbleAction::ToggleItalic => {
                                let (s, e) = crate::rich_text::toggle_wrap(
                                    &mut text,
                                    sel.b_start,
                                    sel.b_end,
                                    "_",
                                    "_",
                                );
                                set_block_text(block, text);
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        sel_key,
                                        crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                                    )
                                });
                                return true;
                            }
                            crate::rich_text::BubbleAction::ToggleWikiLink => {
                                let (s, e) = crate::rich_text::toggle_wrap(
                                    &mut text,
                                    sel.b_start,
                                    sel.b_end,
                                    "[[",
                                    "]]",
                                );
                                set_block_text(block, text);
                                ui.data_mut(|d| {
                                    d.insert_temp(
                                        sel_key,
                                        crate::rich_text::SavedSelection::new(sel.rect(), s, e),
                                    )
                                });
                                return true;
                            }
                            crate::rich_text::BubbleAction::TransformBlock(t_name) => {
                                let target = match t_name {
                                    "heading_1" => BlockKind::Heading,
                                    "heading_2" => BlockKind::Heading,
                                    "heading_3" => BlockKind::Heading,
                                    "todo_list" => BlockKind::Tasks,
                                    "callout" => BlockKind::Callout,
                                    "code_repl" => BlockKind::Code,
                                    _ => BlockKind::Text,
                                };
                                let target_level = match t_name {
                                    "heading_1" => Some(1),
                                    "heading_2" => Some(2),
                                    "heading_3" => Some(3),
                                    _ => None,
                                };

                                let before = text[..sel.b_start].trim_end().to_string();
                                let target_text = text[sel.b_start..sel.b_end].trim().to_string();
                                let after = text[sel.b_end..].trim_start().to_string();

                                if before.is_empty() && after.is_empty() {
                                    *op = Some(BlockOp::TransformTo(target));
                                    if let Some(lvl) = target_level {
                                        block["data"]["level"] = json!(lvl);
                                    }
                                } else {
                                    *op = Some(BlockOp::SplitParagraph {
                                        before,
                                        target_kind: target,
                                        target_level,
                                        target_text,
                                        after,
                                    });
                                }
                                ui.data_mut(|d| {
                                    d.remove_temp::<crate::rich_text::SavedSelection>(sel_key)
                                });
                                return true;
                            }
                        }
                    }
                }
            }

            if response.changed() {
                set_block_text(block, text);
                true
            } else {
                false
            }
        }
        "todo_list" => draw_todo_block(ui, block, locale),
        "callout" => draw_callout_block(ui, block, locale),
        "code_repl" => draw_code_block(ui, block, locale),
        "data_grid_view" => draw_grid_block(ui, block, attachments, locale),
        "audio_note" => draw_audio_block(ui, block, attachments, locale, op),
        "interactive_canvas" => draw_canvas_block(ui, block, locale),
        "ai_opponent_block" => draw_opponent_block(ui, block, locale),
        _ => {
            let inner = Frame::new()
                .fill(theme::SUNKEN)
                .corner_radius(theme::RADIUS)
                .stroke(theme::hairline())
                .inner_margin(Margin {
                    left: 14,
                    right: 14,
                    top: 11,
                    bottom: 11,
                })
                .show(ui, |ui| {
                    let _ = hud(
                        ui,
                        &format!(
                            "{} · {}",
                            t(locale, Key::BlockPreserved),
                            kind.replace('_', " ")
                        ),
                        theme::FAINT,
                    );
                    ui.add_space(6.0);
                    ui.add(
                        Label::new(
                            RichText::new(t(locale, Key::BlockPreservedHint))
                                .size(11.5)
                                .color(theme::MUTED),
                        )
                        .wrap(),
                    );
                });
            let _ = inner;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_kind_generation() {
        let block = BlockKind::Text.block(Locale::En);
        assert_eq!(block["type"], "paragraph");
        assert_eq!(block["data"]["text"], "");

        let heading = BlockKind::Heading.block(Locale::En);
        assert_eq!(heading["type"], "heading");
        assert_eq!(heading["data"]["level"], 2);

        let tasks = BlockKind::Tasks.block(Locale::En);
        assert_eq!(tasks["type"], "todo_list");
    }

    #[test]
    fn test_transform_block() {
        let mut block = json!({
            "id": "blk-123",
            "type": "paragraph",
            "data": { "text": "Original text" }
        });

        // 1. Transform paragraph -> heading
        transform_block(&mut block, BlockKind::Heading, Locale::En);
        assert_eq!(block["type"], "heading");
        assert_eq!(block["data"]["text"], "Original text");
        assert_eq!(block["data"]["level"], 2);
        assert_eq!(block["id"], "blk-123");

        // 2. Transform heading -> todo_list
        transform_block(&mut block, BlockKind::Tasks, Locale::En);
        assert_eq!(block["type"], "todo_list");
        assert_eq!(block["data"]["items"][0]["text"], "Original text");

        // 3. Transform todo_list -> paragraph
        transform_block(&mut block, BlockKind::Text, Locale::En);
        assert_eq!(block["type"], "paragraph");
        assert_eq!(block["data"]["text"], "Original text");
    }
}
