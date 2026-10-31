//! Workspace views: the page editor lives in `main.rs`; everything else is a
//! module here. Each view draws inside the content card and reports what the
//! user did, so the shell stays the only owner of the vault and app state.

pub mod document_view;
pub mod graph_view;
pub mod mail_view;
pub mod mime_view;
pub mod palette;
pub mod planning_view;
pub mod sidebar_view;
pub mod threads_view;

pub use document_view::{draw_document, draw_empty_state, EditorAction, LinkContext};
pub use sidebar_view::{draw_sidebar, vault_switcher};

use egui::{
    pos2, vec2, Align, Color32, FontId, Frame, Layout, Margin, Response, RichText, Sense, Ui,
};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Page,
    Planning,
    Graph,
    Threads,
    Mime,
    Mail,
    Faq,
}

impl View {
    pub fn all() -> [View; 6] {
        [
            View::Page,
            View::Planning,
            View::Graph,
            View::Threads,
            View::Mime,
            View::Mail,
        ]
    }
    pub fn key(self) -> Key {
        match self {
            View::Page => Key::NavPage,
            View::Planning => Key::NavPlanning,
            View::Graph => Key::NavGraph,
            View::Threads => Key::NavThreads,
            View::Mime => Key::NavMime,
            View::Mail => Key::NavPreview,
            View::Faq => Key::Faq,
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            View::Page => Icon::Doc,
            View::Planning => Icon::Check,
            View::Graph => Icon::Graph,
            View::Threads => Icon::Mail,
            View::Mime => Icon::Code,
            View::Mail => Icon::Mail,
            View::Faq => Icon::Info,
        }
    }

    pub fn title(self, locale: Locale) -> &'static str {
        t(locale, self.key())
    }
}

/// What a view asks the shell to do after drawing.
#[derive(Default)]
pub struct ViewAction {
    pub select: Option<usize>,
    /// Page to select by Message-ID; the shell resolves it to an index.
    pub select_id: Option<String>,
    pub changed: bool,
    pub view: Option<View>,
    pub toast: Option<String>,
    pub request_palette: bool,
    pub request_save: bool,
}

impl ViewAction {
    pub fn absorb(&mut self, other: ViewAction) {
        self.select = other.select.or(self.select);
        self.select_id = other.select_id.or_else(|| self.select_id.take());
        self.changed |= other.changed;
        self.view = other.view.or(self.view);
        self.toast = other.toast.or_else(|| self.toast.take());
        self.request_palette |= other.request_palette;
        self.request_save |= other.request_save;
    }
}

/// Segmented view switcher shown in the title bar.
pub fn nav(ui: &mut Ui, current: View, locale: Locale) -> Option<View> {
    let mut chosen = None;
    let available_w = ui.available_width();
    // Compact mode when space is limited: only the active tab displays text,
    // inactive tabs render icon-only to avoid colliding with right titlebar controls.
    let compact = available_w < 430.0;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for view in View::all() {
            let label = view.title(locale);
            let active = view == current;
            let show_text = !compact || active;

            let (width, galley) = if show_text {
                let galley = ui.painter().layout_no_wrap(
                    label.to_string(),
                    FontId::new(11.5, theme::medium()),
                    theme::MUTED,
                );
                let w = 9.0 + 13.0 + 6.0 + galley.size().x + 9.0;
                (w, Some(galley))
            } else {
                (28.0, None)
            };

            let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
            if active {
                ui.painter().rect(
                    rect,
                    theme::RADIUS,
                    theme::WASH_HOVER,
                    theme::hairline(),
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
            }
            let ink = if active {
                theme::INK
            } else if response.hovered() {
                theme::INK
            } else {
                theme::MUTED
            };

            let icon_center = if show_text {
                pos2(rect.left() + 9.0 + 6.5, rect.center().y)
            } else {
                rect.center()
            };
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(icon_center, vec2(13.0, 13.0)),
                view.icon(),
                ink,
            );

            if let Some(galley) = galley {
                ui.painter().galley_with_override_text_color(
                    pos2(
                        rect.left() + 9.0 + 13.0 + 6.0,
                        rect.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    ink,
                );
            }

            if response.clicked() {
                chosen = Some(view);
            }
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(label);
        }
    });
    chosen
}

// ---------------------------------------------------------------------------
// Shared view chrome
// ---------------------------------------------------------------------------

pub const VIEW_PAD: f32 = 28.0;

/// Standard view header: title, subtitle, right-aligned controls.
pub fn header(ui: &mut Ui, title: &str, subtitle: &str) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(VIEW_PAD, 0.0))));
    bar.vertical(|ui| {
        ui.add_space(13.0);
        ui.horizontal(|ui| {
            ui.add_space(0.0);
            ui.label(
                RichText::new(title)
                    .family(theme::display())
                    .size(20.0)
                    .color(theme::INK),
            );
            ui.add_space(10.0);
            if !subtitle.is_empty() {
                ui.label(
                    RichText::new(subtitle)
                        .family(theme::mono())
                        .size(9.5)
                        .color(theme::FAINT),
                );
            }
        });
    });
    rect
}

pub fn toolbar(ui: &mut Ui) -> (Response, egui::Rect) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    (response, rect)
}

/// Segmented control: `[label, label, …]`, returns the clicked one.
pub fn segments(ui: &mut Ui, options: &[(&str, bool)]) -> Option<usize> {
    let mut clicked = None;
    let (rect, _) = ui.allocate_exact_size(
        vec2(
            options
                .iter()
                .map(|(label, _)| label.chars().count() as f32 * 6.6 + 26.0)
                .sum::<f32>()
                + 2.0,
            26.0,
        ),
        Sense::hover(),
    );
    ui.painter().rect(
        rect,
        theme::RADIUS,
        theme::SUNKEN,
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 1.0;
    for (index, (label, active)) in options.iter().enumerate() {
        let width = label.chars().count() as f32 * 6.6 + 26.0;
        let item = egui::Rect::from_min_size(pos2(x, rect.top() + 1.0), vec2(width, 24.0));
        let response = ui.interact(item, ui.scope_id().with(("segment", index)), Sense::click());
        if *active {
            ui.painter()
                .rect_filled(item, theme::RADIUS, theme::WASH_HOVER);
        } else if response.hovered() {
            ui.painter().rect_filled(item, theme::RADIUS, theme::WASH);
        }
        ui.painter().text(
            item.center(),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::new(11.0, theme::medium()),
            if *active { theme::INK } else { theme::MUTED },
        );
        if response.clicked() {
            clicked = Some(index);
        }
        x += width;
    }
    clicked
}

/// Small statistic block: big number over a caps label.
///
/// The box is measured from the label so several stats can sit side by side in
/// a right-to-left toolbar without the first one swallowing the whole row.
pub fn stat(ui: &mut Ui, value: &str, label: &str, color: Color32) {
    let width = (label.chars().count() as f32 * 6.2).max(48.0) + 10.0;
    ui.allocate_ui_with_layout(vec2(width, 32.0), Layout::top_down(Align::Max), |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(
            RichText::new(value)
                .family(theme::display())
                .size(20.0)
                .color(color),
        );
        let _ = crate::hud(ui, label, theme::FAINT);
    });
}

/// Quiet empty state used by every view.
pub fn empty_state(ui: &mut Ui, icon: Icon, title: &str, body: &str) {
    ui.add_space(90.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
        ui.painter().rect(
            rect,
            theme::RADIUS,
            theme::WASH,
            theme::hairline(),
            egui::StrokeKind::Inside,
        );
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), vec2(22.0, 22.0)),
            icon,
            theme::FAINT,
        );
        ui.add_space(14.0);
        ui.label(
            RichText::new(title)
                .family(theme::medium())
                .size(15.0)
                .color(theme::INK),
        );
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(body).size(12.5).color(theme::MUTED)).wrap());
        ui.add_space(4.0);
    });
}

/// Card surface used for lists and tiles inside a view.
pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> Response {
    Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .show(ui, add)
        .response
}

/// Truncate by characters with an ellipsis.
pub fn ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A caps section label with a hairline underneath.
pub fn section(ui: &mut Ui, label: &str) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let galley = crate::caps(
        ui,
        label,
        theme::HUD_SIZE,
        theme::mono_bold(),
        theme::LABEL_SPACING,
        theme::FAINT,
    );
    ui.painter().galley(
        pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        theme::FAINT,
    );
    ui.painter()
        .hline(rect.x_range(), rect.bottom(), theme::hairline());
    ui.add_space(6.0);
}

/// Two-column scroll body helper: content is centred and width-limited.
pub fn centred<R>(ui: &mut Ui, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let available = ui.available_width();
    let width = max_width.min(available - VIEW_PAD * 2.0).max(320.0);
    let pad = ((available - width) / 2.0).max(VIEW_PAD);
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(width);
            add(ui)
        })
        .inner
    })
    .inner
}
