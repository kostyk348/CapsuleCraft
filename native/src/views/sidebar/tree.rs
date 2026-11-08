//! Hierarchy tree rendering and search results for the sidebar.

use std::collections::HashSet;

use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::Capsule;
use crate::search;
use crate::theme;
use crate::truncate_chars;
use crate::vault::PageTree;

use super::dnd::draw_drag_preview;

#[allow(clippy::too_many_arguments)]
pub fn draw_page_nodes(
    ui: &mut Ui,
    capsules: &[Capsule],
    tree: &PageTree,
    parent: Option<usize>,
    depth: usize,
    selected: Option<usize>,
    collapsed: &mut HashSet<String>,
    dragged_page: &mut Option<usize>,
    drop_target: &mut Option<Option<String>>,
    locale: Locale,
) -> Option<usize> {
    let mut clicked = None;
    for &index in tree.children_of(parent) {
        let capsule = &capsules[index];
        let has_children = !tree.children_of(Some(index)).is_empty();
        let key = capsule.message_id.clone();
        let is_collapsed = collapsed.contains(&key);
        let depth = depth.min(4);
        let indent = depth as f32 * 12.0;

        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click_and_drag());

        if response.drag_started() && capsule.native {
            *dragged_page = Some(index);
        }

        let is_selected = selected == Some(index);
        let is_dragged = *dragged_page == Some(index);

        let mut is_valid_drop_target = false;
        let mut is_cycle_target = false;
        if let Some(dragged) = *dragged_page {
            if dragged != index && response.hovered() {
                if tree.is_descendant_of(index, dragged) {
                    is_cycle_target = true;
                } else {
                    is_valid_drop_target = true;
                    *drop_target = Some(Some(capsule.message_id.clone()));
                }
            }
        }

        if is_dragged {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.0, theme::fade(theme::line(), 0.4)),
                egui::StrokeKind::Inside,
            );
        } else if is_valid_drop_target {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.5, theme::accent()),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::fade(theme::accent(), 0.15));
        } else if is_cycle_target {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.5, theme::bad()),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::fade(theme::bad(), 0.15));
        } else if is_selected {
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::wash_selected());
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(rect.left() - 7.0, rect.top() + 7.0), vec2(2.0, 16.0)),
                0.0,
                Color32::WHITE,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, theme::RADIUS, theme::wash());
        }

        let chevron_rect = Rect::from_center_size(
            pos2(rect.left() + indent + 8.0, rect.center().y),
            vec2(14.0, 14.0),
        );
        let mut toggle = false;
        if has_children {
            let chev = ui
                .interact(
                    chevron_rect,
                    ui.scope_id().with(("page-chevron", index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(chevron_rect.center(), vec2(10.0, 10.0)),
                if is_collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                },
                theme::faint(),
            );
            toggle = chev.clicked();
        }

        let ink = if is_selected || response.hovered() || is_valid_drop_target {
            Color32::WHITE
        } else if is_dragged {
            theme::faint()
        } else {
            theme::sidebar_ink()
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(
                pos2(rect.left() + indent + 25.0, rect.center().y),
                vec2(13.0, 13.0),
            ),
            Icon::Doc,
            ink,
        );

        let title_x = rect.left() + indent + 40.0;
        let title = if capsule.subject.trim().is_empty() {
            t(locale, Key::Untitled).to_string()
        } else {
            capsule.subject.clone()
        };
        let font = FontId::new(
            12.5,
            if is_selected {
                theme::medium()
            } else {
                FontFamily::Proportional
            },
        );
        let clip = Rect::from_min_max(
            pos2(title_x, rect.top()),
            pos2(rect.right() - 18.0, rect.bottom()),
        );
        ui.painter().with_clip_rect(clip).text(
            pos2(title_x, rect.center().y),
            Align2::LEFT_CENTER,
            &title,
            font,
            ink,
        );

        if !capsule.native {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(
                    pos2(rect.right() - 12.0, rect.center().y),
                    vec2(11.0, 11.0),
                ),
                Icon::Lock,
                theme::badge_beta(),
            );
        }

        if toggle {
            if is_collapsed {
                collapsed.remove(&key);
            } else {
                collapsed.insert(key.clone());
            }
        } else if response.clicked() && dragged_page.is_none() {
            clicked = Some(index);
        }

        if is_dragged {
            draw_drag_preview(ui, capsule);
        }

        let response = if is_cycle_target {
            response.on_hover_text(t(locale, Key::CannotDropInsideSelf))
        } else if is_valid_drop_target {
            response.on_hover_text(format!(
                "{} «{}»",
                t(locale, Key::MakeChildOf),
                capsule.subject
            ))
        } else if !capsule.native {
            response.on_hover_text(format!("{} (read-only)", capsule.subject))
        } else {
            response.on_hover_text(&capsule.subject)
        };
        let _ = response;

        if has_children && !is_collapsed {
            if let Some(child) = draw_page_nodes(
                ui,
                capsules,
                tree,
                Some(index),
                depth + 1,
                selected,
                collapsed,
                dragged_page,
                drop_target,
                locale,
            ) {
                clicked = Some(child);
            }
        }
    }
    clicked
}

pub fn draw_search_row(
    ui: &mut Ui,
    capsules: &[Capsule],
    tree: &PageTree,
    index: usize,
    selected: Option<usize>,
    score: f32,
    snippet: Option<&str>,
    query: &str,
    locale: Locale,
) -> bool {
    let capsule = &capsules[index];
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    let is_selected = selected == Some(index);
    if is_selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::wash_selected());
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(rect.left() - 7.0, rect.top() + 10.0), vec2(2.0, 18.0)),
            0.0,
            Color32::WHITE,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::wash());
    }
    let ink = if is_selected || response.hovered() {
        Color32::WHITE
    } else {
        theme::sidebar_ink()
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            pos2(rect.left() + 15.0, rect.top() + 16.0),
            vec2(13.0, 13.0),
        ),
        Icon::Doc,
        ink,
    );
    let title = if capsule.subject.trim().is_empty() {
        t(locale, Key::Untitled).to_string()
    } else {
        capsule.subject.clone()
    };
    let title_clip = Rect::from_min_max(
        pos2(rect.left() + 30.0, rect.top()),
        pos2(rect.right() - 34.0, rect.top() + 24.0),
    );
    ui.painter().with_clip_rect(title_clip).text(
        pos2(rect.left() + 30.0, rect.top() + 15.0),
        Align2::LEFT_CENTER,
        &title,
        FontId::new(12.5, theme::medium()),
        ink,
    );
    let second = match snippet {
        Some(snippet) => truncate_chars(snippet, 40),
        None => tree
            .parent_of(index)
            .and_then(|parent| capsules.get(parent))
            .map(|parent| format!("in {}", truncate_chars(&parent.subject, 26)))
            .unwrap_or_else(|| "Root page".to_string()),
    };
    let clip = Rect::from_min_max(
        pos2(rect.left() + 30.0, rect.top() + 22.0),
        pos2(rect.right() - 4.0, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(clip);
    let mut x = rect.left() + 30.0;
    let baseline = rect.top() + 30.0;
    for (segment, is_match) in search::highlight_matches(&second, query) {
        if segment.is_empty() {
            continue;
        }
        let color = if is_match {
            if is_selected || response.hovered() {
                Color32::WHITE
            } else {
                theme::ink()
            }
        } else {
            theme::faint()
        };
        let galley = painter.layout_no_wrap(segment, FontId::new(9.5, theme::mono()), color);
        let width = galley.size().x;
        if x > clip.right() {
            break;
        }
        painter.galley(pos2(x, baseline - galley.size().y / 2.0), galley, color);
        x += width;
    }
    ui.painter().text(
        pos2(rect.right() - 10.0, rect.top() + 15.0),
        Align2::RIGHT_CENTER,
        format!("{score:.1}"),
        FontId::new(9.0, theme::mono()),
        theme::faint(),
    );
    if !capsule.native {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(
                pos2(rect.right() - 12.0, rect.top() + 30.0),
                vec2(11.0, 11.0),
            ),
            Icon::Lock,
            theme::badge_beta(),
        );
    }
    let response = if capsule.native {
        response.on_hover_text(&capsule.subject)
    } else {
        response.on_hover_text(format!("{} (read-only)", capsule.subject))
    };
    response.clicked()
}
