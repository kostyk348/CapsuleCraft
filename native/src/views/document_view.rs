use base64::Engine;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontFamily, FontId, Frame, Label, Layout, Margin, Rect,
    Response, RichText, ScrollArea, Sense, StrokeKind, TextEdit, Ui, UiBuilder,
};
use serde_json::json;

use crate::blocks::{add_block_menu, draw_block_row, BlockKind, BlockOp};
use crate::caps;
use crate::crumb_button;
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
use crate::primary_button;
use crate::secondary_button;
use crate::status_pill;
use crate::tag_chip;
use crate::text_button;
use crate::theme;
use crate::truncate_chars;
use crate::views::View;
use crate::widgets::draw_breadcrumbs;

#[derive(Default)]
pub struct EditorAction {
    pub changed: bool,
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
                            .text_color(theme::INK)
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
                            .color(theme::INK),
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
                    ui.add_space(70.0);
                } else {
                    let mut block_op: Option<(usize, BlockOp)> = None;
                    let block_count = capsule.blocks.len();
                    for index in 0..block_count {
                        let mut block = capsule.blocks[index].clone();
                        let kind = block_type(&block).to_string();

                        // Eliminate duplicate title: if block 0 is an H1 with identical subject text, skip rendering it.
                        if index == 0 && is_duplicate_heading(&block, &capsule.subject) {
                            continue;
                        }

                        let (changed, op) = draw_block_row(
                            ui,
                            &mut block,
                            index,
                            block_count,
                            &mut capsule.attachments,
                            locale,
                        );
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
                                if index < capsule.blocks.len() {
                                    capsule.blocks.remove(index);
                                    action.changed = true;
                                }
                            }
                            BlockOp::Up if index > 0 => {
                                capsule.blocks.swap(index, index - 1);
                                action.changed = true;
                            }
                            BlockOp::Down if index + 1 < capsule.blocks.len() => {
                                capsule.blocks.swap(index, index + 1);
                                action.changed = true;
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
                                crate::mime::set_block_text(&mut new_b, text);
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

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(2.0);
                        if let Some(kind) = add_block_menu(ui, locale) {
                            capsule.blocks.push(kind.block(locale));
                            action.changed = true;
                        }
                        ui.add_space(4.0);
                        let _ = hud(ui, t(locale, Key::SlashHint), theme::FAINT);
                    });
                    ui.add_space(18.0);
                    links_panel(ui, link_context, locale, &mut action);
                    ui.add_space(18.0);
                    if attachments_editor(ui, capsule, locale) {
                        action.changed = true;
                    }
                    ui.add_space(90.0);
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
            let _ = hud(ui, t(locale, Key::NoLinks), theme::FAINT);
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
                    theme::FAINT,
                );
                let (rect, _) = ui.allocate_exact_size(
                    vec2(label.size().x, label.size().y + 4.0),
                    Sense::hover(),
                );
                ui.painter()
                    .galley(rect.min + vec2(0.0, 2.0), label, theme::FAINT);
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
                theme::FAINT,
            );
            let (rect, _) =
                ui.allocate_exact_size(vec2(label.size().x, label.size().y + 4.0), Sense::hover());
            ui.painter()
                .galley(rect.min + vec2(0.0, 2.0), label, theme::FAINT);
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
            theme::MUTED
        } else {
            theme::ACCENT_INK
        },
    );
    let size = galley.size() + vec2(18.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        theme::WASH_HOVER
    } else {
        theme::WASH
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
            theme::MUTED
        } else {
            theme::ACCENT_INK
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
                .color(theme::FAINT),
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
                .color(theme::FAINT),
            );
        } else {
            ui.label(
                RichText::new(t(locale, Key::ImportedEmail))
                    .family(theme::mono())
                    .size(9.5)
                    .color(theme::BADGE_BETA),
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
            let chip = tag_chip(ui, tag, theme::MUTED);
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
                .color(theme::FAINT),
            );
        }
    });
    changed
}

pub fn read_only_notice(ui: &mut Ui, capsule: &Capsule, locale: Locale) {
    let inner = Frame::new()
        .fill(theme::WASH)
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
                theme::BADGE_BETA,
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
                    .color(theme::INK),
                )
                .wrap(),
            );
            ui.add_space(5.0);
            ui.add(
                Label::new(
                    RichText::new(t(locale, Key::HtmlRenderingNotice))
                        .size(11.5)
                        .color(theme::MUTED),
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
        theme::BADGE_BETA,
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
                        .color(theme::INK),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(t(locale, Key::EmptyVaultBody))
                        .size(13.0)
                        .color(theme::MUTED),
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
                        .color(theme::FAINT),
                );
            });
        });
}

// Blocks extracted to crate::blocks
/// Attachment list for one page. Files are stored base64-encoded in the MIME
/// part, exactly like the web app does, so the .eml stays self-contained.
pub fn attachments_editor(ui: &mut Ui, capsule: &mut Capsule, locale: Locale) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let _ = hud(ui, t(locale, Key::AttachmentsTitle), theme::FAINT);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if text_button(ui, Some(Icon::Plus), t(locale, Key::AddAttachment), false).clicked() {
                if let Some(paths) = rfd::FileDialog::new()
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
        let _ = hud(ui, t(locale, Key::EmptyAttachments), theme::FAINT);
        return changed;
    }

    let mut remove: Option<usize> = None;
    for (index, attachment) in capsule.attachments.iter().enumerate() {
        ui.horizontal(|ui| {
            icons::paint(
                ui.painter(),
                Rect::from_min_size(ui.cursor().min + vec2(0.0, 3.0), vec2(12.0, 12.0)),
                Icon::Doc,
                theme::FAINT,
            );
            ui.add_space(18.0);
            ui.label(
                RichText::new(truncate_chars(&attachment.name, 48))
                    .size(12.0)
                    .color(theme::MUTED),
            );
            ui.add_space(6.0);
            let _ = hud(ui, &human_bytes(attachment.size_bytes), theme::FAINT);
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
            });
        });
    }
    if let Some(index) = remove {
        capsule.attachments.remove(index);
        changed = true;
    }
    changed
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
