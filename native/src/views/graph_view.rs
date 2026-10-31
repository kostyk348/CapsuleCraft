//! Relationship graph view: zoomable canvas over page parents, replies,
//! references and tag clusters.
//!
//! Ported from `src/components/GraphView.tsx`. The web version leans on
//! d3-force and D3's zoom behaviour; here the layout is computed by
//! `crate::graph` and the view only paints + hit-tests.

use egui::{pos2, vec2, Align, FontId, Layout, Rect, RichText, Sense, Stroke, Ui};

use crate::graph::{self, EdgeKind};
use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mime::Capsule;
use crate::theme;
use crate::{ghost, hud, menu_row, popover_frame, tag_chip, truncate_chars};

use super::{empty_state, header, ViewAction, VIEW_PAD};

#[derive(Default)]
pub struct GraphState {
    pub tag: Option<String>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub edges: bool,
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            tag: None,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            edges: true,
        }
    }
}

pub fn draw(
    ui: &mut Ui,
    capsules: &[Capsule],
    state: &mut GraphState,
    locale: Locale,
    selected: Option<usize>,
) -> ViewAction {
    let mut action = ViewAction::default();
    let graph = graph::build(capsules, state.tag.as_deref());

    header(
        ui,
        t(locale, Key::GraphTitle),
        &format!(
            "{} {} · {} {}",
            graph.nodes.len(),
            t(locale, Key::Nodes),
            graph.edges.len(),
            t(locale, Key::Edges)
        ),
    );

    let (_, bar_rect) = super::toolbar(ui);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(VIEW_PAD, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let label = state
        .tag
        .clone()
        .unwrap_or_else(|| t(locale, Key::All).to_string());
    let tag_button = ghost(
        &mut bar,
        Icon::Tag,
        Some(&label),
        t(locale, Key::TagFilter),
        true,
        26.0,
    );
    egui::Popup::menu(&tag_button)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(180.0);
            if menu_row(
                ui,
                None,
                t(locale, Key::All),
                None,
                state.tag.is_none(),
                true,
                false,
            )
            .clicked()
            {
                state.tag = None;
            }
            for tag in &graph.tags {
                if menu_row(
                    ui,
                    Some(Icon::Tag),
                    tag,
                    None,
                    state.tag.as_deref() == Some(tag.as_str()),
                    true,
                    false,
                )
                .clicked()
                {
                    state.tag = Some(tag.clone());
                }
            }
        });

    if ghost(
        &mut bar,
        if state.edges {
            Icon::Check
        } else {
            Icon::Close
        },
        Some(if state.edges { "edges on" } else { "edges off" }),
        "Toggle relation edges",
        true,
        26.0,
    )
    .clicked()
    {
        state.edges = !state.edges;
    }

    bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ghost(
            ui,
            Icon::Reload,
            Some(t(locale, Key::Fit)),
            t(locale, Key::GraphHint),
            true,
            26.0,
        )
        .clicked()
        {
            state.pan = egui::Vec2::ZERO;
            state.zoom = 1.0;
        }
        ui.add_space(8.0);
        let _ = hud(ui, t(locale, Key::GraphHint), theme::FAINT);
    });

    if graph.nodes.is_empty() {
        empty_state(
            ui,
            Icon::Graph,
            t(locale, Key::GraphTitle),
            t(locale, Key::EmptyVaultBody),
        );
        return action;
    }

    // Canvas area.
    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), ui.cursor().top()),
        ui.max_rect().right_bottom(),
    );
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // Pan + zoom.
    if response.dragged() {
        state.pan += response.drag_delta();
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            let factor = (scroll * 0.0015).exp();
            state.zoom = (state.zoom * factor).clamp(0.35, 3.0);
        }
    }

    let centre = rect.center() + state.pan;
    let radius = (rect.width().min(rect.height()) * 0.40) * state.zoom;
    let to_screen = |pos: [f32; 2]| -> egui::Pos2 {
        pos2(centre.x + pos[0] * radius, centre.y + pos[1] * radius)
    };

    let positions: std::collections::HashMap<usize, egui::Pos2> = graph
        .nodes
        .iter()
        .map(|node| (node.capsule, to_screen(node.pos)))
        .collect();

    painter.rect_filled(rect, 0.0, theme::BG);

    if state.edges {
        for edge in &graph.edges {
            let (Some(a), Some(b)) = (positions.get(&edge.source), positions.get(&edge.target))
            else {
                continue;
            };
            let color = match edge.kind {
                EdgeKind::PageParent => theme::fade(theme::ACCENT, 0.55),
                EdgeKind::Reply => theme::fade(theme::INK, 0.3),
                EdgeKind::Reference => theme::fade(theme::INK, 0.18),
                EdgeKind::TagCluster => theme::fade(theme::ACCENT_INK, 0.12),
            };
            let width = if graph.nodes.len() > 40 { 1.0 } else { 1.2 };
            painter.line_segment([*a, *b], Stroke::new(width, color));
        }
    }

    let mut hovered: Option<usize> = None;
    let pointer = response.hover_pos();
    for node in &graph.nodes {
        let pos = positions[&node.capsule];
        let capsule = &capsules[node.capsule];
        let node_radius = (4.5 + (node.degree as f32).sqrt() * 1.8).clamp(4.5, 12.0) * state.zoom;
        if let Some(pointer) = pointer {
            if pointer.distance(pos) <= node_radius + 5.0 {
                hovered = Some(node.capsule);
            }
        }
        let is_selected = selected == Some(node.capsule);
        let color = if capsule.native {
            if is_selected {
                theme::ACCENT_INK
            } else {
                theme::ACCENT
            }
        } else if is_selected {
            theme::BADGE_BETA
        } else {
            theme::MUTED
        };
        if is_selected || hovered == Some(node.capsule) {
            painter.circle_filled(pos, node_radius + 4.0, theme::fade(color, 0.18));
        }
        painter.circle_filled(pos, node_radius, color);
        painter.circle_stroke(
            pos,
            node_radius,
            Stroke::new(1.0, theme::fade(theme::BG, 0.8)),
        );
    }

    // Labels: a graph of anonymous dots is not a relationship graph. They stay
    // on above 0.55 zoom and thin out below that instead of turning into noise.
    if state.zoom >= 0.55 {
        let size = (10.5 * state.zoom.clamp(0.85, 1.1)).clamp(9.0, 12.0);
        for node in &graph.nodes {
            if hovered == Some(node.capsule) {
                continue; // the tooltip already names it
            }
            let pos = positions[&node.capsule];
            let node_radius =
                (4.5 + (node.degree as f32).sqrt() * 1.8).clamp(4.5, 12.0) * state.zoom;
            let capsule = &capsules[node.capsule];
            let label = if capsule.subject.is_empty() {
                t(locale, Key::Untitled).to_string()
            } else {
                truncate_chars(&capsule.subject, 18)
            };
            painter.text(
                pos2(pos.x, pos.y + node_radius + 6.0),
                egui::Align2::CENTER_TOP,
                label,
                FontId::new(size, egui::FontFamily::Proportional),
                if selected == Some(node.capsule) {
                    theme::INK
                } else {
                    theme::fade(theme::MUTED, 0.9)
                },
            );
        }
    }

    if let Some(index) = hovered {
        let capsule = &capsules[index];
        let anchor = positions[&index];
        let label = truncate_chars(&capsule.subject, 40);
        let galley = painter.layout_no_wrap(label, FontId::new(11.5, theme::medium()), theme::INK);
        let box_rect = Rect::from_min_size(
            pos2(anchor.x + 12.0, anchor.y - galley.size().y / 2.0 - 6.0),
            galley.size() + vec2(16.0, 12.0),
        );
        painter.rect(
            box_rect,
            theme::RADIUS,
            theme::PANEL,
            theme::hairline(),
            egui::StrokeKind::Inside,
        );
        painter.galley(box_rect.min + vec2(8.0, 6.0), galley, theme::INK);
        if response.clicked() {
            action.select = Some(index);
        }
        if response.double_clicked() {
            action.select = Some(index);
            action.view = Some(super::View::Page);
        }
    }

    // Legend.
    let legend = [
        (EdgeKind::PageParent, t(locale, Key::PageParent)),
        (EdgeKind::Reply, t(locale, Key::Reply)),
        (EdgeKind::Reference, t(locale, Key::Reference)),
        (EdgeKind::TagCluster, t(locale, Key::TagCluster)),
    ];
    let mut x = rect.left() + VIEW_PAD;
    let y = rect.bottom() - 26.0;
    for (kind, label) in legend {
        let color = match kind {
            EdgeKind::PageParent => theme::fade(theme::ACCENT, 0.8),
            EdgeKind::Reply => theme::fade(theme::INK, 0.5),
            EdgeKind::Reference => theme::fade(theme::INK, 0.35),
            EdgeKind::TagCluster => theme::fade(theme::ACCENT_INK, 0.3),
        };
        painter.line_segment([pos2(x, y), pos2(x + 16.0, y)], Stroke::new(1.5, color));
        painter.text(
            pos2(x + 22.0, y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::new(9.5, theme::mono()),
            theme::FAINT,
        );
        x += 30.0 + label.chars().count() as f32 * 6.0;
    }

    // Selected page summary in the corner.
    if let Some(index) = selected {
        if let Some(capsule) = capsules.get(index) {
            if graph.nodes.iter().any(|node| node.capsule == index) {
                let summary_rect = Rect::from_min_size(
                    pos2(rect.right() - 262.0, rect.top() + 14.0),
                    vec2(248.0, 118.0),
                );
                painter.rect(
                    summary_rect,
                    theme::RADIUS,
                    theme::PANEL,
                    theme::hairline(),
                    egui::StrokeKind::Inside,
                );
                let mut card = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(summary_rect.shrink(10.0))
                        .layout(Layout::top_down(Align::Min)),
                );
                let _ = hud(
                    &mut card,
                    if capsule.native { "PAGE" } else { "IMPORTED" },
                    theme::FAINT,
                );
                card.add_space(5.0);
                card.add(
                    egui::Label::new(
                        RichText::new(truncate_chars(&capsule.subject, 34))
                            .family(theme::medium())
                            .size(12.5)
                            .color(theme::INK),
                    )
                    .wrap(),
                );
                card.add_space(4.0);
                card.add(
                    egui::Label::new(
                        RichText::new(truncate_chars(&capsule.from, 32))
                            .size(10.5)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
                if !capsule.tags.is_empty() {
                    card.add_space(5.0);
                    card.horizontal_wrapped(|ui| {
                        for tag in capsule.tags.iter().take(3) {
                            let _ = tag_chip(ui, tag, theme::MUTED);
                        }
                    });
                }
            }
        }
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_state_resets_zoom() {
        let state = GraphState::new();
        assert_eq!(state.zoom, 1.0);
        assert!(state.tag.is_none());
    }
}
