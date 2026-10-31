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
pub const BG: Color32 = hex(0x121316);
/// Menus, modals and the lightbox.
pub const PANEL: Color32 = hex(0x101014);
/// Toolbar buttons, menus and popovers.
pub const CONTROLS: Color32 = hex(0x1e1f22);
/// The title bar and the sidebar.
pub const CHROME: Color32 = hex(0x0a0a0c);
/// Wells and sunken inputs.
pub const SUNKEN: Color32 = hex(0x060607);

/// Body text.
pub const INK: Color32 = hex(0xf2f1ee);
pub const INK_STRONG: Color32 = Color32::WHITE;
/// Secondary text and placeholders.
pub const MUTED: Color32 = alpha(0xf2f1ee, 153);
/// Idle toolbar icons.
pub const DIM: Color32 = alpha(0xf2f1ee, 128);
/// Counters and captions.
pub const FAINT: Color32 = alpha(0xf2f1ee, 102);
/// Sidebar links.
pub const SIDEBAR_INK: Color32 = alpha(0xffffff, 184);

/// Every hairline.
pub const LINE: Color32 = alpha(0xffffff, 38);
pub const LINE_STRONG: Color32 = alpha(0xf2f1ee, 102);
/// Quiet fills.
pub const WASH: Color32 = alpha(0xffffff, 13);
/// The selected sidebar row.
pub const WASH_SELECTED: Color32 = alpha(0xffffff, 20);
/// Hovered menu rows.
pub const WASH_HOVER: Color32 = alpha(0xffffff, 26);

/// Brand blue.
pub const ACCENT: Color32 = hex(0x2d81ff);
pub const ACCENT_INK: Color32 = hex(0x74aaff);

pub const INVERT_BG: Color32 = Color32::WHITE;
pub const INVERT_FG: Color32 = hex(0x0b0b0c);

pub const BAD: Color32 = hex(0xf87171);
pub const BADGE_BETA: Color32 = hex(0xffc14d);

/// 3 px: buttons, inputs, menus.
pub const RADIUS: CornerRadius = CornerRadius::same(3);

/// Toolbar button height.
pub const CONTROL_H: f32 = 30.0;

pub const HUD_SIZE: f32 = 9.5;
pub const HUD_SPACING: f32 = HUD_SIZE * 0.15;
pub const LABEL_SPACING: f32 = HUD_SIZE * 0.12;

pub fn hairline() -> Stroke {
    Stroke::new(1.0, LINE)
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
    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = SUNKEN;
    v.text_edit_bg_color = Some(Color32::TRANSPARENT);
    v.faint_bg_color = CONTROLS;
    v.code_bg_color = SUNKEN;
    v.window_stroke = hairline();
    v.window_corner_radius = RADIUS;
    v.menu_corner_radius = RADIUS;
    v.window_shadow = shadow();
    v.popup_shadow = shadow();
    v.override_text_color = Some(INK);
    v.hyperlink_color = ACCENT_INK;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.text_cursor.stroke = Stroke::new(1.5, Color32::WHITE);
    v.warn_fg_color = BADGE_BETA;
    v.error_fg_color = BAD;
    v.striped = false;
    for (w, fill, stroke, fg) in [
        (&mut v.widgets.noninteractive, CONTROLS, LINE, MUTED),
        (&mut v.widgets.inactive, CONTROLS, LINE, INK),
        (&mut v.widgets.hovered, WASH_HOVER, LINE_STRONG, INK_STRONG),
        (&mut v.widgets.active, WASH_HOVER, LINE_STRONG, INK_STRONG),
        (&mut v.widgets.open, WASH_HOVER, LINE_STRONG, INK_STRONG),
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

/// Installs fonts, colours and spacing. The app is dark only.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.set_theme(egui::ThemePreference::Dark);
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
