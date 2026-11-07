//! Block system and renderers for Capsule notes.

#![allow(unused_imports)]

pub mod audio;
pub mod callout;
pub mod canvas;
pub mod code;
pub mod grid;
pub mod image;
pub mod opponent;
pub mod todo;

pub use audio::draw_audio_block;
pub use callout::draw_callout_block;
pub use canvas::{canvas_element_at, draw_canvas_block};
pub use code::draw_code_block;
pub use grid::{draw_grid_block, parse_csv, to_csv};
pub use image::draw_image_block;
pub use opponent::draw_opponent_block;
pub use todo::draw_todo_block;

use egui::{
    pos2, vec2, Align, Align2, Area, CursorIcon, FontFamily, FontId, Frame, Label, Layout, Margin,
    Order, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui,
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
    /// Copy of this block inserted right below (hover toolbar / grip menu).
    Duplicate,
    /// Drag-and-drop reorder: the block at `from` moves to the gap `to`
    /// (0..=len) of the *current* list. No-op when it lands where it was.
    Reorder {
        from: usize,
        to: usize,
    },
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
    /// Same, for the image block (`BlockKind::Image`).
    AttachImage {
        name: String,
        cid: String,
        mime: String,
        b64: String,
        size: usize,
    },
    /// An image block asked for the full-screen viewer for this attachment.
    OpenViewer(String),
    /// The audio block asks the shell to transcribe this attachment with the
    /// local whisper engine (or to show the Integrations hint when missing).
    RequestTranscribe(String),
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
    Image,
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
            BlockKind::Image => json!({
                "id": new_block_id("image"),
                "type": "image",
                "data": { "cid": "", "caption": "", "width": 100 }
            }),
            BlockKind::Opponent => json!({
                "id": new_block_id("ai_opponent_block"),
                "type": "ai_opponent_block",
                "data": { "criticism": "", "sourceClaim": "", "confidence": 0.5 }
            }),
        }
    }
}

/// The ordered palette of block kinds offered by the bottom "+" button, the
/// grip menu's insert-below list, the gutter "+" button and the slash menu.
pub fn block_kind_items() -> [(Icon, Key, BlockKind); 10] {
    [
        (Icon::Doc, Key::BlockParagraph, BlockKind::Text),
        (Icon::Doc, Key::BlockHeading, BlockKind::Heading),
        (Icon::Check, Key::BlockTodo, BlockKind::Tasks),
        (Icon::Info, Key::BlockCallout, BlockKind::Callout),
        (Icon::Code, Key::BlockCode, BlockKind::Code),
        (Icon::Table, Key::BlockGrid, BlockKind::Grid),
        (Icon::Audio, Key::BlockAudio, BlockKind::Audio),
        (Icon::Image, Key::BlockImage, BlockKind::Image),
        (Icon::Board, Key::BlockCanvas, BlockKind::Canvas),
        (Icon::Command, Key::BlockAi, BlockKind::Opponent),
    ]
}

/// Draws the block-kind rows inside an already-open popup menu. Returns the
/// kind whose row was clicked this frame.
fn block_kind_rows(ui: &mut Ui, locale: Locale) -> Option<BlockKind> {
    let mut chosen = None;
    for (icon, key, kind) in block_kind_items() {
        let hint = if kind == BlockKind::Heading {
            Some("H2")
        } else {
            None
        };
        if menu_row(ui, Some(icon), t(locale, key), hint, false, true, false).clicked() {
            chosen = Some(kind);
        }
    }
    chosen
}

/// Block kinds whose localized label (or the English one, so `/code` works
/// in the Russian UI too) contains the query typed after the slash.
pub fn slash_matches(query: &str, locale: Locale) -> Vec<BlockKind> {
    let query = query.trim().to_lowercase();
    block_kind_items()
        .into_iter()
        .filter(|(_, key, _)| {
            t(locale, *key).to_lowercase().contains(&query)
                || t(Locale::En, *key).to_lowercase().contains(&query)
        })
        .map(|(_, _, kind)| kind)
        .collect()
}

/// Notion-style "/" menu: a floating palette under the editing paragraph.
/// The caller shows it while the paragraph is focused and its text starts
/// with `/`; the text after the slash filters the palette.
pub fn slash_menu(
    ui: &Ui,
    anchor_id: egui::Id,
    anchor_rect: Rect,
    text: &str,
    locale: Locale,
) -> Option<BlockKind> {
    let query = text.strip_prefix('/').unwrap_or("");
    let matches = slash_matches(query, locale);
    let mut chosen = None;
    Area::new(anchor_id.with("slash-menu"))
        .order(Order::Foreground)
        .fixed_pos(pos2(anchor_rect.left() + 4.0, anchor_rect.bottom() + 6.0))
        .show(ui.ctx(), |ui| {
            ui.set_min_width(230.0);
            ui.set_max_width(260.0);
            popover_frame().show(ui, |ui| {
                menu_header(ui, t(locale, Key::SlashMenuTitle));
                if matches.is_empty() {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                    ui.painter().text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        t(locale, Key::SlashNoMatches),
                        FontId::new(12.0, theme::medium()),
                        theme::faint(),
                    );
                } else {
                    for (icon, key, kind) in block_kind_items() {
                        if !matches.contains(&kind) {
                            continue;
                        }
                        if menu_row(ui, Some(icon), t(locale, key), None, false, true, false)
                            .clicked()
                        {
                            chosen = Some(kind);
                        }
                    }
                }
            });
        });
    chosen
}

/// Pure math behind `BlockOp::Reorder`: the block at `from` moves to the gap
/// `to` (0..=len). Returns the post-removal insert index, or None when the
/// move would change nothing (same gap, one below itself, out of range).
pub fn reorder_insert_index(from: usize, to: usize, len: usize) -> Option<usize> {
    if from >= len || to > len || from == to || to == from + 1 {
        return None;
    }
    Some(if to > from { to - 1 } else { to })
}

/// Live state of a block drag: which row grabbed the grip and where the
/// insertion line currently sits. Kept in egui temp memory because rows are
/// rebuilt from fresh clones every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BlockDrag {
    pub(crate) from: usize,
    pub(crate) to: usize,
}

fn drag_state_id() -> egui::Id {
    egui::Id::new("capsule-block-drag")
}

fn hovered_block_id() -> egui::Id {
    egui::Id::new("capsule-block-hovered")
}

/// Current drag state, if a grip drag is in flight this frame.
pub(crate) fn block_drag_state(ui: &Ui) -> Option<BlockDrag> {
    ui.data(|d| d.get_temp::<BlockDrag>(drag_state_id()))
}

pub(crate) fn set_block_drag_state(ui: &Ui, drag: BlockDrag) {
    ui.data_mut(|d| d.insert_temp(drag_state_id(), drag));
}

/// While a drag is live and the pointer rises above the first rendered row
/// (the title area), the target gap becomes 0 — the very top of the page.
/// Without this the topmost gap is unreachable whenever block 0 is skipped
/// by the duplicate-heading filter, and dragging up always dead-ends.
pub(crate) fn nudge_drag_to_top(ui: &Ui, first_row_top: f32) {
    if let Some(mut drag) = block_drag_state(ui) {
        if let Some(y) = ui.input(|i| i.pointer.hover_pos().map(|p| p.y)) {
            if y < first_row_top && drag.to != 0 {
                drag.to = 0;
                set_block_drag_state(ui, drag);
            }
        }
    }
}

/// Alt+↑ / Alt+↓ moves the block the pointer last hovered — an independent
/// path to reordering that works even when the grip is hidden by a popup or
/// the pointer race eats a release. Notion-style: also fires while a text
/// field keeps focus, because Alt+arrows never type anything.
pub(crate) fn keyboard_move_request(ui: &Ui, block_count: usize) -> Option<(usize, bool)> {
    if ui.input(|i| !i.modifiers.alt) {
        return None;
    }
    let index = ui
        .data(|d| d.get_temp::<usize>(hovered_block_id()))
        .filter(|index| *index < block_count)?;
    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
    if up {
        Some((index, true))
    } else if down {
        Some((index, false))
    } else {
        None
    }
}

/// Remember which block the pointer last hovered so Alt+arrows know what to
/// move. Skipped while a drag is live (the dragged row is hovered by
/// definition and must not override the anchor).
fn remember_hovered_block(ui: &Ui, index: usize, drag_live: bool) {
    if !drag_live {
        ui.data_mut(|d| d.insert_temp(hovered_block_id(), index));
    }
}

pub(crate) fn set_last_hovered_block(ui: &Ui, index: usize) {
    ui.data_mut(|d| d.insert_temp(hovered_block_id(), index));
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
            chosen = block_kind_rows(ui, locale);
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

    // Drag state shared by all rows: set by drag_started on the grip, the
    // insertion gap is updated by whichever row the pointer is over.
    let drag_id = egui::Id::new("capsule-block-drag");

    // The grip is a full-height strip of the gutter, not a 14×18 dot: a
    // target you can find and grab without aiming. The icon stays at the
    // top of the row, where the eye expects it.
    let handle_rect = Rect::from_min_size(
        pos2(row_top.x, row_rect.top()),
        vec2(14.0, row_rect.height().max(20.0)),
    );
    let handle = ui
        .interact(
            handle_rect,
            ui.scope_id().with(("block-menu", index)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::Grab)
        .on_hover_text(t(locale, Key::BlockGripHint));
    if handle.drag_started() {
        ui.data_mut(|d| {
            d.insert_temp(
                drag_id,
                BlockDrag {
                    from: index,
                    to: index + 1,
                },
            )
        });
    }
    let mut drag: Option<BlockDrag> = ui.data(|d| d.get_temp::<BlockDrag>(drag_id));
    remember_hovered_block(ui, index, drag.is_some());
    if let Some(d) = drag.as_mut() {
        let pointer_y = ui.input(|i| i.pointer.hover_pos().map(|p| p.y));
        if index != d.from && ui.rect_contains_pointer(row_rect) {
            if let Some(y) = pointer_y {
                d.to = if y < row_rect.center().y {
                    index
                } else {
                    index + 1
                };
            }
        }
        ui.data_mut(|data| data.insert_temp(drag_id, *d));

        // The insertion line at the current gap, drawn by the row it touches.
        let line_y = if d.to == index && index != d.from {
            Some(row_rect.top())
        } else if d.to == index + 1
            && index != d.from
            && (index + 1 == block_count || pointer_y.map_or(false, |y| y >= row_rect.center().y))
        {
            Some(row_rect.bottom())
        } else {
            None
        };
        if let Some(y) = line_y {
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(row_rect.left(), y - 1.5), vec2(row_rect.width(), 3.0)),
                2.0,
                theme::accent(),
            );
            ui.painter()
                .circle_filled(pos2(row_rect.left() + 3.0, y), 3.5, theme::accent());
        }
        if index == d.from && (handle.drag_stopped() || ui.input(|i| i.pointer.any_released())) {
            // The row that grabbed the grip lands the move on release.
            if reorder_insert_index(d.from, d.to, block_count).is_some() {
                op = Some(BlockOp::Reorder {
                    from: d.from,
                    to: d.to,
                });
            }
            ui.data_mut(|data| data.remove_temp::<BlockDrag>(drag_id));
            drag = None;
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }

    // Stale state only: the landing above had its chance on the release
    // frame; a drag interrupted by a modal or a lost pointer is dropped
    // on the first frame with the button up.
    if drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
        ui.data_mut(|d| d.remove_temp::<BlockDrag>(drag_id));
    }

    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(row_top.x + 7.0, row_rect.top() + 12.0), vec2(12.0, 12.0)),
        Icon::Grip,
        if hovered || handle.hovered() {
            theme::muted()
        } else {
            theme::fade(theme::faint(), 0.45)
        },
    );

    // Hover toolbar at the row's top-right: move up, duplicate, move down —
    // one visible click each, never buried in the grip menu. Notion-style
    // overlay, drawn only when the pointer rests on the row.
    if hovered && drag.is_none() {
        let (btn, gap) = (22.0, 2.0);
        let bar_w = btn * 3.0 + gap * 2.0;
        let bar_rect = Rect::from_min_size(
            pos2(row_rect.right() - bar_w - 4.0, row_rect.top()),
            vec2(bar_w, btn),
        );
        ui.painter()
            .rect_filled(bar_rect, 6.0, theme::chrome());
        ui.painter().rect_stroke(
            bar_rect,
            6.0,
            theme::hairline(),
            egui::StrokeKind::Inside,
        );
        let slots: [(&str, Icon, bool, Key, BlockOp); 3] = [
            (
                "up",
                Icon::ArrowUp,
                index > 0,
                Key::MoveUp,
                BlockOp::Up,
            ),
            ("dup", Icon::Copy, true, Key::DuplicateBlock, BlockOp::Duplicate),
            (
                "down",
                Icon::ArrowDown,
                index + 1 < block_count,
                Key::MoveDown,
                BlockOp::Down,
            ),
        ];
        for (n, (suffix, icon, enabled, key, op_kind)) in slots.iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(
                    bar_rect.left() + n as f32 * (btn + gap) + 1.0,
                    bar_rect.top() + 2.0,
                ),
                vec2(btn - 4.0, btn - 4.0),
            );
            let button = ui
                .interact(
                    rect,
                    egui::Id::new(("block-bar", index, *suffix)),
                    Sense::click(),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(t(locale, *key));
            let color = if !*enabled {
                theme::fade(theme::faint(), 0.7)
            } else if button.hovered() {
                theme::ink()
            } else {
                theme::muted()
            };
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(12.0, 12.0)),
                *icon,
                color,
            );
            if *enabled && button.clicked() {
                op = Some(op_kind.clone());
            }
        }
    }

    // Notion-style "+" in the gutter: appears on hover next to the grip and
    // inserts any block kind directly below this row.
    if hovered && drag.is_none() {
        let plus_rect =
            Rect::from_min_size(pos2(row_top.x + 14.0, row_top.y + 3.0), vec2(12.0, 18.0));
        let plus = ui
            .interact(
                plus_rect,
                egui::Id::new(("block-plus", index)),
                Sense::click(),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(t(locale, Key::InsertBelow));
        icons::paint(
            ui.painter(),
            Rect::from_center_size(plus_rect.center(), vec2(12.0, 12.0)),
            Icon::Plus,
            if plus.hovered() {
                theme::muted()
            } else {
                theme::faint()
            },
        );
        egui::Popup::menu(&plus)
            .frame(popover_frame())
            .align(egui::RectAlign::BOTTOM_START)
            .show(|ui| {
                ui.set_min_width(200.0);
                menu_header(ui, t(locale, Key::InsertBelow));
                if let Some(kind) = block_kind_rows(ui, locale) {
                    op = Some(BlockOp::InsertBelow(kind, String::new()));
                }
            });
    }

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

            // Insert below: the full block palette expands in place, hover
            // driven like a native submenu (a click on the toggle itself
            // would close the whole popup).
            let toggle = menu_row(
                ui,
                Some(Icon::Plus),
                t(locale, Key::InsertBelow),
                Some("▸"),
                false,
                true,
                false,
            );
            let submenu_id = egui::Id::new(("block-insert-below-open", index));
            let mut submenu_open = ui.data(|d| d.get_temp::<bool>(submenu_id)).unwrap_or(false);
            if toggle.hovered() {
                submenu_open = true;
            }
            if submenu_open {
                let mut keep_open = toggle.hovered();
                for (icon, key, kind) in block_kind_items() {
                    let row = menu_row(ui, Some(icon), t(locale, key), None, false, true, false);
                    if row.hovered() {
                        keep_open = true;
                    }
                    if row.clicked() {
                        op = Some(BlockOp::InsertBelow(kind, String::new()));
                    }
                }
                if !keep_open {
                    submenu_open = false;
                }
            }
            ui.data_mut(|d| d.insert_temp(submenu_id, submenu_open));
            menu_separator(ui);

            if menu_row(
                ui,
                Some(Icon::Copy),
                t(locale, Key::DuplicateBlock),
                None,
                false,
                true,
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::Duplicate);
            }
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
                .text_color(theme::ink())
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
                .text_color(theme::ink())
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

            // Slash menu: "/" opens the block palette, filtered by whatever
            // is typed after the slash — the hint under the page keeps its
            // promise now.
            if has_focus && text.starts_with('/') && !text.contains('\n') {
                if let Some(kind) = slash_menu(ui, response.id, response.rect, &text, locale) {
                    set_block_text(block, String::new());
                    *op = Some(BlockOp::TransformTo(kind));
                    return true;
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
        "image" => draw_image_block(ui, block, attachments, locale, op),
        "interactive_canvas" => draw_canvas_block(ui, block, attachments, locale, op),
        "ai_opponent_block" => draw_opponent_block(ui, block, locale),
        _ => {
            let inner = Frame::new()
                .fill(theme::sunken())
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
                        theme::faint(),
                    );
                    ui.add_space(6.0);
                    ui.add(
                        Label::new(
                            RichText::new(t(locale, Key::BlockPreservedHint))
                                .size(11.5)
                                .color(theme::muted()),
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
    fn test_reorder_insert_index() {
        // No-ops: same gap, the gap right below itself, out of range.
        assert_eq!(reorder_insert_index(2, 2, 5), None);
        assert_eq!(reorder_insert_index(2, 3, 5), None);
        assert_eq!(reorder_insert_index(5, 1, 5), None);
        assert_eq!(reorder_insert_index(0, 6, 5), None);
        // Down: "a" to just before "e".
        assert_eq!(reorder_insert_index(0, 4, 5), Some(3));
        // To the very bottom.
        assert_eq!(reorder_insert_index(0, 5, 5), Some(4));
        // Up.
        assert_eq!(reorder_insert_index(4, 1, 5), Some(1));
        assert_eq!(reorder_insert_index(4, 0, 5), Some(0));
    }

    #[test]
    fn test_reorder_applies_like_document_view() {
        let move_block = |v: &mut Vec<&str>, from: usize, to: usize| {
            if let Some(insert) = reorder_insert_index(from, to, v.len()) {
                let moved = v.remove(from);
                v.insert(insert, moved);
            }
        };
        let mut v = vec!["a", "b", "c", "d", "e"];
        move_block(&mut v, 0, 4); // a lands right before e
        assert_eq!(v, vec!["b", "c", "d", "a", "e"]);
        move_block(&mut v, 3, 0); // and back to the top
        assert_eq!(v, vec!["a", "b", "c", "d", "e"]);
        move_block(&mut v, 1, 2); // one below itself: untouched
        assert_eq!(v, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_slash_matches() {
        assert_eq!(slash_matches("", Locale::En).len(), 10);
        let ru = slash_matches("загол", Locale::Ru);
        assert_eq!(ru, vec![BlockKind::Heading]);
        // The English label is a fallback inside the Russian locale too.
        assert!(slash_matches("callout", Locale::Ru).contains(&BlockKind::Callout));
        assert!(slash_matches("врезк", Locale::Ru).contains(&BlockKind::Callout));
        assert!(slash_matches("zzz", Locale::En).is_empty());
    }

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

#[cfg(test)]
mod drag_reorder_tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Drives a real pointer drag — press on block 0's grip, move over
    /// block 2, release — through egui's event pipeline and asserts the
    /// emitted `BlockOp::Reorder` plus the resulting block order.
    /// Regression for the dead drag: the stale-state cleanup used to wipe
    /// the drag state on the release frame, before the landing check ran.
    #[test]
    fn grip_drag_emits_reorder() {
        let ctx = egui::Context::default();
        let blocks = Rc::new(RefCell::new(vec![
            json!({"id":"b0","type":"text","data":{"text":"first"}}),
            json!({"id":"b1","type":"text","data":{"text":"second"}}),
            json!({"id":"b2","type":"text","data":{"text":"third"}}),
        ]));
        let op_slot: Rc<RefCell<Option<BlockOp>>> = Rc::new(RefCell::new(None));
        let vertical_id: Rc<RefCell<Option<egui::Id>>> = Rc::new(RefCell::new(None));

        let run = |events: Vec<egui::Event>| {
            let blocks = blocks.clone();
            let op_slot = op_slot.clone();
            let vertical_id = vertical_id.clone();
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_pass(raw, |ctx| {
                egui::Area::new(egui::Id::new("drag-harness"))
                    .fixed_pos(egui::Pos2::ZERO)
                    .show(ctx, |ui| {
                        ui.vertical(|ui| {
                            *vertical_id.borrow_mut() = Some(ui.id());
                            let count = blocks.borrow().len();
                            for index in 0..count {
                                let mut block = blocks.borrow_mut();
                                let (_, op) = draw_block_row(
                                    ui,
                                    &mut block[index],
                                    index,
                                    count,
                                    &mut [],
                                    Locale::En,
                                );
                                if op.is_some() {
                                    *op_slot.borrow_mut() = op;
                                }
                            }
                        });
                    });
            });
            out.textures_delta.clear();
        };

        // Pass 0: no events — harvest the grip rects.
        run(vec![]);
        let vid = vertical_id.borrow().expect("harness drew rows");
        let handle_id = |i: usize| vid.with(("block-menu", i));
        let rect0 = ctx
            .read_response(handle_id(0))
            .expect("grip 0 response")
            .rect;
        let rect2 = ctx
            .read_response(handle_id(2))
            .expect("grip 2 response")
            .rect;

        // Hover the grip of block 0, press, drag over block 2, release.
        run(vec![egui::Event::PointerMoved(rect0.center())]);
        run(vec![egui::Event::PointerButton {
            pos: rect0.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }]);
        run(vec![egui::Event::PointerMoved(pos2(
            rect2.center().x + 200.0,
            rect2.top() + 4.0,
        ))]);
        run(vec![egui::Event::PointerButton {
            pos: pos2(rect2.center().x + 200.0, rect2.top() + 4.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);

        let op = op_slot.borrow().clone();
        assert!(
            matches!(op, Some(BlockOp::Reorder { from: 0, to: 2 })),
            "expected Reorder{{0,2}}, got {op:?}"
        );
        // Apply the op the way document_view does: 0 -> insert index 1.
        if let Some(BlockOp::Reorder { from, to }) = op {
            if let Some(insert) = reorder_insert_index(from, to, 3) {
                let mut blocks = blocks.borrow_mut();
                let moved = blocks.remove(from);
                blocks.insert(insert, moved);
            }
        }
        let ids: Vec<String> = blocks
            .borrow()
            .iter()
            .map(|b| b["id"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(ids, vec!["b1", "b0", "b2"], "block order after the drag");
    }

    /// The hover toolbar's duplicate button copies the block right below it.
    #[test]
    fn toolbar_duplicate_emits_op() {
        let ctx = egui::Context::default();
        let blocks = Rc::new(RefCell::new(vec![
            json!({"id":"b0","type":"text","data":{"text":"first"}}),
            json!({"id":"b1","type":"text","data":{"text":"second"}}),
        ]));
        let op_slot: Rc<RefCell<Option<BlockOp>>> = Rc::new(RefCell::new(None));
        let vertical_id: Rc<RefCell<Option<egui::Id>>> = Rc::new(RefCell::new(None));

        let run = |events: Vec<egui::Event>| {
            let blocks = blocks.clone();
            let op_slot = op_slot.clone();
            let vertical_id = vertical_id.clone();
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run_pass(raw, |ctx| {
                egui::Area::new(egui::Id::new("dup-harness"))
                    .fixed_pos(egui::Pos2::ZERO)
                    .show(ctx, |ui| {
                        ui.vertical(|ui| {
                            *vertical_id.borrow_mut() = Some(ui.id());
                            let count = blocks.borrow().len();
                            for index in 0..count {
                                let mut block = blocks.borrow_mut();
                                let (_, op) = draw_block_row(
                                    ui,
                                    &mut block[index],
                                    index,
                                    count,
                                    &mut [],
                                    Locale::En,
                                );
                                if op.is_some() {
                                    *op_slot.borrow_mut() = op;
                                }
                            }
                        });
                    });
            })
            .textures_delta
            .clear();
        };

        // Warm-up: egui registers the Area's layer state one pass behind, so
        // hover hit-testing only becomes reliable after a few idle passes.
        run(vec![]);
        run(vec![]);
        run(vec![]);
        // Hover the row so the toolbar renders, then harvest the button rect.
        let vid = vertical_id.borrow().expect("harness drew rows");
        let row0_center = ctx
            .read_response(vid.with(("block-menu", 0usize)))
            .map(|r| r.rect.center())
            .unwrap_or_else(|| pos2(400.0, 40.0));
        run(vec![egui::Event::PointerMoved(row0_center)]);
        run(vec![egui::Event::PointerMoved(row0_center)]);
        eprintln!(
            "DEBUG grip after hover: {:?}",
            ctx.read_response(vid.with(("block-menu", 0usize)))
                .map(|r| (r.hovered(), r.rect))
        );
        let dup_rect = ctx
            .read_response(egui::Id::new(("block-bar", 0usize, "dup")))
            .expect("duplicate button response")
            .rect;

        run(vec![egui::Event::PointerMoved(dup_rect.center())]);
        run(vec![egui::Event::PointerButton {
            pos: dup_rect.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }]);
        run(vec![egui::Event::PointerButton {
            pos: dup_rect.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);

        assert!(
            matches!(op_slot.borrow().as_ref(), Some(BlockOp::Duplicate)),
            "expected Duplicate from the toolbar, got {:?}",
            op_slot.borrow()
        );
    }

    /// Alt+↑/↓ returns a move request for the block the pointer last
    /// hovered — the keyboard path that never depends on the grip.
    #[test]
    fn keyboard_alt_arrows_move_last_hovered() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("kbd-harness")).show(ctx, |ui| {
                remember_hovered_block(ui, 1, false);
                assert_eq!(keyboard_move_request(ui, 3), None, "no key yet");
            });
        })
        .textures_delta
        .clear();
        let alt = egui::Modifiers::ALT;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![
                egui::Event::ModifiersChanged(alt),
                egui::Event::Key {
                    key: egui::Key::ArrowDown,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: alt,
                },
            ],
            ..Default::default()
        };
        ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("kbd-harness")).show(ctx, |ui| {
                assert_eq!(
                    keyboard_move_request(ui, 3),
                    Some((1, false)),
                    "Alt+Down moves hovered block 1"
                );
            });
        })
        .textures_delta
        .clear();
    }

    /// A pointer lifted above the first rendered row while dragging snaps
    /// the target gap to 0 — the top of the page.
    #[test]
    fn nudge_drag_to_top_sets_gap_zero() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![egui::Event::PointerMoved(pos2(200.0, 40.0))],
            ..Default::default()
        };
        ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("nudge-harness")).show(ctx, |ui| {
                set_block_drag_state(
                    ui,
                    BlockDrag {
                        from: 1,
                        to: 2,
                    },
                );
                nudge_drag_to_top(ui, 100.0);
                assert_eq!(
                    block_drag_state(ui),
                    Some(BlockDrag { from: 1, to: 0 }),
                    "pointer above the first row must target gap 0"
                );
            });
        })
        .textures_delta
        .clear();
    }
}
