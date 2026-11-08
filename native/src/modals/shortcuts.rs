//! Keyboard-shortcuts cheat sheet (Shift+/ or the palette command).
//!
//! One static table, four groups across two columns. Every binding listed
//! here lives in `frame_ui`; the sheet is documentation, not a source of
//! truth, so it must not invent rows.

use egui::{vec2, Align, Label, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::menu_header;
use crate::theme;

/// One row of the sheet: key combination + what it does.
type Row = (&'static str, Key);

const GENERAL: &[Row] = &[
    ("Ctrl K", Key::ShcPalette),
    ("/", Key::ShcSearch),
    ("Shift /", Key::ShcHelp),
    ("Ctrl Shift A", Key::ShcLocale),
    ("F1", Key::ShcFaq),
];

const NAVIGATION: &[Row] = &[
    ("J / K", Key::ShcNav),
    ("Alt ← / Alt →", Key::ShcForward),
    ("Ctrl 1…9", Key::ShcJump),
    ("Alt 1…6 · Ctrl Tab", Key::ShcViews),
    ("Ctrl F", Key::ShcSearch),
];

const PAGES: &[Row] = &[
    ("Ctrl N", Key::ShcNew),
    ("N", Key::ShcQuickCapture),
    ("Ctrl S", Key::ShcSave),
    ("Ctrl Z / Ctrl Y", Key::ShcUndo),
    ("Ctrl D", Key::ShcDuplicate),
    ("Ctrl P", Key::ShcPinRow),
    ("F2", Key::ShcRename),
    ("Delete", Key::ShcDelete),
    ("F5", Key::ShcReload),
    ("Ctrl E", Key::ShcExport),
    ("Ctrl Shift S", Key::ShcSync),
];

const VIEW: &[Row] = &[
    ("F11", Key::ShcZen),
    ("Ctrl \\", Key::ShcSidebar),
    ("Ctrl Shift W", Key::ShcWide),
    ("Ctrl + / − / 0", Key::ShcZoom),
    ("Ctrl Shift L", Key::ShcWrap),
    ("Ctrl Home / End", Key::ShcTopBottom),
];

fn group(ui: &mut Ui, title: Key, rows: &[Row], locale: Locale) {
    menu_header(ui, t(locale, title));
    for (keys, action) in rows {
        ui.horizontal(|ui| {
            ui.set_max_width(ui.available_width());
            ui.label(
                RichText::new(*keys)
                    .family(theme::mono_bold())
                    .size(11.5)
                    .color(theme::accent_ink()),
            );
            ui.add_space(10.0);
            ui.add(
                Label::new(
                    RichText::new(t(locale, *action))
                        .size(12.5)
                        .color(theme::muted()),
                )
                .wrap(),
            );
        });
        ui.add_space(4.0);
    }
}

pub fn draw_shortcuts_modal(show: bool, locale: Locale, ctx: &egui::Context) -> bool {
    if !show {
        return false;
    }

    let mut close = false;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }

    egui::Area::new(egui::Id::new("shortcuts-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            if resp.clicked() {
                close = true;
            }
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(700.0, 620.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::panel(),
                Stroke::new(1.0, theme::line_strong()),
                egui::StrokeKind::Inside,
            );

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(26.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            body.label(
                RichText::new(t(locale, Key::ShortcutsTitle))
                    .family(theme::medium())
                    .size(19.0)
                    .color(theme::ink()),
            );
            body.add_space(14.0);

            body.columns(2, |columns| {
                // Left: general + navigation. Right: pages + view.
                group(&mut columns[0], Key::ShortcutsGeneral, GENERAL, locale);
                columns[0].add_space(16.0);
                group(
                    &mut columns[0],
                    Key::ShortcutsNavigation,
                    NAVIGATION,
                    locale,
                );
                group(&mut columns[1], Key::ShortcutsPages, PAGES, locale);
                columns[1].add_space(16.0);
                group(&mut columns[1], Key::ShortcutsView, VIEW, locale);
            });
        });

    close
}
