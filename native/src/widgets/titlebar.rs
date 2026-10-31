use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, Response, RichText, Sense, Stroke,
    Ui, UiBuilder,
};

use crate::brand;
use crate::caps;
use crate::chevron;
use crate::chrome;
use crate::crumb_button;
use crate::ghost;
use crate::hud;
use crate::i18n::{t, Key, Locale};
use crate::icon_button;
use crate::icons::{self, Icon};
use crate::info_popover;
use crate::mime::Capsule;
use crate::popover_frame;
use crate::sync_imap;
use crate::theme;
use crate::truncate_chars;
use crate::vault::Vault;
use crate::views::{self, EditorAction, View};
use crate::ShellAction;
pub fn draw_titlebar(
    ui: &mut Ui,
    vault: &Vault,
    dirty: bool,
    status: &str,
    status_error: bool,
    imap_config: &sync_imap::ImapConfig,
    imap_status: &sync_imap::SyncStatus,
    locale: Locale,
    sidebar_collapsed: bool,
    zen_mode: bool,
    icon_style: brand::BrandIconStyle,
) -> ShellAction {
    let mut action = ShellAction::default();

    let total_rect = ui.max_rect();

    // 1. Right utility controls: rendered first with right-to-left layout so they
    // are guaranteed visible, anchored, and have strict layout boundaries.
    let right_width = if chrome::custom_caption_buttons() {
        450.0
    } else {
        340.0
    };
    let right_rect = Rect::from_min_max(
        pos2(
            (total_rect.right() - right_width).max(total_rect.left()),
            total_rect.top(),
        ),
        total_rect.right_bottom(),
    );
    let mut right_bar = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let right_ui = &mut right_bar;
    right_ui.spacing_mut().item_spacing.x = 2.0;
    if chrome::custom_caption_buttons() {
        chrome::caption_buttons(right_ui, chrome::TITLEBAR_HEIGHT);
    }
    right_ui.add_space(6.0);

    if zen_mode {
        if ghost(
            right_ui,
            Icon::Close,
            Some("Zen"),
            t(locale, Key::ZenModeEntered),
            true,
            26.0,
        )
        .clicked()
        {
            action.toggle_zen = true;
        }
        right_ui.add_space(4.0);
    }

    if icon_button(
        right_ui,
        Icon::Info,
        &format!("{} (F1)", t(locale, Key::FaqTitle)),
        true,
        false,
    )
    .clicked()
    {
        action.faq = true;
    }
    if icon_button(
        right_ui,
        Icon::Command,
        &format!("{} (Ctrl+K)", t(locale, Key::PaletteHint)),
        true,
        false,
    )
    .clicked()
    {
        action.palette = true;
    }
    if icon_button(
        right_ui,
        Icon::Code,
        t(locale, Key::ConnectMcp),
        true,
        false,
    )
    .clicked()
    {
        action.connect_mcp = true;
    }
    let (imap_color, imap_hint) = if !imap_config.enabled {
        (theme::MUTED, t(locale, Key::ImapDisabled))
    } else {
        match imap_status {
            sync_imap::SyncStatus::Syncing => (theme::BADGE_BETA, t(locale, Key::ImapSyncing)),
            sync_imap::SyncStatus::Error(_) => (theme::BAD, t(locale, Key::ImapErrorToast)),
            _ => (theme::ACCENT, t(locale, Key::ImapSynced)),
        }
    };
    let imap_resp = icon_button(right_ui, Icon::Mail, imap_hint, true, false);
    // Draw status dot on the IMAP icon
    let dot_pos = pos2(imap_resp.rect.left() + 7.0, imap_resp.rect.top() + 7.0);
    right_ui.painter().circle_filled(dot_pos, 2.5, imap_color);
    if imap_resp.clicked() {
        action.open_imap = true;
    }
    if ghost(
        right_ui,
        Icon::Globe,
        Some(locale.label()),
        t(locale, Key::CommandLocale),
        true,
        26.0,
    )
    .clicked()
    {
        action.toggle_locale = true;
    }
    right_ui.add_space(4.0);

    if icon_button(right_ui, Icon::Reload, t(locale, Key::Reload), true, false).clicked() {
        action.reload = true;
    }
    if icon_button(
        right_ui,
        Icon::Save,
        if dirty {
            t(locale, Key::SaveNow)
        } else {
            t(locale, Key::AllSaved)
        },
        dirty,
        false,
    )
    .clicked()
    {
        action.save = true;
    }

    right_ui.add_space(6.0);
    let (dot_color, text) = if status_error {
        (theme::BAD, t(locale, Key::SaveFailed))
    } else if dirty {
        (theme::BADGE_BETA, t(locale, Key::Unsaved))
    } else {
        (theme::ACCENT, status)
    };
    right_ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 2.5, dot_color);
        if total_rect.width() >= 1150.0 || status_error || dirty {
            let label = ui.label(RichText::new(text).family(theme::mono()).size(9.5).color(
                if status_error {
                    theme::BAD
                } else if dirty {
                    theme::BADGE_BETA
                } else {
                    theme::FAINT
                },
            ));
            if status_error {
                label.on_hover_text(status);
            }
        }
    });

    let right_used_left = right_ui.min_rect().left().min(total_rect.right() - 20.0);

    // 2. Left side: Sidebar toggle, Brand, and Vault switcher.
    // Constrained so it never extends past `right_used_left`.
    let left_rect = Rect::from_min_max(
        total_rect.left_top(),
        pos2(
            (right_used_left - 12.0).max(total_rect.left()),
            total_rect.bottom(),
        ),
    );
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui_left = &mut bar;
    ui_left.spacing_mut().item_spacing.x = 6.0;
    ui_left.add_space(chrome::leading_inset() + 12.0);

    let sidebar_hint = format!("{} (Ctrl + \\)", t(locale, Key::CommandToggleSidebar));
    if icon_button(
        ui_left,
        Icon::Sidebar,
        &sidebar_hint,
        true,
        !sidebar_collapsed,
    )
    .clicked()
    {
        action.toggle_sidebar = true;
    }

    ui_left.add_space(4.0);
    let (divider, _) = ui_left.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui_left
        .painter()
        .vline(divider.center().x, divider.y_range(), theme::hairline());
    ui_left.add_space(6.0);

    // Mark: clickable to toggle brand icon style (Mascot / Minimal Document)
    let (mark, mark_resp) = ui_left.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let mark_resp = mark_resp.on_hover_text(t(locale, Key::CommandToggleIconStyle));
    if mark_resp.clicked() {
        action.toggle_icon_style = true;
    }
    let mark_rect = Rect::from_center_size(mark.center(), vec2(20.0, 20.0));
    if let Some(texture) = brand::texture(ui_left.ctx(), icon_style) {
        ui_left.painter().image(
            texture.id(),
            mark_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        // Vector fallback: two dots in a capsule outline.
        ui_left.painter().rect_stroke(
            mark_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        ui_left
            .painter()
            .circle_filled(mark_rect.center() + vec2(-3.2, 0.0), 2.0, theme::ACCENT);
        ui_left.painter().circle_filled(
            mark_rect.center() + vec2(3.2, 0.0),
            2.0,
            theme::fade(theme::ACCENT, 0.35),
        );
    }

    ui_left.label(
        RichText::new(t(locale, Key::AppName))
            .family(theme::display())
            .size(15.0)
            .color(theme::INK),
    );

    let (badge_rect, _) = ui_left.allocate_exact_size(vec2(45.0, 16.0), Sense::hover());
    ui_left.painter().rect(
        badge_rect,
        theme::RADIUS,
        theme::WASH,
        Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let badge = caps(
        ui_left,
        "NATIVE",
        8.0,
        theme::mono_bold(),
        1.0,
        theme::FAINT,
    );
    ui_left.painter().galley(
        badge_rect.center() - badge.size() / 2.0,
        badge,
        theme::FAINT,
    );

    ui_left.add_space(8.0);
    let (divider, _) = ui_left.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui_left
        .painter()
        .vline(divider.center().x, divider.y_range(), theme::hairline());
    ui_left.add_space(6.0);

    if views::vault_switcher(ui_left, vault).clicked() {
        action.open_vault = true;
    }

    // 3. Middle drag area: only the empty space between the left and right groups
    // acts as window drag handle, preventing any click collisions.
    let left_used_right = ui_left.min_rect().right();
    let drag_rect = Rect::from_min_max(
        pos2(left_used_right + 4.0, total_rect.top()),
        pos2(right_used_left - 4.0, total_rect.bottom()),
    );
    if drag_rect.is_positive() && drag_rect.width() > 10.0 {
        chrome::title_bar_drag_rect(ui, drag_rect);
    }

    ui.painter().hline(
        total_rect.x_range(),
        total_rect.bottom() - 0.5,
        theme::hairline(),
    );

    action
}

// ---------------------------------------------------------------------------
// Workspace Strip: tier 2 toolbar with views tabs and contextual actions
// ---------------------------------------------------------------------------

pub fn draw_workspace_strip(
    ui: &mut Ui,
    view: View,
    locale: Locale,
    selected: Option<usize>,
    capsules: &[Capsule],
    wide_canvas: bool,
) -> (Option<View>, EditorAction) {
    let mut chosen_view = None;
    let mut editor_action = EditorAction::default();

    let total_rect = ui.max_rect();

    // 1. Right context actions
    let right_width = 400.0;
    let right_rect = Rect::from_min_max(
        pos2(
            (total_rect.right() - right_width).max(total_rect.left()),
            total_rect.top(),
        ),
        total_rect.right_bottom(),
    );
    let mut right_bar = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let right_ui = &mut right_bar;
    right_ui.spacing_mut().item_spacing.x = 4.0;
    right_ui.add_space(12.0);

    if view == View::Page {
        if let Some(index) = selected.filter(|i| *i < capsules.len()) {
            let capsule = &capsules[index];

            let info = ghost(
                right_ui,
                Icon::Info,
                Some(t(locale, Key::Info)),
                t(locale, Key::MimeAndPage),
                true,
                26.0,
            );
            egui::Popup::menu(&info)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_END)
                .show(|ui| info_popover(ui, capsule));

            if ghost(
                right_ui,
                Icon::Plus,
                Some(t(locale, Key::NewSubpage)),
                t(locale, Key::NewSubpageHint),
                true,
                26.0,
            )
            .clicked()
            {
                editor_action.new_subpage = true;
            }

            let wide_hint = format!("{} (Ctrl+Shift+W)", t(locale, Key::CommandWideCanvas));
            if ghost(
                right_ui,
                Icon::Table,
                Some(if wide_canvas { "Wide" } else { "Standard" }),
                &wide_hint,
                true,
                26.0,
            )
            .clicked()
            {
                editor_action.toggle_wide = true;
            }

            if ghost(
                right_ui,
                Icon::Command,
                Some(t(locale, Key::CoThink)),
                t(locale, Key::CoThinkHint),
                true,
                26.0,
            )
            .clicked()
            {
                editor_action.cothink = true;
            }
        }
    }

    let right_used_left = right_ui.min_rect().left().min(total_rect.right() - 12.0);

    // 2. Left side: The 6 Navigation tabs
    let left_rect = Rect::from_min_max(
        pos2(total_rect.left(), total_rect.top()),
        pos2(
            (right_used_left - 8.0).max(total_rect.left()),
            total_rect.bottom(),
        ),
    );
    let mut left_bar = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let left_ui = &mut left_bar;
    left_ui.spacing_mut().item_spacing.x = 4.0;
    left_ui.add_space(12.0);

    if let Some(chosen) = views::nav(left_ui, view, locale) {
        chosen_view = Some(chosen);
    }

    // Bottom hairline separator
    ui.painter().hline(
        total_rect.x_range(),
        total_rect.bottom() - 0.5,
        theme::hairline(),
    );

    (chosen_view, editor_action)
}

pub fn draw_breadcrumbs(
    ui: &mut Ui,
    ancestors: &[(usize, String)],
    locale: Locale,
    action: &mut EditorAction,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let _ = hud(ui, t(locale, Key::Vault), theme::MUTED);

        let skip = ancestors.len().saturating_sub(4);
        if skip > 0 {
            ui.label(RichText::new("…").size(11.5).color(theme::FAINT));
            let _ = chevron(ui, theme::FAINT);
        }
        for (ancestor_idx, subject) in &ancestors[skip..] {
            let label = if subject.is_empty() {
                t(locale, Key::Untitled).to_string()
            } else {
                subject.clone()
            };
            if crumb_button(ui, &truncate_chars(&label, 24)).clicked() {
                action.select = Some(*ancestor_idx);
            }
            let _ = chevron(ui, theme::FAINT);
        }
    });
}
