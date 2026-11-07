//! Brand assets shared by the window and the title bar.
//!
//! Four brand styles ship inside the binary — the neon Nova artwork
//! (default), the minimal Document mark, a generated Capsule mark — plus a
//! user-supplied PNG copied into the app data dir. A single executable stays
//! self-contained; the custom icon only touches the disk.

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

const ICON_NOVA_PNG: &[u8] = include_bytes!("../assets/icon_nova.png");
const ICON_DOC_PNG: &[u8] = include_bytes!("../assets/icon.png");
const ICON_CAPSULE_PNG: &[u8] = include_bytes!("../assets/icon_capsule.png");

/// Brand icon style preference.
///
/// `Mascot` survives only as a deserialization alias: configs saved before
/// Nova existed name it, and dropping the variant would reset every other
/// setting along with it. It behaves exactly like [`BrandIconStyle::Nova`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrandIconStyle {
    #[default]
    Nova,
    Document,
    Capsule,
    Custom,
    #[serde(other)]
    Mascot,
}

impl BrandIconStyle {
    /// The next style in the cycle the titlebar mark walks through.
    pub fn cycle(self) -> Self {
        match self {
            Self::Nova | Self::Mascot => Self::Document,
            Self::Document => Self::Capsule,
            Self::Capsule => Self::Custom,
            Self::Custom => Self::Nova,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Nova | Self::Mascot => "nova",
            Self::Document => "document",
            Self::Capsule => "capsule",
            Self::Custom => "custom",
        }
    }

    /// `true` for the alias variant; `UiState::load` normalizes it away.
    pub fn is_legacy_mascot(self) -> bool {
        self == Self::Mascot
    }
}

/// Where a user-supplied icon PNG lives: next to `ui_state.json`.
pub fn custom_icon_path() -> Option<std::path::PathBuf> {
    Some(crate::vault::app_data_dir().join("custom_icon.png"))
}

/// Decode `bytes` as PNG (RGB or RGBA, any size).
fn decode_png(bytes: &[u8]) -> Option<ColorImage> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    let size = [info.width as usize, info.height as usize];
    let raw = &buffer[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => Some(ColorImage::from_rgba_unmultiplied(size, raw)),
        png::ColorType::Rgb => Some(ColorImage::from_rgb(size, raw)),
        _ => None,
    }
}

/// Decode the icon for `style`; `custom` is the file backing
/// [`BrandIconStyle::Custom`], falling back to Nova when missing or broken.
pub fn decode(style: BrandIconStyle, custom: Option<&std::path::Path>) -> Option<ColorImage> {
    match style {
        BrandIconStyle::Nova | BrandIconStyle::Mascot => decode_png(ICON_NOVA_PNG),
        BrandIconStyle::Document => decode_png(ICON_DOC_PNG),
        BrandIconStyle::Capsule => decode_png(ICON_CAPSULE_PNG),
        BrandIconStyle::Custom => custom
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| decode_png(&bytes))
            .or_else(|| decode_png(ICON_NOVA_PNG)),
    }
}

/// The icon the operating system shows in the taskbar and window switcher.
pub fn window_icon(
    style: BrandIconStyle,
    custom: Option<&std::path::Path>,
) -> Option<egui::IconData> {
    let image = decode(style, custom)?;
    Some(egui::IconData {
        rgba: image.as_raw().to_vec(),
        width: image.size[0] as u32,
        height: image.size[1] as u32,
    })
}

/// The brand mark as a cached texture for the title bar. The custom style
/// re-decodes when the backing file changes (path kept next to the texture).
pub fn texture(
    ctx: &Context,
    style: BrandIconStyle,
    custom: Option<&std::path::Path>,
) -> Option<TextureHandle> {
    let id = egui::Id::new(("brand-icon-texture", style.key()));
    if let Some(handle) = ctx.data(|data| data.get_temp::<TextureHandle>(id)) {
        if style != BrandIconStyle::Custom {
            return Some(handle);
        }
        let cached: Option<String> = ctx.data(|data| data.get_temp(id.with("path")));
        if cached.as_deref() == custom.and_then(|p| p.to_str()) {
            return Some(handle);
        }
    }
    let image = decode(style, custom)?;
    let handle = ctx.load_texture("brand-icon", image, TextureOptions::LINEAR);
    ctx.data_mut(|data| {
        data.insert_temp(id, handle.clone());
        if style == BrandIconStyle::Custom {
            data.insert_temp(
                id.with("path"),
                custom.and_then(|p| p.to_str()).map(str::to_string),
            );
        }
    });
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icons_decode() {
        for style in [
            BrandIconStyle::Nova,
            BrandIconStyle::Document,
            BrandIconStyle::Capsule,
        ] {
            let image = decode(style, None).expect("icon decodes");
            assert_eq!(image.size, [256, 256]);
            assert_eq!(image.pixels.len(), 256 * 256);
        }
    }

    #[test]
    fn window_icon_has_rgba_bytes() {
        for style in [
            BrandIconStyle::Nova,
            BrandIconStyle::Document,
            BrandIconStyle::Capsule,
        ] {
            let icon = window_icon(style, None).expect("icon");
            assert_eq!(icon.width, 256);
            assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        }
    }

    #[test]
    fn custom_falls_back_to_nova_when_missing() {
        let missing = std::path::Path::new("/nonexistent/custom_icon.png");
        let image = decode(BrandIconStyle::Custom, Some(missing)).expect("fallback");
        assert_eq!(image.size, [256, 256]);
    }

    #[test]
    fn custom_loads_from_disk() {
        let dir = std::env::temp_dir().join(format!("capsule-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let path = dir.join("custom_icon.png");
        std::fs::write(&path, ICON_CAPSULE_PNG).expect("write png");
        let image = decode(BrandIconStyle::Custom, Some(&path)).expect("custom decodes");
        assert_eq!(image.size, [256, 256]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn style_cycles_through_all_four() {
        let mut style = BrandIconStyle::Nova;
        let mut seen = vec![style];
        for _ in 0..3 {
            style = style.cycle();
            seen.push(style);
        }
        assert_eq!(seen.len(), 4);
        assert!(seen.contains(&BrandIconStyle::Document));
        assert!(seen.contains(&BrandIconStyle::Capsule));
        assert!(seen.contains(&BrandIconStyle::Custom));
        assert_eq!(style.cycle(), BrandIconStyle::Nova);
    }

    #[test]
    fn legacy_mascot_still_parses() {
        let style: BrandIconStyle = serde_json::from_str("\"mascot\"").expect("alias");
        assert_eq!(style, BrandIconStyle::Mascot);
        assert!(style.is_legacy_mascot());
        // The alias behaves like Nova: same bytes, same key.
        assert_eq!(style.key(), BrandIconStyle::Nova.key());
        let nova = decode(BrandIconStyle::Nova, None).expect("nova");
        let mascot = decode(BrandIconStyle::Mascot, None).expect("mascot");
        assert_eq!(nova.as_raw(), mascot.as_raw());
    }
}
