//! Visual system for the native prototype.
//!
//! The reference is ArtCraft's native shell: near-black surfaces, white/15
//! hairlines, Archivo display type, Inter body text, Geist Mono HUD labels and
//! 3 px corners on anything pressable. The aim is a document app that feels
//! designed, not a default-egui form.

use std::sync::Arc;

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Stroke,
    TextStyle, Visuals,
};

const fn hex(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

const fn alpha(rgb: u32, a: u8) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// The page behind everything.
/// Colour mode of the whole interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

static MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn mode() -> Mode {
    if MODE.load(std::sync::atomic::Ordering::Relaxed) == 1 {
        Mode::Light
    } else {
        Mode::Dark
    }
}

pub fn set_mode(m: Mode) {
    MODE.store(
        if m == Mode::Light { 1 } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Flips the mode and returns the new one.
pub fn toggle() -> Mode {
    let next = match mode() {
        Mode::Dark => Mode::Light,
        Mode::Light => Mode::Dark,
    };
    set_mode(next);
    next
}

fn pick(dark: Color32, light: Color32) -> Color32 {
    match mode() {
        Mode::Dark => dark,
        Mode::Light => light,
    }
}

/// Remembers the mode beside the locale setting; a plain file so the
/// existing `native-settings.json` locale format stays untouched.
pub fn persist() {
    let Some(dir) = crate::i18n::settings_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join("native-theme.txt"),
        match mode() {
            Mode::Dark => "dark\n",
            Mode::Light => "light\n",
        },
    );
}

/// Restores the persisted mode (called once at startup).
pub fn load() {
    let Some(dir) = crate::i18n::settings_dir() else {
        return;
    };
    if let Ok(text) = std::fs::read_to_string(dir.join("native-theme.txt")) {
        if text.trim() == "light" {
            set_mode(Mode::Light);
        }
    }
}

const DARK_BG: Color32 = hex(0x121316);
const LIGHT_BG: Color32 = hex(0xf7f6f3);
/// bg() in the active colour mode.
pub fn bg() -> Color32 {
    pick(DARK_BG, LIGHT_BG)
}

/// Menus, modals and the lightbox.
const DARK_PANEL: Color32 = hex(0x101014);
const LIGHT_PANEL: Color32 = hex(0xffffff);
/// panel() in the active colour mode.
pub fn panel() -> Color32 {
    pick(DARK_PANEL, LIGHT_PANEL)
}

/// Toolbar buttons, menus and popovers.
const DARK_CONTROLS: Color32 = hex(0x1e1f22);
const LIGHT_CONTROLS: Color32 = hex(0xefece7);
/// controls() in the active colour mode.
pub fn controls() -> Color32 {
    pick(DARK_CONTROLS, LIGHT_CONTROLS)
}

/// The title bar and the sidebar.
const DARK_CHROME: Color32 = hex(0x0a0a0c);
const LIGHT_CHROME: Color32 = hex(0xeae7e1);
/// chrome() in the active colour mode.
pub fn chrome() -> Color32 {
    pick(DARK_CHROME, LIGHT_CHROME)
}

/// Wells and sunken() inputs.
const DARK_SUNKEN: Color32 = hex(0x060607);
const LIGHT_SUNKEN: Color32 = hex(0xe3e0da);
/// sunken() in the active colour mode.
pub fn sunken() -> Color32 {
    pick(DARK_SUNKEN, LIGHT_SUNKEN)
}


/// Body text.
const DARK_INK: Color32 = hex(0xf2f1ee);
const LIGHT_INK: Color32 = hex(0x1c1c1e);
/// ink() in the active colour mode.
pub fn ink() -> Color32 {
    pick(DARK_INK, LIGHT_INK)
}

const DARK_INK_STRONG: Color32 = Color32::WHITE;
const LIGHT_INK_STRONG: Color32 = hex(0x000000);
/// ink_strong() in the active colour mode.
pub fn ink_strong() -> Color32 {
    pick(DARK_INK_STRONG, LIGHT_INK_STRONG)
}

/// Secondary text and placeholders.
const DARK_MUTED: Color32 = alpha(0xf2f1ee, 153);
const LIGHT_MUTED: Color32 = alpha(0x1c1c1e, 153);
/// muted() in the active colour mode.
pub fn muted() -> Color32 {
    pick(DARK_MUTED, LIGHT_MUTED)
}

/// Idle toolbar icons.
const DARK_DIM: Color32 = alpha(0xf2f1ee, 128);
const LIGHT_DIM: Color32 = alpha(0x1c1c1e, 128);
/// dim() in the active colour mode.
pub fn dim() -> Color32 {
    pick(DARK_DIM, LIGHT_DIM)
}

/// Counters and captions.
const DARK_FAINT: Color32 = alpha(0xf2f1ee, 102);
const LIGHT_FAINT: Color32 = alpha(0x1c1c1e, 102);
/// faint() in the active colour mode.
pub fn faint() -> Color32 {
    pick(DARK_FAINT, LIGHT_FAINT)
}

/// Sidebar links.
const DARK_SIDEBAR_INK: Color32 = alpha(0xffffff, 184);
const LIGHT_SIDEBAR_INK: Color32 = alpha(0x141414, 196);
/// sidebar_ink() in the active colour mode.
pub fn sidebar_ink() -> Color32 {
    pick(DARK_SIDEBAR_INK, LIGHT_SIDEBAR_INK)
}


/// Every hairline.
const DARK_LINE: Color32 = alpha(0xffffff, 38);
const LIGHT_LINE: Color32 = alpha(0x000000, 38);
/// line() in the active colour mode.
pub fn line() -> Color32 {
    pick(DARK_LINE, LIGHT_LINE)
}

const DARK_LINE_STRONG: Color32 = alpha(0xf2f1ee, 102);
const LIGHT_LINE_STRONG: Color32 = alpha(0x1c1c1e, 102);
/// line_strong() in the active colour mode.
pub fn line_strong() -> Color32 {
    pick(DARK_LINE_STRONG, LIGHT_LINE_STRONG)
}

/// Quiet fills.
const DARK_WASH: Color32 = alpha(0xffffff, 13);
const LIGHT_WASH: Color32 = alpha(0x000000, 8);
/// wash() in the active colour mode.
pub fn wash() -> Color32 {
    pick(DARK_WASH, LIGHT_WASH)
}

/// The selected sidebar row.
const DARK_WASH_SELECTED: Color32 = alpha(0xffffff, 20);
const LIGHT_WASH_SELECTED: Color32 = alpha(0x000000, 12);
/// wash_selected() in the active colour mode.
pub fn wash_selected() -> Color32 {
    pick(DARK_WASH_SELECTED, LIGHT_WASH_SELECTED)
}

/// Hovered menu rows.
const DARK_WASH_HOVER: Color32 = alpha(0xffffff, 26);
const LIGHT_WASH_HOVER: Color32 = alpha(0x000000, 16);
/// wash_hover() in the active colour mode.
pub fn wash_hover() -> Color32 {
    pick(DARK_WASH_HOVER, LIGHT_WASH_HOVER)
}


/// Brand blue.
const DARK_ACCENT: Color32 = hex(0x2d81ff);
const LIGHT_ACCENT: Color32 = hex(0x2d81ff);
/// accent() in the active colour mode.
pub fn accent() -> Color32 {
    pick(DARK_ACCENT, LIGHT_ACCENT)
}

const DARK_ACCENT_INK: Color32 = hex(0x74aaff);
const LIGHT_ACCENT_INK: Color32 = hex(0x1c62d6);
/// accent_ink() in the active colour mode.
pub fn accent_ink() -> Color32 {
    pick(DARK_ACCENT_INK, LIGHT_ACCENT_INK)
}


const DARK_INVERT_BG: Color32 = Color32::WHITE;
const LIGHT_INVERT_BG: Color32 = hex(0x0b0b0c);
/// invert_bg() in the active colour mode.
pub fn invert_bg() -> Color32 {
    pick(DARK_INVERT_BG, LIGHT_INVERT_BG)
}

const DARK_INVERT_FG: Color32 = hex(0x0b0b0c);
const LIGHT_INVERT_FG: Color32 = hex(0xffffff);
/// invert_fg() in the active colour mode.
pub fn invert_fg() -> Color32 {
    pick(DARK_INVERT_FG, LIGHT_INVERT_FG)
}


const DARK_BAD: Color32 = hex(0xf87171);
const LIGHT_BAD: Color32 = hex(0xd9534f);
/// bad() in the active colour mode.
pub fn bad() -> Color32 {
    pick(DARK_BAD, LIGHT_BAD)
}

const DARK_BADGE_BETA: Color32 = hex(0xffc14d);
const LIGHT_BADGE_BETA: Color32 = hex(0xb45309);
/// badge_beta in the active colour mode.
pub fn badge_beta() -> Color32 {
    pick(DARK_BADGE_BETA, LIGHT_BADGE_BETA)
}

/// Success state (installed components, finished syncs).
const DARK_OK: Color32 = hex(0x4ade80);
const LIGHT_OK: Color32 = hex(0x15803d);
/// ok() in the active colour mode.
pub fn ok() -> Color32 {
    pick(DARK_OK, LIGHT_OK)
}


/// 3 px: buttons, inputs, menus.
pub const RADIUS: CornerRadius = CornerRadius::same(3);

/// Toolbar button height.
pub const CONTROL_H: f32 = 30.0;

pub const HUD_SIZE: f32 = 9.5;
pub const HUD_SPACING: f32 = HUD_SIZE * 0.15;
pub const LABEL_SPACING: f32 = HUD_SIZE * 0.12;

pub fn hairline() -> Stroke {
    Stroke::new(1.0, line())
}

/// `c` at `a` (0..1) opacity.
pub fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

pub fn shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(150),
    }
}

/// Inter 500.
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Inter 600.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Archivo 620 at 118 % width.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// Geist Mono 500.
pub fn mono() -> FontFamily {
    FontFamily::Name("mono".into())
}

/// Geist Mono 650.
pub fn mono_bold() -> FontFamily {
    FontFamily::Name("mono-bold".into())
}

fn visuals() -> Visuals {
    let mut v = match mode() {
        Mode::Dark => Visuals::dark(),
        Mode::Light => Visuals::light(),
    };
    v.panel_fill = bg();
    v.window_fill = panel();
    v.extreme_bg_color = sunken();
    v.text_edit_bg_color = Some(Color32::TRANSPARENT);
    v.faint_bg_color = controls();
    v.code_bg_color = sunken();
    v.window_stroke = hairline();
    v.window_corner_radius = RADIUS;
    v.menu_corner_radius = RADIUS;
    v.window_shadow = shadow();
    v.popup_shadow = shadow();
    v.override_text_color = Some(ink());
    v.hyperlink_color = accent_ink();
    v.selection.bg_fill = accent();
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.text_cursor.stroke = Stroke::new(1.5, Color32::WHITE);
    v.warn_fg_color = badge_beta();
    v.error_fg_color = bad();
    v.striped = false;
    for (w, fill, stroke, fg) in [
        (&mut v.widgets.noninteractive, controls(), line(), muted()),
        (&mut v.widgets.inactive, controls(), line(), ink()),
        (&mut v.widgets.hovered, wash_hover(), line_strong(), ink_strong()),
        (&mut v.widgets.active, wash_hover(), line_strong(), ink_strong()),
        (&mut v.widgets.open, wash_hover(), line_strong(), ink_strong()),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.corner_radius = RADIUS;
        w.fg_stroke = Stroke::new(1.0, fg);
        w.expansion = 0.0;
    }
    v
}

/// Installs fonts, colours and spacing for the active colour mode.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let dark = mode() == Mode::Dark;
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.set_visuals_of(egui::Theme::Light, visuals());
    ctx.set_theme(egui::ThemePreference::Dark);
    let _ = dark;
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = CONTROL_H;
        style.spacing.menu_margin = egui::Margin::same(4);
        style.spacing.window_margin = egui::Margin::same(16);
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 8.0,
            floating: true,
            ..egui::style::ScrollStyle::thin()
        };
        style.text_styles = [
            (TextStyle::Heading, FontId::new(24.0, display())),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, medium())),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

type Face<'a> = (&'a str, &'static [u8], &'a [([u8; 4], f32)], FontFamily);

fn install_fonts(ctx: &egui::Context) {
    const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.ttf");
    const ARCHIVO: &[u8] = include_bytes!("../assets/fonts/Archivo-Variable.ttf");
    const GEIST_MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono-Variable.ttf");

    let mut fonts = FontDefinitions::default();
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let faces: [Face<'_>; 8] = [
        (
            "Inter",
            INTER,
            &[(*b"wght", 400.0), (*b"opsz", 14.0)],
            FontFamily::Proportional,
        ),
        (
            "Inter-Medium",
            INTER,
            &[(*b"wght", 500.0), (*b"opsz", 14.0)],
            medium(),
        ),
        (
            "Inter-SemiBold",
            INTER,
            &[(*b"wght", 600.0), (*b"opsz", 14.0)],
            semibold(),
        ),
        (
            "Inter-Display",
            INTER,
            &[(*b"wght", 700.0), (*b"opsz", 32.0)],
            FontFamily::Name("inter-display".into()),
        ),
        (
            "Archivo-Display",
            ARCHIVO,
            &[(*b"wght", 620.0), (*b"wdth", 118.0)],
            display(),
        ),
        (
            "GeistMono",
            GEIST_MONO,
            &[(*b"wght", 400.0)],
            FontFamily::Monospace,
        ),
        ("GeistMono-Medium", GEIST_MONO, &[(*b"wght", 500.0)], mono()),
        (
            "GeistMono-SemiBold",
            GEIST_MONO,
            &[(*b"wght", 650.0)],
            mono_bold(),
        ),
    ];

    for (name, bytes, coords, family) in faces {
        let tweak = FontTweak {
            coords: VariationCoords::new(coords.iter().copied()),
            ..Default::default()
        };
        fonts.font_data.insert(
            name.to_owned(),
            Arc::new(FontData::from_static(bytes).tweak(tweak)),
        );
        let mut stack = vec![name.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts.families.insert(family, stack);
    }

    // Display stack: Archivo for Latin display, then Inter-Display (wght 700, opsz 32)
    // for seamless Cyrillic headings, then Inter-SemiBold and Inter.
    let mut display_stack = vec![
        "Archivo-Display".to_string(),
        "Inter-Display".to_string(),
        "Inter-SemiBold".to_string(),
        "Inter".to_string(),
    ];
    display_stack.extend(fallback.iter().cloned());
    fonts.families.insert(display(), display_stack);

    // Monospace fallbacks: include Inter so non-ASCII glyphs do not fall through to system fonts.
    let mut mono_stack = vec!["GeistMono".to_string(), "Inter".to_string()];
    mono_stack.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Monospace, mono_stack);

    let mut mono_m_stack = vec![
        "GeistMono-Medium".to_string(),
        "GeistMono".to_string(),
        "Inter".to_string(),
    ];
    mono_m_stack.extend(fallback.iter().cloned());
    fonts.families.insert(mono(), mono_m_stack);

    let mut mono_b_stack = vec![
        "GeistMono-SemiBold".to_string(),
        "GeistMono".to_string(),
        "Inter".to_string(),
    ];
    mono_b_stack.extend(fallback.iter().cloned());
    fonts.families.insert(mono_bold(), mono_b_stack);

    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_the_default_mode() {
        set_mode(Mode::Dark);
        assert_eq!(mode(), Mode::Dark);
        assert_eq!(bg(), DARK_BG);
    }

    #[test]
    fn light_mode_swaps_the_surfaces() {
        set_mode(Mode::Light);
        assert_eq!(mode(), Mode::Light);
        assert_eq!(bg(), LIGHT_BG);
        // Paper is light, ink is dark — the two palettes must not collide.
        assert!(LIGHT_BG.r() > 200 && DARK_BG.r() < 40);
        assert!(LIGHT_INK.r() < 60 && DARK_INK.r() > 200);
        set_mode(Mode::Dark);
    }

    #[test]
    fn toggle_round_trips() {
        set_mode(Mode::Dark);
        assert_eq!(toggle(), Mode::Light);
        assert_eq!(toggle(), Mode::Dark);
    }
}
