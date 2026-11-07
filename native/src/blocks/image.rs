//! Inline image block: pick a file (or an attachment already in the page) and
//! render it in the page flow, wherever the block sits.
//!
//! v2.1.0 request: «вставку картинок куда хочу». The block is a normal block,
//! so it can be moved with the block menu, inserted below any other block
//! («Вставить изображение ниже»), and it renders at the chosen width with a
//! caption — instead of images living only in the attachment shelf at the
//! bottom of the page.
//!
//! The bytes live in `capsule.attachments` (one copy, same as audio); the
//! block only stores the reference plus presentation: `{cid, caption, width}`.

use base64::Engine as _;
use egui::{vec2, Align, Frame, Layout, Margin, Rect, RichText, Sense, TextEdit, Ui};
use serde_json::{json, Value};

use super::BlockOp;
use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::Attachment;
use crate::theme;
use crate::{guess_mime, hud, sanitize_cid, text_button};

/// Image MIME types the block accepts (and the `image` crate decodes).
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

fn is_image(attachment: &Attachment) -> bool {
    attachment.mime_type.starts_with("image/")
}

/// Width presets as percent of the text column. 100 % is the default.
const WIDTHS: [u64; 4] = [25, 50, 75, 100];

pub fn draw_image_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &[Attachment],
    locale: Locale,
    op: &mut Option<BlockOp>,
) -> bool {
    let mut changed = false;
    let cid = block
        .get("data")
        .and_then(|data| data.get("cid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches("cid:")
        .to_string();
    let file = attachments
        .iter()
        .find(|attachment| attachment.cid == cid && is_image(attachment));

    Frame::new()
        .fill(theme::sunken())
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            // ── Header: name, pick, replace, open ────────────────────────
            ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    Icon::Image,
                    theme::accent_ink(),
                );
                ui.add_space(20.0);
                let name = file
                    .map(|attachment| attachment.name.clone())
                    .unwrap_or_else(|| t(locale, Key::ImageNoFile).to_string());
                ui.label(RichText::new(name).size(12.0).color(theme::ink()));
                if let Some(attachment) = file {
                    ui.add_space(6.0);
                    let _ = hud(ui, &crate::human_bytes(attachment.size_bytes), theme::faint());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, Some(Icon::Plus), t(locale, Key::ImageChoose), false)
                        .clicked()
                    {
                        if let Some((name, cid, mime, b64, size, ..)) = pick_image(locale) {
                            block["data"]["cid"] = json!(format!("cid:{cid}"));
                            block["data"]["width"] = json!(100);
                            if block
                                .get("data")
                                .and_then(|data| data.get("caption"))
                                .is_none()
                            {
                                block["data"]["caption"] = json!("");
                            }
                            *op = Some(BlockOp::AttachImage {
                                name,
                                cid,
                                mime,
                                b64,
                                size,
                            });
                            changed = true;
                        }
                    }
                    // Images already in the page: one click instead of a
                    // second copy of the same file on disk.
                    let pixels = attachments
                        .iter()
                        .filter(|attachment| is_image(attachment))
                        .collect::<Vec<_>>();
                    if !pixels.is_empty() {
                        let response =
                            text_button(ui, None, t(locale, Key::ImageFromAttachments), false);
                        let mut chosen: Option<String> = None;
                        egui::Popup::menu(&response)
                            .frame(crate::popover_frame())
                            .align(egui::RectAlign::BOTTOM_END)
                            .show(|ui| {
                                ui.set_min_width(180.0);
                                for attachment in &pixels {
                                    if crate::menu_row(
                                        ui,
                                        Some(Icon::Image),
                                        &crate::util::truncate_chars(&attachment.name, 34),
                                        None,
                                        attachment.cid == cid,
                                        true,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        chosen = Some(attachment.cid.clone());
                                    }
                                }
                            });
                        if let Some(picked) = chosen {
                            if picked != cid {
                                block["data"]["cid"] = json!(format!("cid:{picked}"));
                                changed = true;
                            }
                        }
                    }
                    if let Some(attachment) = file {
                        if text_button(ui, Some(Icon::Eye), t(locale, Key::ImageOpen), false)
                            .clicked()
                        {
                            *op = Some(BlockOp::OpenViewer(attachment.cid.clone()));
                        }
                    }
                });
            });

            ui.add_space(10.0);

            // ── The image itself ─────────────────────────────────────────
            let percent = block
                .get("data")
                .and_then(|data| data.get("width"))
                .and_then(Value::as_u64)
                .unwrap_or(100)
                .clamp(10, 100);
            let available = ui.available_width();
            if let Some(attachment) = file {
                match texture_for(ui, &attachment.cid, attachment) {
                    Some(handle) => {
                        let natural = handle.size_vec2();
                        let target = available * (percent as f32 / 100.0);
                        let scale = if natural.x > 0.0 {
                            (target / natural.x).min(1.0)
                        } else {
                            1.0
                        };
                        let size = natural * scale;
                        let response = ui.add(
                            egui::Image::new((handle.id(), size))
                                .sense(Sense::click())
                                .corner_radius(4.0),
                        );
                        response.clone().on_hover_text(t(locale, Key::ImageOpen));
                        if response.clicked() {
                            *op = Some(BlockOp::OpenViewer(attachment.cid.clone()));
                        }
                    }
                    None => {
                        let _ = hud(ui, t(locale, Key::ImageDecodeFailed), theme::bad());
                    }
                }
                ui.add_space(8.0);
                // Width presets + replace.
                ui.horizontal(|ui| {
                    let _ = hud(ui, t(locale, Key::ImageWidth), theme::faint());
                    for width in WIDTHS {
                        if text_button(ui, None, &format!("{width}%"), width == percent).clicked() {
                            block["data"]["width"] = json!(width);
                            changed = true;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if text_button(ui, Some(Icon::Reload), t(locale, Key::ImageReplace), false)
                            .clicked()
                        {
                            if let Some((name, cid, mime, b64, size, ..)) = pick_image(locale) {
                                block["data"]["cid"] = json!(format!("cid:{cid}"));
                                *op = Some(BlockOp::AttachImage {
                                    name,
                                    cid,
                                    mime,
                                    b64,
                                    size,
                                });
                                changed = true;
                            }
                        }
                    });
                });
            } else {
                // Empty state: one obvious button, the same dashed feel as
                // the rest of the editor's placeholders.
                let height = 74.0_f32.min(available * 0.3);
                let (rect, _) = ui.allocate_exact_size(vec2(available, height), Sense::hover());
                ui.painter().rect_stroke(
                    rect,
                    theme::RADIUS,
                    egui::Stroke::new(1.0, theme::hairline().color),
                    egui::StrokeKind::Inside,
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink(10.0))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                let _ = hud(&mut child, t(locale, Key::ImageNoFile), theme::faint());
                child.add_space(8.0);
                if text_button(
                    &mut child,
                    Some(Icon::Plus),
                    t(locale, Key::ImageChoose),
                    false,
                )
                .clicked()
                {
                    if let Some((name, cid, mime, b64, size, ..)) = pick_image(locale) {
                        block["data"]["cid"] = json!(format!("cid:{cid}"));
                        block["data"]["width"] = json!(100);
                        block["data"]["caption"] = json!("");
                        *op = Some(BlockOp::AttachImage {
                            name,
                            cid,
                            mime,
                            b64,
                            size,
                        });
                        changed = true;
                    }
                }
            }

            // ── Caption ──────────────────────────────────────────────────
            let mut caption = block
                .get("data")
                .and_then(|data| data.get("caption"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let response = ui.add(
                TextEdit::singleline(&mut caption)
                    .font(egui::FontId::new(12.0, egui::FontFamily::Proportional))
                    .text_color(theme::muted())
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .hint_text(t(locale, Key::ImageCaption)),
            );
            if response.changed() {
                block["data"]["caption"] = json!(caption);
                changed = true;
            }
        });
    changed
}

/// File dialog → `(name, cid, mime, base64, size, width, height)`.
pub(crate) fn pick_image(locale: Locale) -> Option<(String, String, String, String, usize, u32, u32)> {
    let path = crate::util::dialog::FileDialog::new()
        .set_title(t(locale, Key::ImageChoose))
        .add_filter("images", &IMAGE_EXTENSIONS)
        .pick_file()?;
    let bytes = std::fs::read(&path).ok()?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image")
        .to_string();
    // The cid is derived from the bytes, not from the name: picking the same
    // picture twice must reuse one attachment instead of piling up copies.
    let cid = format!("image-{}", sanitize_cid(&name));
    let (width, height) = image::load_from_memory(&bytes)
        .map(|decoded| {
            use image::GenericImageView as _;
            decoded.dimensions()
        })
        .unwrap_or((0, 0));
    let mime = guess_mime(&name);
    let size = bytes.len();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some((name, cid, mime, b64, size, width, height))
}

/// Decode once per (cid) and keep the texture in egui's temp store: a page
/// with ten photos must not decode ten JPEGs every frame.
pub(crate) fn texture_for(ui: &Ui, cid: &str, attachment: &Attachment) -> Option<egui::TextureHandle> {
    let key = ui.id().with(("image-block-texture", cid));
    if let Some(handle) = ui.data(|data| data.get_temp::<egui::TextureHandle>(key)) {
        return Some(handle);
    }
    let raw = attachment.raw_b64.as_deref().unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        decoded.as_raw(),
    );
    let handle = ui
        .ctx()
        .load_texture(format!("image-block-{cid}"), color, Default::default());
    ui.data_mut(|data| data.insert_temp(key, handle.clone()));
    Some(handle)
}
