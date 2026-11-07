//! Visual system for the native prototype.
//!
//! The reference is ArtCraft's native shell: near-black surfaces, white/15
//! hairlines, Archivo display type, Inter body text, Geist Mono HUD labels and
//! 3 px corners on anything pressable. The aim is a document app that feels
//! designed, not a default-egui form.
//!
//! Obsidian-style theming: a table of full presets (dark and light bases),
//! a user accent colour layered on top, an interface font scale, and all of
//! it persisted to `native-appearance.json` beside the locale setting.

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

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

/// One complete theme. Every surface, text and status colour is explicit;
/// the derived translucency slots (muted/dim/faint/lines/washes) are computed
/// from `ink` and `dark` so a new theme is a single short table row.
#[derive(Clone, Debug)]
pub struct ThemeDef {
    pub id: &'static str,
    pub label: &'static str,
    pub dark: bool,
    /// The page behind everything.
    pub bg: Color32,
    /// Menus, modals and the lightbox.
    pub panel: Color32,
    /// Toolbar buttons, menus and popovers.
    pub controls: Color32,
    /// The title bar and the sidebar.
    pub chrome: Color32,
    /// Wells and sunken inputs.
    pub sunken: Color32,
    /// Body text.
    pub ink: Color32,
    pub ink_strong: Color32,
    /// Sidebar links.
    pub sidebar_ink: Color32,
    /// Brand accent; the user override (see `accent()`) wins over this.
    pub accent: Color32,
    /// Readable companion of the accent for text on `bg`.
    pub accent_ink: Color32,
    pub invert_bg: Color32,
    pub invert_fg: Color32,
    pub bad: Color32,
    pub badge_beta: Color32,
    pub ok: Color32,
}

impl ThemeDef {
    fn derived(&self) -> Derived {
        let dark = self.dark;
        let line_fg = if dark { 0xffffff } else { 0x000000 };
        Derived {
            muted: alpha(self.ink_maybe_plain(), 153),
            dim: alpha(self.ink_maybe_plain(), 128),
            faint: alpha(self.ink_maybe_plain(), 102),
            line: alpha(line_fg, 38),
            line_strong: alpha(self.ink_maybe_plain(), 102),
            wash: alpha(line_fg, if dark { 13 } else { 8 }),
            wash_selected: alpha(line_fg, if dark { 20 } else { 12 }),
            wash_hover: alpha(line_fg, if dark { 26 } else { 16 }),
        }
    }

    /// `ink` may arrive translucent for sidebar use; plain colours derive the
    /// text-alpha slots. `ink` itself is always opaque in the tables below.
    fn ink_maybe_plain(&self) -> u32 {
        (self.ink.r() as u32) << 16 | (self.ink.g() as u32) << 8 | self.ink.b() as u32
    }
}

/// Translucency slots computed from the base table.
#[derive(Clone, Copy)]
struct Derived {
    muted: Color32,
    dim: Color32,
    faint: Color32,
    line: Color32,
    line_strong: Color32,
    wash: Color32,
    wash_selected: Color32,
    wash_hover: Color32,
}

#[derive(Clone, Copy)]
struct Row {
    id: &'static str,
    label: &'static str,
    dark: bool,
    bg: u32,
    panel: u32,
    controls: u32,
    chrome: u32,
    sunken: u32,
    ink: u32,
    ink_strong: u32,
    sidebar_ink: u32,
    sidebar_ink_alpha: u8,
    accent: u32,
    accent_ink: u32,
    invert_bg: u32,
    invert_fg: u32,
    bad: u32,
    badge_beta: u32,
    ok: u32,
}

const fn row(r: Row) -> ThemeDef {
    ThemeDef {
        id: r.id,
        label: r.label,
        dark: r.dark,
        bg: hex(r.bg),
        panel: hex(r.panel),
        controls: hex(r.controls),
        chrome: hex(r.chrome),
        sunken: hex(r.sunken),
        ink: hex(r.ink),
        ink_strong: hex(r.ink_strong),
        sidebar_ink: alpha(r.sidebar_ink, r.sidebar_ink_alpha),
        accent: hex(r.accent),
        accent_ink: hex(r.accent_ink),
        invert_bg: hex(r.invert_bg),
        invert_fg: hex(r.invert_fg),
        bad: hex(r.bad),
        badge_beta: hex(r.badge_beta),
        ok: hex(r.ok),
    }
}

/// The shipped themes. First entry is the default; ids are stable storage keys.
const ROWS: [Row; 8] = [
    // The original ArtCraft dark — app default.
    Row {
        id: "dark",
        label: "Graphite",
        dark: true,
        bg: 0x121316,
        panel: 0x101014,
        controls: 0x1e1f22,
        chrome: 0x0a0a0c,
        sunken: 0x060607,
        ink: 0xf2f1ee,
        ink_strong: 0xffffff,
        sidebar_ink: 0xffffff,
        sidebar_ink_alpha: 184,
        accent: 0x2d81ff,
        accent_ink: 0x74aaff,
        invert_bg: 0xffffff,
        invert_fg: 0x0b0b0c,
        bad: 0xf87171,
        badge_beta: 0xffc14d,
        ok: 0x4ade80,
    },
    // The paper light base.
    Row {
        id: "light",
        label: "Paper",
        dark: false,
        bg: 0xf7f6f3,
        panel: 0xffffff,
        controls: 0xefece7,
        chrome: 0xeae7e1,
        sunken: 0xe3e0da,
        ink: 0x1c1c1e,
        ink_strong: 0x000000,
        sidebar_ink: 0x141414,
        sidebar_ink_alpha: 196,
        accent: 0x2d81ff,
        accent_ink: 0x1c62d6,
        invert_bg: 0x0b0b0c,
        invert_fg: 0xffffff,
        bad: 0xd9534f,
        badge_beta: 0xb45309,
        ok: 0x15803d,
    },
    Row {
        id: "nord",
        label: "Nord",
        dark: true,
        bg: 0x2e3440,
        panel: 0x2b303b,
        controls: 0x3b4252,
        chrome: 0x242933,
        sunken: 0x232831,
        ink: 0xeceff4,
        ink_strong: 0xffffff,
        sidebar_ink: 0xd8dee9,
        sidebar_ink_alpha: 210,
        accent: 0x88c0d0,
        accent_ink: 0x8fbcbb,
        invert_bg: 0xeceff4,
        invert_fg: 0x2e3440,
        bad: 0xbf616a,
        badge_beta: 0xebcb8b,
        ok: 0xa3be8c,
    },
    Row {
        id: "gruvbox",
        label: "Gruvbox",
        dark: true,
        bg: 0x282828,
        panel: 0x1d2021,
        controls: 0x3c3836,
        chrome: 0x1b1b1b,
        sunken: 0x161616,
        ink: 0xebdbb2,
        ink_strong: 0xfbf1c7,
        sidebar_ink: 0xebdbb2,
        sidebar_ink_alpha: 200,
        accent: 0xfe8019,
        accent_ink: 0xffa566,
        invert_bg: 0xfbf1c7,
        invert_fg: 0x282828,
        bad: 0xfb4934,
        badge_beta: 0xfabd2f,
        ok: 0xb8bb26,
    },
    Row {
        id: "catppuccin",
        label: "Catppuccin",
        dark: true,
        bg: 0x1e1e2e,
        panel: 0x181825,
        controls: 0x313244,
        chrome: 0x11111b,
        sunken: 0x0e0e14,
        ink: 0xcdd6f4,
        ink_strong: 0xffffff,
        sidebar_ink: 0xcdd6f4,
        sidebar_ink_alpha: 200,
        accent: 0x89b4fa,
        accent_ink: 0xb4befe,
        invert_bg: 0xcdd6f4,
        invert_fg: 0x1e1e2e,
        bad: 0xf38ba8,
        badge_beta: 0xf9e2af,
        ok: 0xa6e3a1,
    },
    Row {
        id: "solarized",
        label: "Solarized",
        dark: false,
        bg: 0xfdf6e3,
        panel: 0xeee8d5,
        controls: 0xe4ddc8,
        chrome: 0xf3ecd9,
        sunken: 0xe3dcc6,
        ink: 0x073642,
        ink_strong: 0x002b36,
        sidebar_ink: 0x586e75,
        sidebar_ink_alpha: 220,
        accent: 0x268bd2,
        accent_ink: 0x1a6ea4,
        invert_bg: 0x073642,
        invert_fg: 0xfdf6e3,
        bad: 0xdc322f,
        badge_beta: 0xb58900,
        ok: 0x859900,
    },
    Row {
        id: "everforest",
        label: "Everforest",
        dark: true,
        bg: 0x2d353b,
        panel: 0x272e33,
        controls: 0x3a454a,
        chrome: 0x22282d,
        sunken: 0x1c2226,
        ink: 0xd3c6aa,
        ink_strong: 0xf2e5bc,
        sidebar_ink: 0xd3c6aa,
        sidebar_ink_alpha: 200,
        accent: 0xa7c080,
        accent_ink: 0xc0ca8d,
        invert_bg: 0xd3c6aa,
        invert_fg: 0x2d353b,
        bad: 0xe67e80,
        badge_beta: 0xdbbc7f,
        ok: 0x83c092,
    },
    Row {
        id: "rose",
        label: "Rosé Pine",
        dark: true,
        bg: 0x191724,
        panel: 0x1f1d2e,
        controls: 0x26233a,
        chrome: 0x15131f,
        sunken: 0x12101a,
        ink: 0xe0def4,
        ink_strong: 0xffffff,
        sidebar_ink: 0xe0def4,
        sidebar_ink_alpha: 200,
        accent: 0xebbcba,
        accent_ink: 0xf0c2c0,
        invert_bg: 0xe0def4,
        invert_fg: 0x191724,
        bad: 0xeb6f92,
        badge_beta: 0xf6c177,
        ok: 0x9ccfd8,
    },
];

fn presets() -> &'static [ThemeDef] {
    static PRESETS: std::sync::OnceLock<Vec<ThemeDef>> = std::sync::OnceLock::new();
    PRESETS.get_or_init(|| ROWS.iter().map(|r| row(*r)).collect())
}

pub fn preset_list() -> &'static [ThemeDef] {
    presets()
}

fn preset_index() -> usize {
    PRESET
        .load(std::sync::atomic::Ordering::Relaxed)
        .min((presets().len() - 1) as u8) as usize
}

pub fn preset() -> &'static ThemeDef {
    &presets()[preset_index()]
}

/// Switch to a preset by id; unknown ids keep the current theme.
pub fn set_preset(id: &str) {
    if let Some((index, _)) = presets().iter().enumerate().find(|(_, t)| t.id == id) {
        PRESET.store(index as u8, std::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Mode (base dark/light), accent override, font scale
// ---------------------------------------------------------------------------

/// Colour mode of the whole interface: which base the current preset has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

static PRESET: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static ACCENT: std::sync::RwLock<Option<Color32>> = std::sync::RwLock::new(None);
/// f32 bits of the interface font scale (1.0 = stock).
static FONT_SCALE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1.0f32.to_bits());

pub fn mode() -> Mode {
    if preset().dark {
        Mode::Dark
    } else {
        Mode::Light
    }
}

pub fn set_mode(m: Mode) {
    set_preset(if m == Mode::Light { "light" } else { "dark" });
}

/// Flips between the two base presets and returns the new mode.
pub fn toggle() -> Mode {
    let next = match mode() {
        Mode::Dark => Mode::Light,
        Mode::Light => Mode::Dark,
    };
    set_mode(next);
    next
}

/// The effective accent: the user pick when set, the preset accent otherwise.
pub fn accent() -> Color32 {
    accent_color()
}

pub fn accent_color() -> Color32 {
    match ACCENT.read() {
        Ok(guard) => guard.unwrap_or(preset().accent),
        Err(_) => preset().accent,
    }
}

/// `Some(colour)` layers a custom accent over every theme; `None` returns to
/// the preset accent.
pub fn set_accent(c: Option<Color32>) {
    if let Ok(mut guard) = ACCENT.write() {
        *guard = c;
    }
}

pub fn accent_ink() -> Color32 {
    let base = accent_color();
    match mode() {
        // On dark bases a lifted tint reads better than the raw accent.
        Mode::Dark => base.lerp_to_gamma(Color32::WHITE, 0.25),
        Mode::Light => base.lerp_to_gamma(Color32::BLACK, 0.18),
    }
}

pub fn font_scale() -> f32 {
    f32::from_bits(FONT_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_font_scale(scale: f32) {
    FONT_SCALE.store(
        scale.clamp(0.8, 1.4).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Remembers the appearance beside the locale setting.
pub fn persist() {
    let Some(dir) = crate::i18n::settings_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let accent = match ACCENT.read() {
        Ok(guard) => guard.map(|c| vec![c.r(), c.g(), c.b()]),
        Err(_) => None,
    };
    let doc = serde_json::json!({
        "theme": preset().id,
        "accent": accent,
        "font_scale": font_scale(),
    });
    let _ = std::fs::write(dir.join("native-appearance.json"), doc.to_string());
}

/// Restores the persisted appearance (called once at startup). Falls back to
/// the legacy `native-theme.txt` from the first theming iteration.
pub fn load() {
    let Some(dir) = crate::i18n::settings_dir() else {
        return;
    };
    let path = dir.join("native-appearance.json");
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(id) = doc["theme"].as_str() {
                set_preset(id);
            }
            if let Some(rgb) = doc["accent"].as_array() {
                let vals: Vec<u8> = rgb
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                if vals.len() == 3 {
                    set_accent(Some(Color32::from_rgb(vals[0], vals[1], vals[2])));
                }
            }
            if let Some(scale) = doc["font_scale"].as_f64() {
                set_font_scale(scale as f32);
            }
            return;
        }
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("native-theme.txt")) {
        if text.trim() == "light" {
            set_mode(Mode::Light);
        }
    }
}

// ---------------------------------------------------------------------------
// Colour accessors (stable API — the whole app reads through these)
// ---------------------------------------------------------------------------

/// The page behind everything.
pub fn bg() -> Color32 {
    preset().bg
}

/// Menus, modals and the lightbox.
pub fn panel() -> Color32 {
    preset().panel
}

/// Toolbar buttons, menus and popovers.
pub fn controls() -> Color32 {
    preset().controls
}

/// The title bar and the sidebar.
pub fn chrome() -> Color32 {
    preset().chrome
}

/// Wells and sunken inputs.
pub fn sunken() -> Color32 {
    preset().sunken
}

/// Body text.
pub fn ink() -> Color32 {
    preset().ink
}

pub fn ink_strong() -> Color32 {
    preset().ink_strong
}

/// Secondary text and placeholders.
pub fn muted() -> Color32 {
    preset().derived().muted
}

/// Idle toolbar icons.
pub fn dim() -> Color32 {
    preset().derived().dim
}

/// Counters and captions.
pub fn faint() -> Color32 {
    preset().derived().faint
}

/// Sidebar links.
pub fn sidebar_ink() -> Color32 {
    preset().sidebar_ink
}

/// Every hairline.
pub fn line() -> Color32 {
    preset().derived().line
}

pub fn line_strong() -> Color32 {
    preset().derived().line_strong
}

/// Quiet fills.
pub fn wash() -> Color32 {
    preset().derived().wash
}

/// The selected sidebar row.
pub fn wash_selected() -> Color32 {
    preset().derived().wash_selected
}

/// Hovered menu rows.
pub fn wash_hover() -> Color32 {
    preset().derived().wash_hover
}

pub fn invert_bg() -> Color32 {
    preset().invert_bg
}

pub fn invert_fg() -> Color32 {
    preset().invert_fg
}

pub fn bad() -> Color32 {
    preset().bad
}

pub fn badge_beta() -> Color32 {
    preset().badge_beta
}

/// Success state (installed components, finished syncs).
pub fn ok() -> Color32 {
    preset().ok
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
    v.selection.bg_fill = accent_color();
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.text_cursor.stroke = Stroke::new(1.5, Color32::WHITE);
    v.warn_fg_color = badge_beta();
    v.error_fg_color = bad();
    v.striped = false;
    for (w, fill, stroke, fg) in [
        (&mut v.widgets.noninteractive, controls(), line(), muted()),
        (&mut v.widgets.inactive, controls(), line(), ink()),
        (
            &mut v.widgets.hovered,
            wash_hover(),
            line_strong(),
            ink_strong(),
        ),
        (
            &mut v.widgets.active,
            wash_hover(),
            line_strong(),
            ink_strong(),
        ),
        (
            &mut v.widgets.open,
            wash_hover(),
            line_strong(),
            ink_strong(),
        ),
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

/// Installs fonts, colours and spacing for the active theme.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let visuals = visuals();
    ctx.set_visuals_of(egui::Theme::Dark, visuals.clone());
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = CONTROL_H;
        style.spacing.menu_margin = egui::Margin::same(4);
        style.spacing.window_margin = Margin_same();
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 8.0,
            floating: true,
            ..egui::style::ScrollStyle::thin()
        };
        let s = font_scale();
        style.text_styles = [
            (TextStyle::Heading, FontId::new(24.0 * s, display())),
            (
                TextStyle::Body,
                FontId::new(14.0 * s, FontFamily::Proportional),
            ),
            (TextStyle::Button, FontId::new(13.0 * s, medium())),
            (
                TextStyle::Small,
                FontId::new(12.0 * s, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0 * s, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// Keep the original 16 px margin literal in one place.
fn Margin_same() -> egui::Margin {
    egui::Margin::same(16)
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
    fn presets_are_unique_and_complete() {
        let mut ids: Vec<&str> = preset_list().iter().map(|t| t.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "preset ids must be unique");
        assert!(count >= 6, "a theme picker wants more than two presets");
        assert!(preset_list().iter().any(|t| t.dark));
        assert!(preset_list().iter().any(|t| !t.dark));
    }

    #[test]
    fn preset_switch_changes_every_surface() {
        set_preset("dark");
        let dark_bg = bg();
        let dark_accent = accent_color();
        set_preset("nord");
        assert_ne!(bg(), dark_bg);
        assert_ne!(accent_color(), dark_accent);
        assert_eq!(mode(), Mode::Dark);
        set_preset("solarized");
        assert_eq!(mode(), Mode::Light);
        set_preset("dark");
    }

    #[test]
    fn unknown_preset_id_keeps_the_current_theme() {
        set_preset("nord");
        set_preset("does-not-exist");
        assert_eq!(preset().id, "nord");
        set_preset("dark");
    }

    #[test]
    fn accent_override_wins_until_cleared() {
        set_accent(None);
        assert_eq!(accent_color(), preset().accent);
        set_accent(Some(hex(0xff5544)));
        assert_eq!(accent_color(), hex(0xff5544));
        // The override survives a theme switch (like Obsidian's accent).
        set_preset("nord");
        assert_eq!(accent_color(), hex(0xff5544));
        set_accent(None);
        assert_eq!(accent_color(), preset().accent);
        set_preset("dark");
    }

    #[test]
    fn font_scale_clamps_and_round_trips() {
        set_font_scale(1.0);
        assert!((font_scale() - 1.0).abs() < 1e-6);
        set_font_scale(9.9);
        assert!(font_scale() <= 1.4);
        set_font_scale(0.1);
        assert!(font_scale() >= 0.8);
        set_font_scale(1.0);
    }

    #[test]
    fn toggle_round_trips_between_bases() {
        set_preset("dark");
        assert_eq!(toggle(), Mode::Light);
        assert_eq!(preset().id, "light");
        assert_eq!(toggle(), Mode::Dark);
        assert_eq!(preset().id, "dark");
    }
}
