//! Individual node, edge, and label rendering for the detailed `LodMode::Pages` view.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, FontId, Painter, Rect, Response, Stroke, StrokeKind};

use crate::graph::{EdgeKind, Graph, GraphMetrics};
use crate::i18n::{t, Key, Locale};
use crate::mime::Capsule;
use crate::theme;
use crate::truncate_chars;
use crate::views::{View, ViewAction, VIEW_PAD};

use super::interaction::GraphTransform;
use super::render_clusters::{cluster_color, keep_non_overlapping};
use super::GraphState;

const LABEL_MAX: usize = 240;
const NEAR_EDGE_CAP: usize = 6000;

pub fn render_pages(
    painter: &Painter,
    _rect: Rect,
    response: &Response,
    transform: &GraphTransform,
    graph: &Graph,
    metrics: &GraphMetrics,
    state: &GraphState,
    capsules: &[Capsule],
    visible: &[u32],
    positions: &[[f32; 2]],
    selected: Option<usize>,
    locale: Locale,
    action: &mut ViewAction,
) {
    let visible_set: HashSet<u32> = visible.iter().copied().collect();
    let focus = state.focus;

    let dim_node = |slot: u32| -> f32 {
        match focus {
            Some(focus) if graph.cluster_of.get(slot as usize) != Some(&focus) => 0.22,
            _ => 1.0,
        }
    };

    // Slot lookup by capsule index
    let mut slot_of: HashMap<usize, u32> = HashMap::with_capacity(graph.nodes.len());
    for (slot, node) in graph.nodes.iter().enumerate() {
        slot_of.insert(node.capsule, slot as u32);
    }

    // Active focus (hovered node takes precedence, then selected page)
    let selected_slot = selected.and_then(|sel| slot_of.get(&sel).copied());

    // Local mode neighborhood calculation
    let local_set: Option<HashSet<u32>> = if state.local_mode {
        let root = selected_slot.or_else(|| {
            if !graph.nodes.is_empty() {
                Some(0)
            } else {
                None
            }
        });
        root.map(|r| metrics.neighborhood_bfs(r, state.local_depth.clamp(1, 3)))
    } else {
        None
    };

    let is_node_allowed = |slot: u32| -> bool {
        if state.hide_orphans && graph.nodes[slot as usize].degree == 0 {
            return false;
        }
        if let Some(set) = &local_set {
            if !set.contains(&slot) {
                return false;
            }
        }
        true
    };

    // Hit-testing / hover state
    let pointer = response.hover_pos();
    let hovered = pointer
        .and_then(|p| {
            metrics
                .grid
                .nearest(positions, transform.to_graph(p), 10.0 / transform.radius)
        })
        .filter(|&slot| is_node_allowed(slot));

    if state.dragged_node.is_some() {
        response.clone().on_hover_cursor(egui::CursorIcon::Grabbing);
    } else if hovered.is_some() {
        response.clone().on_hover_cursor(egui::CursorIcon::Grab);
    }

    let active_slot = hovered.or(selected_slot).filter(|&slot| is_node_allowed(slot));

    // Search query matching
    let search_clean = state.search_query.trim().to_lowercase();
    let has_search = !search_clean.is_empty();
    let is_search_match = |slot: u32| -> bool {
        if !has_search {
            return false;
        }
        let capsule_idx = graph.nodes[slot as usize].capsule;
        capsules.get(capsule_idx).map_or(false, |c| {
            c.subject.to_lowercase().contains(&search_clean)
        })
    };

    // Connected neighborhood query O(k)
    let mut active_neighbors: HashSet<u32> = HashSet::new();
    let mut active_edges: HashSet<usize> = HashSet::new();
    if let Some(focus_slot) = active_slot {
        active_neighbors.insert(focus_slot);
        if let Some(neighbors) = metrics.adjacency.get(focus_slot as usize) {
            for &(neighbor, edge_idx) in neighbors {
                active_neighbors.insert(neighbor);
                active_edges.insert(edge_idx);
            }
        }
    }
    let has_active = active_slot.is_some();

    // 1. Render edges
    if state.edges {
        let mut drawn = 0usize;
        let mut active_draw_queue: Vec<(egui::Pos2, egui::Pos2)> = Vec::new();

        for (edge_idx, edge) in graph.edges.iter().enumerate() {
            if drawn >= NEAR_EDGE_CAP {
                break;
            }
            let (Some(&a), Some(&b)) = (slot_of.get(&edge.source), slot_of.get(&edge.target))
            else {
                continue;
            };
            if !visible_set.contains(&a) || !visible_set.contains(&b) {
                continue;
            }
            if !is_node_allowed(a) || !is_node_allowed(b) {
                continue;
            }
            drawn += 1;
            let pa = transform.to_screen(positions[a as usize]);
            let pb = transform.to_screen(positions[b as usize]);

            if has_active && active_edges.contains(&edge_idx) {
                active_draw_queue.push((pa, pb));
            } else {
                let color = match edge.kind {
                    EdgeKind::PageParent => theme::fade(theme::accent(), 0.55),
                    EdgeKind::Reply => theme::fade(theme::ink(), 0.3),
                    EdgeKind::Reference => theme::fade(theme::ink(), 0.18),
                    EdgeKind::TagCluster => theme::fade(theme::accent_ink(), 0.12),
                };
                let alpha_mult = if has_active { 0.10 } else { 1.0 };
                painter.line_segment(
                    [pa, pb],
                    Stroke::new(
                        1.0,
                        theme::fade(color, dim_node(a).min(dim_node(b)) * alpha_mult),
                    ),
                );
            }
        }

        // Active edges drawn on top with 2.2px accent stroke
        for (pa, pb) in active_draw_queue {
            painter.line_segment([pa, pb], Stroke::new(2.2, theme::accent()));
        }
    }

    // 2. Render nodes
    let mut order: Vec<u32> = visible.to_vec();
    order.sort_by_key(|&slot| graph.nodes[slot as usize].degree);
    for &slot in &order {
        if !is_node_allowed(slot) {
            continue;
        }
        let node = &graph.nodes[slot as usize];
        let pos = transform.to_screen(positions[slot as usize]);
        let node_radius = (4.5 + (node.degree as f32).sqrt() * 1.8).clamp(4.5, 12.0)
            * state.zoom.clamp(0.7, 1.15);
        let is_selected = selected == Some(node.capsule);
        let is_focus_center = active_slot == Some(slot);
        let is_connected_neighbor = active_neighbors.contains(&slot);
        let is_match = is_search_match(slot);
        let base = cluster_color(graph.cluster_of[slot as usize]);
        let color = if is_selected || is_focus_center || is_match {
            theme::accent_ink()
        } else {
            base
        };

        let dim = if has_search {
            if is_match {
                1.0
            } else {
                0.15 * dim_node(slot)
            }
        } else if has_active {
            if is_focus_center || is_connected_neighbor {
                1.0
            } else {
                0.20 * dim_node(slot)
            }
        } else {
            dim_node(slot)
        };

        // Outer glow
        if is_match {
            painter.circle_stroke(pos, node_radius + 4.0, Stroke::new(2.0, theme::accent()));
            painter.circle_filled(pos, node_radius + 2.0, theme::fade(theme::accent(), 0.35));
        } else if is_focus_center || is_selected {
            painter.circle_filled(pos, node_radius + 6.0, theme::fade(theme::accent(), 0.35));
            painter.circle_filled(pos, node_radius + 3.0, theme::fade(color, 0.3));
        } else if is_connected_neighbor && has_active {
            painter.circle_filled(pos, node_radius + 3.0, theme::fade(color, 0.25));
        }

        painter.circle_filled(pos, node_radius, theme::fade(color, dim));
        painter.circle_stroke(
            pos,
            node_radius,
            Stroke::new(1.0, theme::fade(theme::bg(), 0.8)),
        );
    }

    // 3. Render labels
    if state.zoom >= 0.55 {
        let size = (10.5 * state.zoom.clamp(0.85, 1.1)).clamp(9.0, 12.0);
        let font = FontId::new(size, egui::FontFamily::Proportional);
        let mut top: Vec<u32> = visible.to_vec();
        top.sort_by(|&a, &b| {
            let ma = is_search_match(a);
            let mb = is_search_match(b);
            mb.cmp(&ma).then_with(|| {
                graph.nodes[b as usize].degree.cmp(&graph.nodes[a as usize].degree)
            })
        });
        let mut candidates: Vec<(u32, String, egui::Rect)> = Vec::new();
        for &slot in &top {
            if candidates.len() >= LABEL_MAX {
                break;
            }
            if !is_node_allowed(slot) {
                continue;
            }
            if hovered == Some(slot) {
                continue;
            }
            let node = &graph.nodes[slot as usize];
            let pos = transform.to_screen(positions[slot as usize]);
            let node_radius = (4.5 + (node.degree as f32).sqrt() * 1.8).clamp(4.5, 12.0)
                * state.zoom.clamp(0.7, 1.15);
            let capsule = &capsules[node.capsule];
            let label = if capsule.subject.is_empty() {
                t(locale, Key::Untitled).to_string()
            } else {
                truncate_chars(&capsule.subject, 26)
            };
            let galley = painter.layout_no_wrap(label.clone(), font.clone(), theme::muted());
            let text_size = galley.size();
            let min = pos2(pos.x - text_size.x / 2.0 - 2.0, pos.y + node_radius + 5.0);
            candidates.push((
                slot,
                label,
                Rect::from_min_size(min, text_size + vec2(4.0, 2.0)),
            ));
        }
        let keep: Vec<usize> =
            keep_non_overlapping(&candidates.iter().map(|c| c.2).collect::<Vec<_>>());
        for i in keep {
            let (slot, label, label_rect) = &candidates[i];
            let node = &graph.nodes[*slot as usize];
            let is_match = is_search_match(*slot);
            let is_active_node = active_neighbors.contains(slot) || is_match;
            let alpha = if has_search {
                if is_match {
                    1.0
                } else {
                    0.15 * dim_node(*slot)
                }
            } else if has_active {
                if is_active_node {
                    1.0
                } else {
                    0.15 * dim_node(*slot)
                }
            } else {
                0.9 * dim_node(*slot)
            };
            painter.text(
                label_rect.min + vec2(2.0, 0.0),
                egui::Align2::LEFT_TOP,
                label,
                font.clone(),
                if selected == Some(node.capsule) || is_match {
                    theme::ink()
                } else {
                    theme::fade(theme::muted(), alpha)
                },
            );
        }
    }

    // 4. Hover card
    if let Some(slot) = hovered {
        let node = &graph.nodes[slot as usize];
        let capsule = &capsules[node.capsule];
        let anchor = transform.to_screen(positions[slot as usize]);
        let label = truncate_chars(&capsule.subject, 40);
        let galley =
            painter.layout_no_wrap(label, FontId::new(11.5, theme::medium()), theme::ink());
        let box_rect = Rect::from_min_size(
            pos2(anchor.x + 12.0, anchor.y - galley.size().y / 2.0 - 6.0),
            galley.size() + vec2(16.0, 12.0),
        );
        painter.rect(
            box_rect,
            theme::RADIUS,
            theme::panel(),
            theme::hairline(),
            StrokeKind::Inside,
        );
        painter.galley(box_rect.min + vec2(8.0, 6.0), galley, theme::ink());
        if response.clicked() {
            action.select = Some(node.capsule);
        }
        if response.double_clicked() {
            action.select = Some(node.capsule);
            action.view = Some(View::Page);
        }
    }
}

pub fn draw_page_legend(painter: &Painter, rect: Rect, locale: Locale) {
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
            EdgeKind::PageParent => theme::fade(theme::accent(), 0.8),
            EdgeKind::Reply => theme::fade(theme::ink(), 0.5),
            EdgeKind::Reference => theme::fade(theme::ink(), 0.35),
            EdgeKind::TagCluster => theme::fade(theme::accent_ink(), 0.3),
        };
        painter.line_segment([pos2(x, y), pos2(x + 16.0, y)], Stroke::new(1.5, color));
        painter.text(
            pos2(x + 22.0, y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::new(9.5, theme::mono()),
            theme::faint(),
        );
        x += 30.0 + label.chars().count() as f32 * 6.0;
    }
}
