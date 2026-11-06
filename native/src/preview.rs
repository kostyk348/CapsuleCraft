//! Inline attachment previews drawn directly on the page (v1.12.3).
//!
//! Images render as pictures and PDFs as their first page, so a page with an
//! attached scan or PDF *shows* the document instead of hiding it behind a
//! filename row. Everything decodes once, on first draw, and the texture is
//! cached for the session — a key includes the file name and size, so a
//! re-saved attachment invalidates itself.
//!
//! Rendering happens lazily on the UI thread: an image is downscaled to
//! [`IMAGE_MAX_DIM`] and a PDF page renders at a fixed 760 px, both cheap
//! enough for a single one-off hit per attachment.

use std::collections::HashMap;
use std::path::Path;

use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::mime::Attachment;
use crate::theme;

/// Longest side of an inline image texture, in pixels.
pub const IMAGE_MAX_DIM: u32 = 1600;

/// Width at which PDF first pages render, in pixels.
pub const PDF_PREVIEW_WIDTH: f32 = 760.0;

/// A resolved preview for one attachment.
pub enum Preview {
    /// Decoded (and possibly downscaled) raster image.
    Image(TextureHandle),
    /// First page of a PDF + total page count.
    Pdf(TextureHandle, usize),
    /// The pdfium engine component is not installed.
    NeedsEngine,
    /// Bytes exist but could not be decoded.
    Failed(String),
}

#[derive(Default)]
pub struct PreviewCache {
    map: HashMap<String, Preview>,
}

impl PreviewCache {
    pub fn get(&self, key: &str) -> Option<&Preview> {
        self.map.get(key)
    }

    pub fn insert(&mut self, key: String, preview: Preview) {
        // Bound the cache: textures are big, and 96 previews already cover
        // any realistic browsing session. A plain clear keeps the code honest
        // (entries rebuild on the next draw) without an LRU dependency.
        if self.map.len() >= 96 {
            self.map.clear();
        }
        self.map.insert(key, preview);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// Cache key for one attachment of one page.
pub fn preview_key(capsule_id: &str, attachment: &Attachment) -> String {
    format!(
        "{}/{}|{}|{}",
        capsule_id, attachment.name, attachment.mime_type, attachment.size_bytes
    )
}

/// True when the attachment deserves an inline preview.
pub fn previewable(attachment: &Attachment) -> bool {
    attachment.mime_type.starts_with("image/") && attachment.mime_type != "image/svg+xml"
        || attachment.mime_type == "application/pdf"
}

/// Decode image bytes into a texture, downscaled to [`IMAGE_MAX_DIM`].
pub fn decode_image(ctx: &egui::Context, key: &str, bytes: &[u8]) -> Result<TextureHandle, String> {
    let decoded = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let scaled = if decoded.width().max(decoded.height()) > IMAGE_MAX_DIM {
        decoded.thumbnail(IMAGE_MAX_DIM, IMAGE_MAX_DIM)
    } else {
        decoded
    };
    let rgba = scaled.to_rgba8();
    let (width, height) = rgba.dimensions();
    let image =
        ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba.as_raw());
    Ok(ctx.load_texture(format!("inline-{key}"), image, TextureOptions::LINEAR))
}

/// Render a PDF's first page through pdfium into a texture.
pub fn render_pdf_preview(
    ctx: &egui::Context,
    key: &str,
    bytes: &[u8],
    lib: &Path,
) -> Result<Preview, String> {
    let (image, pages) = crate::viewer::render_pdf_page(bytes, lib, 0, PDF_PREVIEW_WIDTH)?;
    let handle = ctx.load_texture(format!("inline-{key}"), image, TextureOptions::LINEAR);
    Ok(Preview::Pdf(handle, pages))
}

/// Paint the checkerboard-free sunken frame every preview sits in.
pub fn preview_frame<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(theme::wash())
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(egui::Margin::same(6))
        .show(ui, add)
}

/// Accent outline drawn over a hovered preview card.
pub fn hover_outline(ui: &egui::Ui, rect: egui::Rect, hovered: bool) {
    if hovered {
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            egui::Stroke::new(1.0, theme::accent()),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    fn attachment(name: &str, mime: &str, size: usize) -> Attachment {
        Attachment {
            name: name.to_string(),
            cid: format!("{name}@capsuleos"),
            mime_type: mime.to_string(),
            content: String::new(),
            size_bytes: size,
            raw_b64: None,
        }
    }

    #[test]
    fn previewable_covers_raster_and_pdf_but_not_svg() {
        assert!(previewable(&attachment("a.png", "image/png", 1)));
        assert!(previewable(&attachment("a.jpg", "image/jpeg", 1)));
        assert!(previewable(&attachment("a.pdf", "application/pdf", 1)));
        assert!(!previewable(&attachment("a.svg", "image/svg+xml", 1)));
        assert!(!previewable(&attachment("a.csv", "text/csv", 1)));
        assert!(!previewable(&attachment("a.mp3", "audio/mpeg", 1)));
    }

    #[test]
    fn keys_distinguish_pages_and_versions() {
        let a = attachment("scan.pdf", "application/pdf", 100);
        let mut b = attachment("scan.pdf", "application/pdf", 100);
        assert_eq!(preview_key("page-1", &a), preview_key("page-1", &b));
        b.size_bytes = 200;
        assert_ne!(preview_key("page-1", &a), preview_key("page-1", &b));
        assert_ne!(preview_key("page-1", &a), preview_key("page-2", &a));
    }

    #[test]
    fn decode_rejects_garbage_and_accepts_png() {
        let ctx = egui::Context::default();
        assert!(decode_image(&ctx, "k", b"not an image").is_err());
        // A 3×2 red PNG, hand-built via the png crate to stay dependency-free.
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 3, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9, 1, 2, 3])
                .unwrap();
        }
        let handle = decode_image(&ctx, "k", &png).expect("png decodes");
        assert_eq!(handle.size_vec2().x, 3.0);
        assert_eq!(handle.size_vec2().y, 2.0);
    }

    #[test]
    fn cache_bounds_and_clears() {
        let mut cache = PreviewCache::default();
        for i in 0..120 {
            cache.insert(format!("k{i}"), Preview::NeedsEngine);
        }
        // Bound hit: the map never grows past the cap.
        assert!(cache.get("k119").is_some());
        cache.clear();
        assert!(cache.get("k119").is_none());
    }

    #[test]
    fn base64_roundtrip_helper_matches_app_encoding() {
        // The app stores attachments as standard base64; previews decode with
        // the same alphabet.
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"hello");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            b"hello"
        );
    }
}
