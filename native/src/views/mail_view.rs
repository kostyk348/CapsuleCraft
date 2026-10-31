//! Email view: one page rendered as a message.
//!
//! Native pages show their blocks read-only in message form; imported emails
//! show the decoded text/plain or text/html part. Either way this view never
//! edits the file — the editor is the only write path, and imported emails are
//! read-only there too.

use egui::{vec2, Align, Color32, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke, Ui};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mail::{self, MailBlock};
use crate::mime::{block_type, Capsule};
use crate::theme;
use crate::{ghost, hud, tag_chip, truncate_chars};

use super::{centred, empty_state, header, ViewAction};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PartMode {
    #[default]
    Html,
    Plain,
}

#[derive(Default)]
pub struct MailState {
    pub part: PartMode,
}

pub fn draw(ui: &mut Ui, capsule: &Capsule, state: &mut MailState, locale: Locale) -> ViewAction {
    let mut action = ViewAction::default();
    header(
        ui,
        t(locale, Key::ExternalPreview),
        &format!(
            "{} · {}",
            if capsule.native {
                t(locale, Key::NavPage)
            } else {
                t(locale, Key::ImportedEmail)
            },
            truncate_chars(&capsule.file_name, 42)
        ),
    );

    let (_, bar_rect) = super::toolbar(ui);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(super::VIEW_PAD, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let has_html = capsule.html_body.is_some();
    let has_plain = capsule.plain_body.is_some();
    if has_html && has_plain {
        let options = [
            (t(locale, Key::HtmlPart), state.part == PartMode::Html),
            (t(locale, Key::PlainPart), state.part == PartMode::Plain),
        ];
        if let Some(index) = super::segments(&mut bar, &options) {
            state.part = if index == 0 {
                PartMode::Html
            } else {
                PartMode::Plain
            };
        }
    } else {
        let _ = hud(
            &mut bar,
            if has_html {
                t(locale, Key::HtmlPart)
            } else if has_plain {
                t(locale, Key::PlainPart)
            } else {
                "capsule"
            },
            theme::FAINT,
        );
    }

    bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ghost(
            ui,
            Icon::Download,
            Some(".eml"),
            t(locale, Key::ExportEml),
            true,
            26.0,
        )
        .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(t(locale, Key::ExportEml))
                .set_file_name(&capsule.file_name)
                .add_filter("email", &["eml"])
                .save_file()
            {
                let _ = std::fs::write(path, capsule.export_eml());
            }
        }
        ui.add_space(8.0);
        if !capsule.native {
            let _ = tag_chip(ui, t(locale, Key::ReadOnly), theme::BADGE_BETA);
        }
    });

    ScrollArea::vertical()
        .id_salt("mail-body")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            centred(ui, 720.0, |ui| {
                ui.add_space(28.0);
                // Envelope.
                ui.add(
                    egui::Label::new(
                        RichText::new(if capsule.subject.is_empty() {
                            t(locale, Key::Untitled).to_string()
                        } else {
                            capsule.subject.clone()
                        })
                        .family(theme::display())
                        .size(27.0)
                        .color(theme::INK),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(7.0, 5.0);
                    envelope_chip(ui, &capsule.from, theme::INK);
                    ui.label(RichText::new("→").size(11.0).color(theme::FAINT));
                    envelope_chip(ui, &capsule.to, theme::MUTED);
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(7.0, 5.0);
                    ui.label(
                        RichText::new(&capsule.date)
                            .family(theme::mono())
                            .size(9.5)
                            .color(theme::FAINT),
                    );
                    for tag in &capsule.tags {
                        let _ = tag_chip(ui, tag, theme::MUTED);
                    }
                    if !capsule.attachments.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "{} {}",
                                capsule.attachments.len(),
                                t(locale, Key::Attachments)
                            ))
                            .family(theme::mono())
                            .size(9.5)
                            .color(theme::FAINT),
                        );
                    }
                });
                ui.add_space(16.0);
                ui.painter().hline(
                    ui.cursor().min.x..=ui.cursor().min.x + ui.available_width(),
                    ui.cursor().min.y,
                    theme::hairline(),
                );
                ui.add_space(18.0);

                if capsule.native {
                    native_body(ui, capsule, locale);
                } else {
                    external_body(ui, capsule, state, locale);
                }

                if !capsule.attachments.is_empty() {
                    ui.add_space(26.0);
                    attachments(ui, capsule, locale);
                }
                ui.add_space(60.0);
            });
        });

    let _ = action.view.take();
    action
}

fn envelope_chip(ui: &mut Ui, value: &str, color: Color32) {
    ui.add(
        egui::Label::new(
            RichText::new(truncate_chars(value, 52))
                .family(theme::mono())
                .size(10.0)
                .color(color),
        )
        .truncate(),
    );
}

fn external_body(ui: &mut Ui, capsule: &Capsule, state: &MailState, locale: Locale) {
    let html = capsule.html_body.as_deref();
    let plain = capsule.plain_body.as_deref();

    let blocks = match state.part {
        PartMode::Html => html.map(mail::html_to_blocks),
        PartMode::Plain => None,
    }
    .or_else(|| {
        if state.part == PartMode::Plain {
            plain.map(mail::plain_to_blocks)
        } else {
            None
        }
    })
    .or_else(|| plain.map(mail::plain_to_blocks))
    .or_else(|| html.map(mail::html_to_blocks))
    .unwrap_or_default();

    if blocks.is_empty() {
        empty_state(
            ui,
            Icon::Mail,
            t(locale, Key::NoBody),
            capsule.parse_note.as_deref().unwrap_or(""),
        );
        return;
    }

    for block in &blocks {
        render_block(ui, block);
    }

    if html.is_some() {
        ui.add_space(14.0);
        ui.add(
            egui::Label::new(
                RichText::new(t(locale, Key::HtmlRenderingNotice))
                    .size(10.5)
                    .color(theme::FAINT),
            )
            .wrap(),
        );
    }
}

fn render_block(ui: &mut Ui, block: &MailBlock) {
    match block {
        MailBlock::Rule => {
            ui.add_space(10.0);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter()
                .hline(rect.x_range(), rect.center().y, theme::hairline());
            ui.add_space(10.0);
        }
        MailBlock::Quote(spans) => {
            let inner = Frame::new()
                .fill(theme::WASH)
                .inner_margin(Margin {
                    left: 14,
                    right: 10,
                    top: 8,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| render_spans(ui, spans, 12.5, theme::MUTED));
                });
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    inner.response.rect.min,
                    vec2(2.0, inner.response.rect.height()),
                ),
                0.0,
                theme::fade(theme::ACCENT, 0.6),
            );
            ui.add_space(10.0);
        }
        MailBlock::Heading(level, spans) => {
            ui.add_space(if *level == 1 { 12.0 } else { 8.0 });
            ui.horizontal_wrapped(|ui| {
                let size = match level {
                    1 => 21.0,
                    2 => 17.0,
                    _ => 14.0,
                };
                render_spans_styled(ui, spans, size, theme::display(), theme::INK);
            });
            ui.add_space(6.0);
        }
        MailBlock::Bullet(spans) => {
            ui.horizontal_top(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new("•").size(12.0).color(theme::FAINT));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| render_spans(ui, spans, 13.5, theme::INK));
            });
            ui.add_space(3.0);
        }
        MailBlock::Paragraph(spans) => {
            ui.horizontal_wrapped(|ui| render_spans(ui, spans, 13.5, theme::INK));
            ui.add_space(9.0);
        }
    }
}

fn render_spans(ui: &mut Ui, spans: &[mail::Span], size: f32, color: Color32) {
    render_spans_styled(ui, spans, size, egui::FontFamily::Proportional, color);
}

fn render_spans_styled(
    ui: &mut Ui,
    spans: &[mail::Span],
    size: f32,
    family: egui::FontFamily,
    color: Color32,
) {
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        let family = if span.code {
            theme::mono()
        } else {
            family.clone()
        };
        let mut text = RichText::new(&span.text)
            .size(size)
            .color(color)
            .family(family);
        if span.bold {
            text = text.strong();
        }
        if span.italic {
            text = text.italics();
        }
        if let Some(url) = &span.link {
            ui.add(egui::Label::new(text.color(theme::ACCENT_INK)).sense(Sense::click()))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(url)
                .clicked()
                .then(|| {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
                });
        } else {
            ui.add(egui::Label::new(text));
        }
    }
}

fn native_body(ui: &mut Ui, capsule: &Capsule, locale: Locale) {
    if capsule.blocks.is_empty() {
        ui.label(
            RichText::new(t(locale, Key::NoBody))
                .size(12.5)
                .color(theme::FAINT),
        );
        return;
    }
    for block in &capsule.blocks {
        match block_type(block) {
            "heading" => {
                let level = block
                    .get("data")
                    .and_then(|data| data.get("level"))
                    .and_then(|level| level.as_u64())
                    .unwrap_or(1)
                    .clamp(1, 3);
                let size = match level {
                    1 => 21.0,
                    2 => 17.0,
                    _ => 14.0,
                };
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::mime::block_data_text(block))
                            .family(theme::display())
                            .size(size)
                            .color(theme::INK),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
            }
            "paragraph" => {
                let text = crate::mime::block_data_text(block);
                let galley = crate::links::layout_clean(ui, &text, 13.5, ui.available_width());
                ui.label(galley);
                ui.add_space(9.0);
            }
            "callout" => {
                let data = block.get("data");
                let title = data
                    .and_then(|d| d.get("title"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let content = data
                    .and_then(|d| d.get("content"))
                    .or_else(|| data.and_then(|d| d.get("text")))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let tone = data
                    .and_then(|d| d.get("tone"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("blue");
                let accent = match tone {
                    "emerald" => Color32::from_rgb(52, 211, 153),
                    "amber" => theme::BADGE_BETA,
                    "purple" => Color32::from_rgb(167, 139, 250),
                    "rose" => Color32::from_rgb(251, 113, 133),
                    _ => theme::ACCENT,
                };
                let inner = Frame::new()
                    .fill(theme::WASH)
                    .corner_radius(theme::RADIUS)
                    .inner_margin(Margin {
                        left: 14,
                        right: 12,
                        top: 10,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        if !title.is_empty() {
                            ui.label(
                                RichText::new(title)
                                    .family(theme::semibold())
                                    .size(12.5)
                                    .color(accent),
                            );
                            ui.add_space(3.0);
                        }
                        let wrap_width = ui.available_width();
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                ui.add_space(3.0);
                                continue;
                            }
                            if trimmed.starts_with("### ") {
                                ui.add_space(5.0);
                                ui.label(
                                    RichText::new(&trimmed[4..])
                                        .font(egui::FontId::new(13.0, theme::semibold()))
                                        .color(theme::INK),
                                );
                            } else if trimmed.starts_with("## ") {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(&trimmed[3..])
                                        .font(egui::FontId::new(14.0, theme::semibold()))
                                        .color(theme::INK),
                                );
                            } else if let Some(dot_pos) = trimmed.find(". ") {
                                if trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit())
                                    && dot_pos <= 3
                                {
                                    let num = &trimmed[..dot_pos];
                                    let rest = trimmed[dot_pos + 2..].trim();
                                    ui.horizontal(|ui| {
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new(format!("{num}."))
                                                .font(egui::FontId::new(11.0, theme::mono()))
                                                .color(accent),
                                        );
                                        let galley = crate::links::layout_clean(
                                            ui,
                                            rest,
                                            12.5,
                                            wrap_width - 24.0,
                                        );
                                        ui.label(galley);
                                    });
                                } else {
                                    let galley =
                                        crate::links::layout_clean(ui, line, 12.5, wrap_width);
                                    ui.label(galley);
                                }
                            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                                let rest = trimmed[2..].trim();
                                ui.horizontal(|ui| {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("•")
                                            .font(egui::FontId::new(12.5, theme::semibold()))
                                            .color(accent),
                                    );
                                    let galley = crate::links::layout_clean(
                                        ui,
                                        rest,
                                        12.5,
                                        wrap_width - 20.0,
                                    );
                                    ui.label(galley);
                                });
                            } else {
                                let galley = crate::links::layout_clean(ui, line, 12.5, wrap_width);
                                ui.label(galley);
                            }
                        }
                    });
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        inner.response.rect.min,
                        vec2(2.0, inner.response.rect.height()),
                    ),
                    0.0,
                    accent,
                );
                ui.add_space(10.0);
            }
            "todo_list" => {
                if let Some(title) = block
                    .get("data")
                    .and_then(|d| d.get("title"))
                    .and_then(|t| t.as_str())
                    .filter(|title| !title.is_empty())
                {
                    ui.label(
                        RichText::new(title)
                            .family(theme::semibold())
                            .size(13.0)
                            .color(theme::INK),
                    );
                    ui.add_space(4.0);
                }
                let items = block
                    .get("data")
                    .and_then(|d| d.get("items"))
                    .and_then(|items| items.as_array())
                    .cloned()
                    .unwrap_or_default();
                for item in items {
                    let done = item
                        .get("done")
                        .and_then(|done| done.as_bool())
                        .unwrap_or(false);
                    let text = item
                        .get("text")
                        .and_then(|text| text.as_str())
                        .unwrap_or("");
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        ui.painter().rect(
                            rect,
                            theme::RADIUS,
                            if done {
                                theme::ACCENT
                            } else {
                                Color32::TRANSPARENT
                            },
                            Stroke::new(
                                1.0,
                                if done {
                                    theme::ACCENT
                                } else {
                                    theme::LINE_STRONG
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        if done {
                            crate::icons::paint(
                                ui.painter(),
                                rect.shrink(2.0),
                                Icon::Check,
                                Color32::WHITE,
                            );
                        }
                        ui.add(
                            egui::Label::new(RichText::new(text).size(12.5).color(if done {
                                theme::FAINT
                            } else {
                                theme::INK
                            }))
                            .wrap(),
                        );
                    });
                }
                ui.add_space(9.0);
            }
            kind => {
                ui.label(
                    RichText::new(format!("[{kind}]"))
                        .family(theme::mono())
                        .size(10.0)
                        .color(theme::FAINT),
                );
                ui.add_space(6.0);
            }
        }
    }
}

fn attachments(ui: &mut Ui, capsule: &Capsule, locale: Locale) {
    ui.label(
        RichText::new(t(locale, Key::AttachmentsTitle))
            .family(theme::semibold())
            .size(13.0)
            .color(theme::INK),
    );
    ui.add_space(6.0);
    for attachment in &capsule.attachments {
        Frame::new()
            .fill(theme::WASH)
            .corner_radius(theme::RADIUS)
            .stroke(theme::hairline())
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    crate::icons::paint(
                        ui.painter(),
                        egui::Rect::from_min_size(
                            ui.cursor().min + vec2(0.0, 4.0),
                            vec2(13.0, 13.0),
                        ),
                        Icon::Doc,
                        theme::MUTED,
                    );
                    ui.add_space(18.0);
                    ui.label(RichText::new(&attachment.name).size(12.0).color(theme::INK));
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            attachment.mime_type,
                            crate::human_bytes(attachment.size_bytes)
                        ))
                        .family(theme::mono())
                        .size(9.0)
                        .color(theme::FAINT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost(ui, Icon::Download, None, t(locale, Key::SaveAs), true, 22.0)
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title(t(locale, Key::SaveAs))
                                .set_file_name(&attachment.name)
                                .save_file()
                            {
                                let bytes = match &attachment.raw_b64 {
                                    Some(b64) => {
                                        use base64::Engine as _;
                                        base64::engine::general_purpose::STANDARD
                                            .decode(b64)
                                            .unwrap_or_else(|_| {
                                                attachment.content.as_bytes().to_vec()
                                            })
                                    }
                                    None => attachment.content.as_bytes().to_vec(),
                                };
                                let _ = std::fs::write(path, bytes);
                            }
                        }
                    });
                });
            });
        ui.add_space(5.0);
    }
}
