//! Density bin rendering for the graph view when tens of thousands of nodes share the screen.

use std::collections::HashMap;

use egui::{pos2, vec2, FontId, Painter, Rect, Response, StrokeKind};

use crate::graph::{Graph, GraphMetrics};
use crate::i18n::Locale;
use crate::theme;
use crate::truncate_chars;

use super::interaction::{focus_cluster, GraphTransform};
use super::render_clusters::{cluster_color, cluster_display_name};
use super::GraphState;

pub fn render_density(
    painter: &Painter,
    rect: Rect,
    response: &Response,
    transform: &GraphTransform,
    graph: &Graph,
    metrics: &GraphMetrics,
    state: &mut GraphState,
    visible: &[u32],
    positions: &[[f32; 2]],
    locale: Locale,
) {
    let cell = ((rect.width() * rect.height()) / visible.len().max(1) as f32)
        .sqrt()
        .clamp(14.0, 44.0);
    let mut bins: HashMap<(i32, i32), (u32, u32)> = HashMap::new(); // cell → (count, a cluster)
    for &slot in visible {
        let p = transform.to_screen(positions[slot as usize]);
        let key = ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32);
        let entry = bins
            .entry(key)
            .or_insert((0, graph.cluster_of[slot as usize]));
        entry.0 += 1;
    }
    let max_count = bins.values().map(|(count, _)| *count).max().unwrap_or(1);
    let mut cells: Vec<((i32, i32), (u32, u32))> =
        bins.iter().map(|(key, value)| (*key, *value)).collect();
    cells.sort_by_key(|&(key, _)| (key.0, key.1));

    let focus = state.focus;
    let dim_cluster = |index: u32| -> f32 {
        match focus {
            Some(focus) if focus != index => 0.3,
            _ => 1.0,
        }
    };

    for (key, (count, cluster)) in cells {
        let cx = (key.0 as f32 + 0.5) * cell;
        let cy = (key.1 as f32 + 0.5) * cell;
        if !rect.contains(pos2(cx, cy)) {
            continue;
        }
        let strength = 0.12 + 0.5 * ((count as f32) / (max_count as f32)).sqrt();
        let dim = dim_cluster(cluster);
        painter.rect_filled(
            Rect::from_center_size(pos2(cx, cy), vec2(cell - 1.5, cell - 1.5)),
            (cell - 1.5) / 2.0,
            theme::fade(cluster_color(cluster), strength * dim),
        );
    }
    if let Some(pointer) = response.hover_pos() {
        let key = (
            (pointer.x / cell).floor() as i32,
            (pointer.y / cell).floor() as i32,
        );
        if let Some((count, cluster)) = bins.get(&key) {
            let name = cluster_display_name(&graph.clusters[*cluster as usize].key, locale);
            let galley = painter.layout_no_wrap(
                format!("{} · {}", truncate_chars(&name, 30), count),
                FontId::new(11.5, theme::medium()),
                theme::ink(),
            );
            let box_rect = Rect::from_min_size(
                pos2(pointer.x + 12.0, pointer.y - galley.size().y / 2.0 - 6.0),
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
        }
    }
    if response.double_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let key = (
                (pointer.x / cell).floor() as i32,
                (pointer.y / cell).floor() as i32,
            );
            if let Some((_, cluster)) = bins.get(&key) {
                focus_cluster(state, rect, metrics, *cluster);
            }
        }
    }
}
