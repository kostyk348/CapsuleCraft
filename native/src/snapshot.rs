//! PNG capture of the rendered window.
//!
//! Used by `--snapshot` to produce the README screenshots from the real app
//! (same fonts, same layout, no window-manager automation) and available for
//! manual visual checks. eframe hands us the framebuffer through
//! `Context::request_screenshot`, so this works even when the window is not
//! visible on screen.

use std::path::Path;

use egui::ColorImage;

/// Encode an image as 8-bit RGBA PNG bytes (used to hand a rendered PDF page
/// to the OCR engine, which takes raster input, and to attach images).
pub fn encode_png(image: &ColorImage) -> Result<Vec<u8>, String> {
    let [width, height] = image.size;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        let [r, g, b, a] = pixel.to_array();
        bytes.extend_from_slice(&[r, g, b, a]);
    }
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&bytes).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(png_bytes)
}

/// Encode a rendered frame as 8-bit RGBA PNG.
pub fn write_png(path: &Path, image: &ColorImage) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let bytes = encode_png(image).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, ColorImage};

    #[test]
    fn writes_a_valid_png_header() {
        let dir = std::env::temp_dir().join("capsuleos-snapshot-test");
        let path = dir.join("frame.png");
        let image = ColorImage {
            size: [2, 1],
            source_size: egui::vec2(2.0, 1.0),
            pixels: vec![Color32::RED, Color32::BLUE],
        };
        write_png(&path, &image).expect("png written");
        let bytes = std::fs::read(&path).expect("png readable");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(bytes.len() > 8 + 2 * 4);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn creates_missing_directories() {
        let dir = std::env::temp_dir().join("capsuleos-snapshot-nested/a/b");
        let path = dir.join("frame.png");
        let image = ColorImage {
            size: [1, 1],
            source_size: egui::vec2(1.0, 1.0),
            pixels: vec![Color32::BLACK],
        };
        write_png(&path, &image).expect("png written");
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }
}
