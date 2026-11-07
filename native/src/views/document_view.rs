use base64::Engine;
use egui::{
    pos2, vec2, Align, CursorIcon, FontFamily, FontId, Frame, Label, Layout, Margin, Rect,
    Response, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui,
};
use serde_json::{json, Value};
use std::path::Path;

use crate::blocks::{add_block_menu, draw_block_row, BlockKind, BlockOp};
use crate::caps;
use crate::dot_sep;
use crate::format_date;
use crate::hud;
use crate::human_bytes;
use crate::i18n::{self, t, Key, Locale};
use crate::icon_button;
use crate::icons::{self, Icon};
use crate::is_duplicate_heading;
use crate::links;
use crate::menu_header;
use crate::menu_row;
use crate::mime::{block_type, Attachment, Capsule};
use crate::popover_frame;
use crate::preview::{self, Preview, PreviewCache};
use crate::primary_button;
use crate::secondary_button;
use crate::status_pill;
use crate::tag_chip;
use crate::text_button;
use crate::theme;
use crate::truncate_chars;
use crate::views::View;
use crate::widgets::draw_breadcrumbs;

/// An action a user performed on one attachment row.
#[derive(Debug)]
pub enum AttachmentAction {
    /// Open the built-in image/PDF viewer.
    View {
        name: String,
        mime: String,
        bytes: Vec<u8>,
    },
    /// Send an image through the OCR engine.
    Ocr { name: String, bytes: Vec<u8> },
}

#[derive(Default)]
pub struct EditorAction {
    pub changed: bool,
    /// Block index the user asked to delete — the shell confirms first.
    pub delete_block: Option<usize>,
    pub select: Option<usize>,
    pub new_subpage: bool,
    pub new_page: bool,
    pub open_vault: bool,
    pub faq: bool,
    pub cothink: bool,
    pub view: Option<View>,
    /// Title of a page that a [[link]] points at but which does not exist.
    pub create_page: Option<String>,
    /// Previous subject of the page being renamed, so links can follow it.
    pub rename_from: Option<String>,
    pub toggle_wide: bool,
    /// Attachment rows acted on this frame.
    pub attachments: Vec<AttachmentAction>,
    /// "Озвучить страницу" was pressed.
    pub speak: bool,
    /// Transcribe the audio attachment with this cid.
    pub transcribe: Option<String>,
}

impl EditorAction {
    pub fn absorb(&mut self, other: EditorAction) {
        self.changed |= other.changed;
        self.select = other.select.or(self.select);
        self.new_subpage |= other.new_subpage;
        self.new_page |= other.new_page;
        self.open_vault |= other.open_vault;
        self.faq |= other.faq;
        self.cothink |= other.cothink;
        self.view = other.view.or(self.view);
        self.create_page = other.create_page.or_else(|| self.create_page.take());
        self.rename_from = other.rename_from.or_else(|| self.rename_from.take());
        self.toggle_wide |= other.toggle_wide;
        self.attachments.extend(other.attachments);
        self.speak |= other.speak;
        self.transcribe = other.transcribe.or_else(|| self.transcribe.take());
    }
}

/// What the document knows about links before it borrows the page mutably.
#[derive(Default)]
pub struct LinkContext {
    /// (title as written, resolved page index) for every outgoing link.
    pub outgoing: Vec<(String, Option<usize>)>,
    /// Pages that link here: (index, subject).
    pub backlinks: Vec<(usize, String)>,
    /// Closest pages by meaning: (index, subject, similarity 0..1).
    pub related: Vec<(usize, String, f32)>,
}
pub fn draw_document(
    ui: &mut Ui,
    capsule: &mut Capsule,
    ancestors: &[(usize, String)],
    locale: Locale,
    link_context: &LinkContext,
    scroll: Option<f32>,
    wide_canvas: bool,
    previews: &mut PreviewCache,
    pdfium: Option<&Path>,
    // Id of the block to scroll to and stroke — the paragraph OCR just
    // appended, so "куда оно добавилось?" is answered visually.
    highlight: Option<&str>,
) -> EditorAction {
    let mut action = EditorAction::default();
    let mut area = ScrollArea::vertical().id_salt("document");
    if let Some(offset) = scroll {
        area = area.vertical_scroll_offset(offset);
    }
    area.auto_shrink([false, false]).show(ui, |ui| {
        let available = ui.available_width();
        let (content_w, pad) = if wide_canvas {
            let w = (available - 96.0).max(480.0);
            let p = ((available - w) / 2.0).max(48.0);
            (w, p)
        } else {
            let w = (available - 96.0).clamp(480.0, 960.0);
            let p = ((available - w) / 2.0).max(28.0);
            (w, p)
        };
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.allocate_ui_with_layout(vec2(content_w, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(content_w);
                ui.add_space(24.0);

                draw_breadcrumbs(ui, ancestors, locale, &mut action);
                ui.add_space(12.0);

                if capsule.native {
                    // Strip newlines if pasted or typed, ensuring subject remains single-concept
                    if capsule.subject.contains('\n') || capsule.subject.contains('\r') {
                        capsule.subject = capsule.subject.replace(['\r', '\n'], " ");
                        action.changed = true;
                    }
                    let response = ui.add(
                        TextEdit::multiline(&mut capsule.subject)
                            .font(FontId::new(34.0, theme::display()))
                            .text_color(theme::ink())
                            .frame(Frame::NONE)
                            .margin(Margin::same(0))
                            .desired_width(content_w)
                            .desired_rows(1)
                            .hint_text(t(locale, Key::UntitledPage)),
                    );
                    if response.changed() {
                        if capsule.subject.contains('\n') || capsule.subject.contains('\r') {
                            capsule.subject = capsule.subject.replace(['\r', '\n'], " ");
                        }
                        action.changed = true;
                    }
                    // A rename is committed when the title loses focus,
                    // not on every keystroke: rewriting every `[[link]]`
                    // in the vault mid-word would be destructive.
                    let title_id =
                        egui::Id::new(("subject-before-edit", capsule.message_id.clone()));
                    if response.gained_focus() {
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(title_id, capsule.subject.clone()));
                    }
                    if response.lost_focus() {
                        let before = ui
                            .ctx()
                            .data_mut(|data| data.remove_temp::<String>(title_id));
                        if let Some(before) = before {
                            if !before.trim().is_empty()
                                && links::normalise(&before) != links::normalise(&capsule.subject)
                            {
                                action.rename_from = Some(before);
                            }
                        }
                    }
                } else {
                    ui.add(
                        Label::new(
                            RichText::new(if capsule.subject.is_empty() {
                                t(locale, Key::Untitled)
                            } else {
                                &capsule.subject
                            })
                            .family(theme::display())
                            .size(34.0)
                            .color(theme::ink()),
                        )
                        .wrap(),
                    );
                }

                ui.add_space(12.0);
                if document_meta(ui, capsule, locale) {
                    action.changed = true;
                }
                ui.add_space(28.0);

                if !capsule.native {
                    read_only_notice(ui, capsule, locale);
                    ui.add_space(18.0);
                    if secondary_button(ui, Some(Icon::Mail), t(locale, Key::NavPreview), 30.0)
                        .clicked()
                    {
                        action.view = Some(View::Mail);
                    }
                    ui.add_space(18.0);
                    if !capsule.attachments.is_empty() {
                        super::section(ui, t(locale, Key::AttachmentsTitle));
                        // Imported emails show their documents too: images and
                        // PDF first pages right on the page, then a read-only
                        // row list with view / OCR / save.
                        inline_previews(ui, capsule, previews, pdfium, locale, &mut action);
                        ui.add_space(10.0);
                        let actions = attachments_readonly(ui, capsule, locale);
                        action.attachments.extend(actions);
                    }
                    ui.add_space(70.0);
                } else {
                    // --- Page mode: «Страница | Канвас» ---
                    // A per-page view toggle kept in egui state. Canvas mode
                    // replaces the block list with one full-page canvas
                    // stored in the capsule as page-canvas.json.
                    let mode_id = egui::Id::new(("page-canvas-mode", capsule.message_id.clone()));
                    let mut canvas_mode = ui
                        .ctx()
                        .data_mut(|data| data.get_temp::<bool>(mode_id).unwrap_or(false));
                    ui.horizontal(|ui| {
                        let _ = crate::hud(ui, t(locale, Key::PageModePage), theme::faint());
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            let page_active = !canvas_mode;
                            let canvas_clicked = if canvas_mode {
                                crate::primary_button(ui, t(locale, Key::PageModeCanvas), 24.0)
                                    .clicked()
                            } else {
                                crate::secondary_button(
                                    ui,
                                    None,
                                    t(locale, Key::PageModeCanvas),
                                    24.0,
                                )
                                .clicked()
                            };
                            let page_clicked = if page_active {
                                crate::primary_button(ui, t(locale, Key::PageModePage), 24.0)
                                    .clicked()
                            } else {
                                crate::secondary_button(
                                    ui,
                                    None,
                                    t(locale, Key::PageModePage),
                                    24.0,
                                )
                                .clicked()
                            };
                            if canvas_clicked && !canvas_mode {
                                canvas_mode = true;
                                ui.ctx().data_mut(|data| data.insert_temp(mode_id, true));
                            }
                            if page_clicked && canvas_mode {
                                canvas_mode = false;
                                ui.ctx().data_mut(|data| data.insert_temp(mode_id, false));
                            }
                        });
                    });
                    ui.add_space(10.0);

                    if canvas_mode {
                        let mut data = capsule.page_canvas.clone().unwrap_or_else(
                            || serde_json::json!({ "elements": [], "snap": false }),
                        );
                        // Full-page: fill the visible viewport height.
                        if let Some(obj) = data.as_object_mut() {
                            obj.insert(
                                "height".to_string(),
                                serde_json::json!(ui.available_height().max(480.0)),
                            );
                        }
                        let mut canvas_op: Option<crate::blocks::BlockOp> = None;
                        if crate::blocks::draw_canvas_block(
                            ui,
                            &mut data,
                            &capsule.attachments,
                            locale,
                            &mut canvas_op,
                        ) {
                            capsule.page_canvas = Some(data.clone());
                            action.changed = true;
                        }
                        // The page canvas can ask for the same things a block
                        // row can: a new image attachment or the viewer.
                        match canvas_op {
                            Some(crate::blocks::BlockOp::AttachImage {
                                name,
                                cid,
                                mime,
                                b64,
                                size,
                            }) => {
                                let attachment = crate::mime::Attachment {
                                    name,
                                    cid: cid.clone(),
                                    mime_type: mime,
                                    content: String::new(),
                                    size_bytes: size,
                                    raw_b64: Some(b64),
                                };
                                match capsule
                                    .attachments
                                    .iter_mut()
                                    .find(|existing| existing.cid == cid)
                                {
                                    Some(existing) => *existing = attachment,
                                    None => capsule.attachments.push(attachment),
                                }
                                action.changed = true;
                            }
                            Some(crate::blocks::BlockOp::OpenViewer(cid)) => {
                                if let Some(attachment) =
                                    capsule.attachments.iter().find(|a| a.cid == cid)
                                {
                                    if let Some(bytes) = attachment_bytes(attachment) {
                                        action.attachments.push(AttachmentAction::View {
                                            name: attachment.name.clone(),
                                            mime: attachment.mime_type.clone(),
                                            bytes,
                                        });
                                    }
                                }
                            }
                            _ => {}
                        }
                        ui.add_space(24.0);
                    } else {
                        let mut block_op: Option<(usize, BlockOp)> = None;
                        let block_count = capsule.blocks.len();
                        let first_row_top = ui.cursor().top();
                        for index in 0..block_count {
                            let mut block = capsule.blocks[index].clone();
                            let kind = block_type(&block).to_string();

                            // Eliminate duplicate title: if block 0 is an H1 with identical subject text, skip rendering it.
                            if index == 0 && is_duplicate_heading(&block, &capsule.subject) {
                                continue;
                            }

                            let block_id = block.get("id").and_then(Value::as_str).unwrap_or("");
                            let highlighted = highlight == Some(block_id);
                            let highlight_top = ui.cursor().top();
                            let (changed, op) = draw_block_row(
                                ui,
                                &mut block,
                                index,
                                block_count,
                                &mut capsule.attachments,
                                locale,
                            );
                            if highlighted {
                                // Scroll the fresh OCR paragraph into view and
                                // stroke it — the block announces itself instead
                                // of quietly appearing somewhere at the bottom.
                                let rect = Rect::from_min_max(
                                    pos2(ui.min_rect().left(), highlight_top),
                                    pos2(ui.min_rect().right(), ui.min_rect().bottom()),
                                );
                                ui.scroll_to_rect(rect, Some(Align::Center));
                                ui.painter().rect_stroke(
                                    rect.expand(5.0),
                                    6.0,
                                    Stroke::new(1.5, theme::accent()),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if changed {
                                capsule.blocks[index] = block;
                                action.changed = true;
                            }
                            if op.is_some() {
                                block_op = op.map(|op| (index, op));
                            }
                            ui.add_space(match kind.as_str() {
                                "heading" => 20.0,
                                "todo_list" => 16.0,
                                "callout" => 16.0,
                                "unsupported" => 12.0,
                                _ => 10.0,
                            });
                        }

                        if let Some((index, op)) = block_op {
                            match op {
                                BlockOp::Delete => {
                                    // One stray click must not vaporise content:
                                    // the shell shows the confirm modal first.
                                    action.delete_block = Some(index);
                                }
                                BlockOp::Up if index > 0 => {
                                    capsule.blocks.swap(index, index - 1);
                                    action.changed = true;
                                }
                                BlockOp::Down if index + 1 < capsule.blocks.len() => {
                                    capsule.blocks.swap(index, index + 1);
                                    action.changed = true;
                                }
                                BlockOp::Duplicate => {
                                    // A copy lands right below with a fresh id so
                                    // links, todos and code state stay independent.
                                    if let Some(block) = capsule.blocks.get(index) {
                                        let mut copy = block.clone();
                                        if let Some(obj) = copy.as_object_mut() {
                                            obj.insert(
                                                "id".to_string(),
                                                json!(crate::mime::new_block_id("paragraph")),
                                            );
                                        }
                                        capsule.blocks.insert(index + 1, copy);
                                        action.changed = true;
                                    }
                                }
                                BlockOp::Reorder { from, to } => {
                                    if let Some(insert) = crate::blocks::reorder_insert_index(
                                        from,
                                        to,
                                        capsule.blocks.len(),
                                    ) {
                                        let moved = capsule.blocks.remove(from);
                                        capsule.blocks.insert(insert, moved);
                                        action.changed = true;
                                    }
                                }
                                BlockOp::SetLevel(level) => {
                                    if let Some(block) = capsule.blocks.get_mut(index) {
                                        block["data"]["level"] = json!(level);
                                        action.changed = true;
                                    }
                                }
                                BlockOp::TransformTo(target) => {
                                    if let Some(block) = capsule.blocks.get_mut(index) {
                                        crate::blocks::transform_block(block, target, locale);
                                        action.changed = true;
                                    }
                                }
                                BlockOp::InsertBelow(kind, text) => {
                                    let mut new_b = kind.block(locale);
                                    if !text.is_empty() {
                                        crate::mime::set_block_text(&mut new_b, text);
                                    }
                                    if index + 1 <= capsule.blocks.len() {
                                        capsule.blocks.insert(index + 1, new_b);
                                    } else {
                                        capsule.blocks.push(new_b);
                                    }
                                    action.changed = true;
                                }
                                BlockOp::AttachAudio {
                                    name,
                                    cid,
                                    mime,
                                    b64,
                                    size,
                                } => {
                                    capsule.attachments.push(crate::mime::Attachment {
                                        name,
                                        cid,
                                        mime_type: mime,
                                        content: String::new(),
                                        size_bytes: size,
                                        raw_b64: Some(b64),
                                    });
                                    action.changed = true;
                                }
                                BlockOp::AttachImage {
                                    name,
                                    cid,
                                    mime,
                                    b64,
                                    size,
                                } => {
                                    // Same cid means "replace these bytes": picking
                                    // the same picture twice must not duplicate the
                                    // attachment.
                                    let attachment = crate::mime::Attachment {
                                        name,
                                        cid: cid.clone(),
                                        mime_type: mime,
                                        content: String::new(),
                                        size_bytes: size,
                                        raw_b64: Some(b64),
                                    };
                                    match capsule
                                        .attachments
                                        .iter_mut()
                                        .find(|existing| existing.cid == cid)
                                    {
                                        Some(existing) => *existing = attachment,
                                        None => capsule.attachments.push(attachment),
                                    }
                                    action.changed = true;
                                }
                                BlockOp::OpenViewer(cid) => {
                                    if let Some(attachment) =
                                        capsule.attachments.iter().find(|a| a.cid == cid)
                                    {
                                        if let Some(bytes) = attachment_bytes(attachment) {
                                            action.attachments.push(AttachmentAction::View {
                                                name: attachment.name.clone(),
                                                mime: attachment.mime_type.clone(),
                                                bytes,
                                            });
                                        }
                                    }
                                }
                                BlockOp::RequestTranscribe(cid) => {
                                    action.transcribe = Some(cid);
                                }
                                BlockOp::SplitParagraph {
                                    before,
                                    target_kind,
                                    target_level,
                                    target_text,
                                    after,
                                } => {
                                    let mut new_blocks = Vec::new();
                                    if !before.is_empty() {
                                        let mut b_before = BlockKind::Text.block(locale);
                                        crate::mime::set_block_text(&mut b_before, before);
                                        new_blocks.push(b_before);
                                    }
                                    let mut b_target = target_kind.block(locale);
                                    crate::mime::set_block_text(&mut b_target, target_text);
                                    if let Some(lvl) = target_level {
                                        b_target["data"]["level"] = json!(lvl);
                                    }
                                    new_blocks.push(b_target);
                                    if !after.is_empty() {
                                        let mut b_after = BlockKind::Text.block(locale);
                                        crate::mime::set_block_text(&mut b_after, after);
                                        new_blocks.push(b_after);
                                    }
                                    capsule.blocks.splice(index..=index, new_blocks);
                                    action.changed = true;
                                }
                                _ => {}
                            }
                        }

                        // While a grip drag is live, a pointer lifted above the
                        // first rendered row (over the title) means "drop at the
                        // very top" — gap 0, unreachable by row hit-testing when
                        // block 0 is skipped as a duplicate heading.
                        crate::blocks::nudge_drag_to_top(ui, first_row_top);

                        // Alt+↑ / Alt+↓ moves the block last hovered. Independent
                        // of the grip, the popup and the pointer, so reordering
                        // stays possible even when a drag release is eaten.
                        if let Some((hovered_index, up)) =
                            crate::blocks::keyboard_move_request(ui, block_count)
                        {
                            let target = if up {
                                hovered_index.checked_sub(1)
                            } else if hovered_index + 1 < block_count {
                                Some(hovered_index + 1)
                            } else {
                                None
                            };
                            if let Some(target) = target {
                                capsule.blocks.swap(hovered_index, target);
                                crate::blocks::set_last_hovered_block(ui, target);
                                action.changed = true;
                            }
                        }
                    }
                    if !canvas_mode {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_space(2.0);
                            if let Some(kind) = add_block_menu(ui, locale) {
                                capsule.blocks.push(kind.block(locale));
                                action.changed = true;
                            }
                            ui.add_space(4.0);
                            let _ = hud(ui, t(locale, Key::SlashHint), theme::faint());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(
                                    ui,
                                    Icon::Speaker,
                                    t(locale, Key::SpeakPage),
                                    true,
                                    false,
                                )
                                .clicked()
                                {
                                    action.speak = true;
                                }
                            });
                        });
                        ui.add_space(18.0);
                        links_panel(ui, link_context, locale, &mut action);
                        ui.add_space(18.0);
                        // Inline previews above the management rows: the page
                        // shows its attached documents, not just their names.
                        if inline_previews(ui, capsule, previews, pdfium, locale, &mut action) {
                            ui.add_space(14.0);
                        }
                        let (changed, attachment_actions) = attachments_editor(ui, capsule, locale);
                        if changed {
                            action.changed = true;
                        }
                        action.attachments.extend(attachment_actions);
                        ui.add_space(90.0);
                    }
                }
            });
        });
    });
    action
}

/// Outgoing `[[links]]` and the pages that link back, drawn under the blocks.
///
/// A link the user clicks either navigates or — when no page has that subject
/// yet — offers to create it, so the wiki can grow from prose alone.
pub fn links_panel(ui: &mut Ui, context: &LinkContext, locale: Locale, action: &mut EditorAction) {
    ui.vertical(|ui| {
        ui.set_max_width(ui.available_width());
        super::section(ui, t(locale, Key::LinksTitle));
        if context.outgoing.is_empty() {
            let _ = hud(ui, t(locale, Key::NoLinks), theme::faint());
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (title, resolved) in &context.outgoing {
                    let missing = resolved.is_none();
                    let response = link_chip(ui, title, missing);
                    let response = if missing {
                        response.on_hover_text(t(locale, Key::CreateLinkedPage))
                    } else {
                        response.on_hover_text(t(locale, Key::OpenLinkedPage))
                    };
                    if response.clicked() {
                        match resolved {
                            Some(index) => action.select = Some(*index),
                            None => action.create_page = Some(title.clone()),
                        }
                    }
                }
            });
        }
        if !context.backlinks.is_empty() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let label = caps(
                    ui,
                    t(locale, Key::Backlinks),
                    theme::HUD_SIZE,
                    theme::mono_bold(),
                    theme::LABEL_SPACING,
                    theme::faint(),
                );
                let (rect, _) = ui.allocate_exact_size(
                    vec2(label.size().x, label.size().y + 4.0),
                    Sense::hover(),
                );
                ui.painter()
                    .galley(rect.min + vec2(0.0, 2.0), label, theme::faint());
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (index, subject) in &context.backlinks {
                    let name = if subject.is_empty() {
                        t(locale, Key::Untitled).to_string()
                    } else {
                        subject.clone()
                    };
                    if link_chip(ui, &name, false)
                        .on_hover_text(t(locale, Key::OpenLinkedPage))
                        .clicked()
                    {
                        action.select = Some(*index);
                    }
                }
            });
        }
        if !context.related.is_empty() {
            ui.add_space(10.0);
            let label = caps(
                ui,
                t(locale, Key::Related),
                theme::HUD_SIZE,
                theme::mono_bold(),
                theme::LABEL_SPACING,
                theme::faint(),
            );
            let (rect, _) =
                ui.allocate_exact_size(vec2(label.size().x, label.size().y + 4.0), Sense::hover());
            ui.painter()
                .galley(rect.min + vec2(0.0, 2.0), label, theme::faint());
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (index, subject, score) in &context.related {
                    let name = if subject.is_empty() {
                        t(locale, Key::Untitled).to_string()
                    } else {
                        subject.clone()
                    };
                    let label = format!("{} · {:.0}%", name, score * 100.0);
                    if link_chip(ui, &label, false)
                        .on_hover_text(t(locale, Key::OpenLinkedPage))
                        .clicked()
                    {
                        action.select = Some(*index);
                    }
                }
            });
        }
    });
}

/// A clickable page chip. Missing targets are drawn muted with a leading `+`.
pub fn link_chip(ui: &mut Ui, label: &str, missing: bool) -> Response {
    let text = if missing {
        format!("+ {label}")
    } else {
        label.to_string()
    };
    let galley = ui.painter().layout_no_wrap(
        text,
        FontId::new(11.5, theme::medium()),
        if missing {
            theme::muted()
        } else {
            theme::accent_ink()
        },
    );
    let size = galley.size() + vec2(18.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        theme::wash_hover()
    } else {
        theme::wash()
    };
    ui.painter().rect(
        rect,
        theme::RADIUS,
        fill,
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley_with_override_text_color(
        rect.min + vec2(9.0, 5.0),
        galley,
        if missing {
            theme::muted()
        } else {
            theme::accent_ink()
        },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn document_meta(ui: &mut Ui, capsule: &mut Capsule, locale: Locale) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(7.0, 6.0);
        ui.label(
            RichText::new(format_date(capsule))
                .family(theme::mono())
                .size(9.5)
                .color(theme::faint()),
        );
        dot_sep(ui);
        if capsule.native {
            ui.label(
                RichText::new(i18n::plural(
                    locale,
                    capsule.blocks.len(),
                    Key::BlockOne,
                    Key::BlockFew,
                    Key::BlockMany,
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::faint()),
            );
            ui.label(
                RichText::new(i18n::plural(
                    locale,
                    links::word_count(capsule),
                    Key::WordOne,
                    Key::WordFew,
                    Key::WordMany,
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::faint()),
            );
        } else {
            ui.label(
                RichText::new(t(locale, Key::ImportedEmail))
                    .family(theme::mono())
                    .size(9.5)
                    .color(theme::badge_beta()),
            );
        }

        // Page status: the same four values the web app writes into the
        // `X-Capsule-Status` header.
        if capsule.native {
            ui.add_space(2.0);
            let pill = status_pill(ui, &capsule.status);
            egui::Popup::menu(&pill)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_START)
                .show(|ui| {
                    ui.set_min_width(170.0);
                    menu_header(ui, t(locale, Key::Status));
                    for (value, key) in [
                        ("draft", Key::StatusDraft),
                        ("active", Key::StatusActive),
                        ("synthesis", Key::StatusSynthesis),
                        ("archived", Key::StatusArchived),
                    ] {
                        if menu_row(
                            ui,
                            None,
                            t(locale, key),
                            None,
                            capsule.status == value,
                            true,
                            false,
                        )
                        .clicked()
                        {
                            capsule.status = value.to_string();
                            changed = true;
                        }
                    }
                });
        }

        let tag_id = egui::Id::new(("tag-input", capsule.message_id.clone()));
        let mut buffer: String = ui.data(|data| data.get_temp(tag_id)).unwrap_or_default();
        let mut remove_tag: Option<usize> = None;
        for (index, tag) in capsule.tags.iter().enumerate() {
            ui.add_space(2.0);
            let chip = tag_chip(ui, tag, theme::muted());
            egui::Popup::menu(&chip)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_START)
                .show(|ui| {
                    ui.set_min_width(150.0);
                    if menu_row(
                        ui,
                        Some(Icon::Trash),
                        t(locale, Key::Remove),
                        Some(tag.as_str()),
                        false,
                        true,
                        true,
                    )
                    .clicked()
                    {
                        remove_tag = Some(index);
                    }
                });
        }
        if let Some(index) = remove_tag {
            capsule.tags.remove(index);
            changed = true;
        }

        if capsule.native {
            ui.add_space(2.0);
            let add = text_button(ui, Some(Icon::Plus), t(locale, Key::AddTag), false);
            egui::Popup::menu(&add)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_START)
                .show(|ui| {
                    ui.set_min_width(190.0);
                    menu_header(ui, t(locale, Key::Tags));
                    let response = ui.add(
                        TextEdit::singleline(&mut buffer)
                            .font(FontId::new(12.0, FontFamily::Proportional))
                            .desired_width(170.0)
                            .hint_text(t(locale, Key::Keyword)),
                    );
                    if response.changed() {
                        ui.data_mut(|data| data.insert_temp(tag_id, buffer.clone()));
                    }
                    let enter = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if enter
                        || menu_row(ui, None, t(locale, Key::AddTag), None, false, true, false)
                            .clicked()
                    {
                        let tag = buffer.trim().to_string();
                        if !tag.is_empty() && !capsule.tags.iter().any(|existing| existing == &tag)
                        {
                            capsule.tags.push(tag);
                            changed = true;
                        }
                        buffer.clear();
                        ui.data_mut(|data| data.insert_temp(tag_id, String::new()));
                    }
                });
        }

        if !capsule.attachments.is_empty() {
            dot_sep(ui);
            let bytes: usize = capsule.attachments.iter().map(|a| a.size_bytes).sum();
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    format!(
                        "{} {}",
                        capsule.attachments.len(),
                        t(locale, Key::Attachment)
                    ),
                    human_bytes(bytes)
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::faint()),
            );
        }
    });
    changed
}

pub fn read_only_notice(ui: &mut Ui, capsule: &Capsule, locale: Locale) {
    let inner = Frame::new()
        .fill(theme::wash())
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            let _ = hud(
                ui,
                t(locale, Key::ExternalReadOnlyNotice),
                theme::badge_beta(),
            );
            ui.add_space(7.0);
            ui.add(
                Label::new(
                    RichText::new(
                        capsule
                            .parse_note
                            .as_deref()
                            .unwrap_or(t(locale, Key::ImportedEmail)),
                    )
                    .size(13.0)
                    .color(theme::ink()),
                )
                .wrap(),
            );
            ui.add_space(5.0);
            ui.add(
                Label::new(
                    RichText::new(t(locale, Key::HtmlRenderingNotice))
                        .size(11.5)
                        .color(theme::muted()),
                )
                .wrap(),
            );
        });
    ui.painter().rect_filled(
        Rect::from_min_size(
            inner.response.rect.min,
            vec2(2.0, inner.response.rect.height()),
        ),
        0.0,
        theme::badge_beta(),
    );
}

pub fn draw_empty_state(ui: &mut Ui, action: &mut EditorAction, locale: Locale) {
    ScrollArea::vertical()
        .id_salt("empty")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(110.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(t(locale, Key::EmptyVaultTitle))
                        .family(theme::display())
                        .size(32.0)
                        .color(theme::ink()),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(t(locale, Key::EmptyVaultBody))
                        .size(13.0)
                        .color(theme::muted()),
                );
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    let total = 420.0;
                    ui.add_space((ui.available_width() - total) / 2.0);
                    if primary_button(ui, t(locale, Key::CreateFirstPage), 34.0).clicked() {
                        action.new_page = true;
                    }
                    ui.add_space(8.0);
                    if secondary_button(ui, Some(Icon::Info), t(locale, Key::Faq), 34.0).clicked() {
                        action.faq = true;
                    }
                    ui.add_space(8.0);
                    if secondary_button(ui, Some(Icon::Folder), t(locale, Key::ChangeFolder), 34.0)
                        .clicked()
                    {
                        action.open_vault = true;
                    }
                });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(t(locale, Key::ZeroDb))
                        .family(theme::mono())
                        .size(9.0)
                        .color(theme::faint()),
                );
            });
        });
}

// Blocks extracted to crate::blocks
/// Attachment list for one page. Files are stored base64-encoded in the MIME
/// part, exactly like the web app does, so the .eml stays self-contained.
/// Image and PDF rows carry view/OCR affordances.
pub fn attachments_editor(
    ui: &mut Ui,
    capsule: &mut Capsule,
    locale: Locale,
) -> (bool, Vec<AttachmentAction>) {
    let mut changed = false;
    let mut actions: Vec<AttachmentAction> = Vec::new();
    ui.horizontal(|ui| {
        let _ = hud(ui, t(locale, Key::AttachmentsTitle), theme::faint());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if text_button(ui, Some(Icon::Plus), t(locale, Key::AddAttachment), false).clicked() {
                if let Some(paths) = crate::util::dialog::FileDialog::new()
                    .set_title(t(locale, Key::AddAttachment))
                    .pick_files()
                {
                    for path in paths {
                        let Ok(bytes) = std::fs::read(&path) else {
                            continue;
                        };
                        let name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("attachment")
                            .to_string();
                        capsule.attachments.push(Attachment {
                            name: name.clone(),
                            cid: format!("{}@capsuleos", sanitize_cid(&name)),
                            mime_type: guess_mime(&name),
                            content: String::from_utf8_lossy(&bytes).to_string(),
                            size_bytes: bytes.len(),
                            raw_b64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
                        });
                        changed = true;
                    }
                }
            }
        });
    });
    ui.add_space(6.0);

    if capsule.attachments.is_empty() {
        let _ = hud(ui, t(locale, Key::EmptyAttachments), theme::faint());
        return (changed, actions);
    }

    let mut remove: Option<usize> = None;
    for (index, attachment) in capsule.attachments.iter().enumerate() {
        let viewable =
            attachment.mime_type.starts_with("image/") || attachment.mime_type == "application/pdf";
        let ocrable =
            attachment.mime_type.starts_with("image/") && attachment.mime_type != "image/svg+xml";
        ui.horizontal(|ui| {
            icons::paint(
                ui.painter(),
                Rect::from_min_size(ui.cursor().min + vec2(0.0, 3.0), vec2(12.0, 12.0)),
                Icon::Doc,
                theme::faint(),
            );
            ui.add_space(18.0);
            ui.label(
                RichText::new(truncate_chars(&attachment.name, 48))
                    .size(12.0)
                    .color(theme::muted()),
            );
            ui.add_space(6.0);
            let _ = hud(ui, &human_bytes(attachment.size_bytes), theme::faint());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(
                    ui,
                    Icon::Trash,
                    t(locale, Key::RemoveAttachment),
                    true,
                    false,
                )
                .clicked()
                {
                    remove = Some(index);
                }
                if ocrable
                    && icon_button(
                        ui,
                        Icon::Sparkle,
                        t(locale, Key::OcrAttachment),
                        true,
                        false,
                    )
                    .clicked()
                {
                    if let Some(bytes) = attachment_bytes(attachment) {
                        actions.push(AttachmentAction::Ocr {
                            name: attachment.name.clone(),
                            bytes,
                        });
                    }
                }
                if viewable
                    && icon_button(ui, Icon::Eye, t(locale, Key::ViewAttachment), true, false)
                        .clicked()
                {
                    if let Some(bytes) = attachment_bytes(attachment) {
                        actions.push(AttachmentAction::View {
                            name: attachment.name.clone(),
                            mime: attachment.mime_type.clone(),
                            bytes,
                        });
                    }
                }
            });
        });
    }
    if let Some(index) = remove {
        capsule.attachments.remove(index);
        changed = true;
    }
    (changed, actions)
}

/// Decode an attachment's base64 payload for the viewer / OCR.
fn attachment_bytes(attachment: &Attachment) -> Option<Vec<u8>> {
    use base64::Engine as _;
    attachment
        .raw_b64
        .as_deref()
        .and_then(|raw| base64::engine::general_purpose::STANDARD.decode(raw).ok())
        .filter(|bytes| !bytes.is_empty())
}

pub(crate) fn sanitize_cid(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

pub(crate) fn guess_mime(name: &str) -> String {
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "csv" => "text/csv",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Inline previews of image/PDF attachments, drawn directly on the page.
///
/// Returns `true` when at least one card was drawn (so callers can add
/// spacing). Images render as pictures; PDFs as their first page with a
/// "PDF · N страниц" caption. A click opens the full-screen viewer.
fn inline_previews(
    ui: &mut Ui,
    capsule: &Capsule,
    previews: &mut PreviewCache,
    pdfium: Option<&Path>,
    locale: Locale,
    action: &mut EditorAction,
) -> bool {
    let indices: Vec<usize> = capsule
        .attachments
        .iter()
        .enumerate()
        .filter(|(_, attachment)| preview::previewable(attachment))
        .map(|(index, _)| index)
        .collect();
    if indices.is_empty() {
        return false;
    }
    let ctx = ui.ctx().clone();
    let mut drawn = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
        for index in indices {
            let Some(attachment) = capsule.attachments.get(index) else {
                continue;
            };
            let key = preview::preview_key(&capsule.message_id, attachment);
            if previews.get(&key).is_none() {
                let entry = match attachment.mime_type.as_str() {
                    "application/pdf" => match (pdfium, attachment_bytes(attachment)) {
                        (Some(lib), Some(bytes)) => {
                            preview::render_pdf_preview(&ctx, &key, &bytes, lib)
                                .unwrap_or_else(Preview::Failed)
                        }
                        (None, _) => Preview::NeedsEngine,
                        (_, None) => Preview::Failed("empty attachment".to_string()),
                    },
                    mime if mime.starts_with("image/") => match attachment_bytes(attachment) {
                        Some(bytes) => preview::decode_image(&ctx, &key, &bytes)
                            .map(Preview::Image)
                            .unwrap_or_else(Preview::Failed),
                        None => Preview::Failed("empty attachment".to_string()),
                    },
                    _ => continue,
                };
                previews.insert(key.clone(), entry);
            }
            let Some(card) = previews.get(&key) else {
                continue;
            };
            let response = draw_preview_card(ui, card, attachment, locale);
            if response.clicked() {
                if let Some(bytes) = attachment_bytes(attachment) {
                    action.attachments.push(AttachmentAction::View {
                        name: attachment.name.clone(),
                        mime: attachment.mime_type.clone(),
                        bytes,
                    });
                }
            }
            drawn = true;
        }
    });
    drawn
}

/// One preview card: the rendered document in a sunken frame, clickable.
fn draw_preview_card(
    ui: &mut Ui,
    card: &Preview,
    attachment: &Attachment,
    locale: Locale,
) -> Response {
    let max_w = ui.available_width().min(560.0);
    let (handle, caption, pages): (&egui::TextureHandle, Option<String>, Option<usize>) = match card
    {
        Preview::Image(handle) => (handle, None, None),
        Preview::Pdf(handle, pages) => (
            handle,
            Some(format!("PDF · {} {}", pages, t(locale, Key::PdfPagesCount))),
            Some(*pages),
        ),
        Preview::NeedsEngine => {
            let response = ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 1.0), vec2(12.0, 12.0)),
                    Icon::Doc,
                    theme::muted(),
                );
                ui.add_space(18.0);
                ui.label(
                    RichText::new(truncate_chars(&attachment.name, 48))
                        .size(12.0)
                        .color(theme::muted()),
                );
                ui.add_space(6.0);
                let _ = hud(ui, t(locale, Key::PdfEngineMissing), theme::faint());
            });
            return response.response;
        }
        Preview::Failed(message) => {
            let response = ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 1.0), vec2(12.0, 12.0)),
                    Icon::Doc,
                    theme::muted(),
                );
                ui.add_space(18.0);
                ui.label(
                    RichText::new(truncate_chars(&attachment.name, 48))
                        .size(12.0)
                        .color(theme::muted()),
                );
                ui.add_space(6.0);
                let _ = hud(ui, message, theme::bad());
            });
            return response.response;
        }
    };
    let natural = handle.size_vec2();
    let card_w = if pages.is_some() {
        // A PDF preview is a document card, not a full-bleed picture.
        (max_w * 0.62).min(340.0)
    } else {
        max_w
    };
    let scale = if natural.x > 0.0 {
        (card_w / natural.x).min(1.0).max(0.05)
    } else {
        1.0
    };
    let size = natural * scale;
    let framed = preview::preview_frame(ui, |ui| {
        let (rect, inner) = ui.allocate_exact_size(size, Sense::click());
        egui::Image::new((handle.id(), size)).paint_at(ui, rect);
        preview::hover_outline(ui, rect, inner.hovered());
        if let Some(caption) = &caption {
            ui.add_space(4.0);
            ui.label(
                RichText::new(caption)
                    .family(theme::mono())
                    .size(9.0)
                    .color(theme::faint()),
            );
        }
        inner.on_hover_cursor(CursorIcon::PointingHand)
    });
    framed.inner
}

/// Read-only attachment rows for imported emails: view / OCR / save, without
/// the add-and-remove affordances of a native page (imported mail never
/// rewrites its source .eml).
fn attachments_readonly(ui: &mut Ui, capsule: &Capsule, locale: Locale) -> Vec<AttachmentAction> {
    let mut actions: Vec<AttachmentAction> = Vec::new();
    for attachment in &capsule.attachments {
        let viewable =
            attachment.mime_type.starts_with("image/") || attachment.mime_type == "application/pdf";
        let ocrable =
            attachment.mime_type.starts_with("image/") && attachment.mime_type != "image/svg+xml";
        ui.horizontal(|ui| {
            icons::paint(
                ui.painter(),
                Rect::from_min_size(ui.cursor().min + vec2(0.0, 3.0), vec2(12.0, 12.0)),
                Icon::Doc,
                theme::faint(),
            );
            ui.add_space(18.0);
            ui.label(
                RichText::new(truncate_chars(&attachment.name, 48))
                    .size(12.0)
                    .color(theme::muted()),
            );
            ui.add_space(6.0);
            let _ = hud(
                ui,
                &format!(
                    "{} · {}",
                    attachment.mime_type,
                    human_bytes(attachment.size_bytes)
                ),
                theme::faint(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, Icon::Download, t(locale, Key::SaveAs), true, false).clicked() {
                    if let Some(bytes) = attachment_bytes(attachment) {
                        if let Some(path) = crate::util::dialog::FileDialog::new()
                            .set_title(t(locale, Key::SaveAs))
                            .set_file_name(&attachment.name)
                            .save_file()
                        {
                            let _ = std::fs::write(path, bytes);
                        }
                    }
                }
                if ocrable
                    && icon_button(
                        ui,
                        Icon::Sparkle,
                        t(locale, Key::OcrAttachment),
                        true,
                        false,
                    )
                    .clicked()
                {
                    if let Some(bytes) = attachment_bytes(attachment) {
                        actions.push(AttachmentAction::Ocr {
                            name: attachment.name.clone(),
                            bytes,
                        });
                    }
                }
                if viewable
                    && icon_button(ui, Icon::Eye, t(locale, Key::ViewAttachment), true, false)
                        .clicked()
                {
                    if let Some(bytes) = attachment_bytes(attachment) {
                        actions.push(AttachmentAction::View {
                            name: attachment.name.clone(),
                            mime: attachment.mime_type.clone(),
                            bytes,
                        });
                    }
                }
            });
        });
        ui.add_space(4.0);
    }
    actions
}
