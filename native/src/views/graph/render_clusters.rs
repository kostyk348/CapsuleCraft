//! Cluster-level rendering and packing for the graph view.

use egui::{pos2, vec2, Align, Color32, FontId, Layout, Painter, Rect, Response, RichText, Stroke, Ui};

use crate::graph::{Graph, GraphMetrics};
use crate::i18n::{t, Key, Locale};
use crate::theme;
use crate::truncate_chars;

use super::interaction::focus_cluster;
use super::GraphState;

/// Golden-angle hue walk: neighbouring clusters get distinct colours without
/// a palette table and without randomness.
pub fn cluster_color(index: u32) -> Color32 {
    let hue = (index as f32 * 0.618_034) % 1.0;
    let (r, g, b) = hsv_to_rgb(hue, 0.52, 0.86);
    Color32::from_rgb(r, g, b)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor() as i32 % 6;
    let f = h * 6.0 - (h * 6.0).floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Bubble-chart packing for the cluster overview. Radii are scaled down so
/// the total bubble area fits `fill` of the box, then overlapping bubbles
/// are relaxed apart — anchored to their layout centroids with a gentle
/// pull, so the geography of the map survives. Deterministic: fixed pair
/// order, no randomness. Returns the packed centers and the final radii.
pub fn pack_bubbles(
    anchors: &[[f32; 2]],
    radii: &[f32],
    bounds: ([f32; 2], [f32; 2]),
    fill: f32,
    iterations: usize,
) -> (Vec<[f32; 2]>, Vec<f32>) {
    let n = anchors.len();
    let mut centers = anchors.to_vec();
    let mut radii = radii.to_vec();
    if n == 0 {
        return (centers, radii);
    }
    let span = [
        (bounds.1[0] - bounds.0[0]).max(1.0),
        (bounds.1[1] - bounds.0[1]).max(1.0),
    ];
    let area = span[0] * span[1];
    let total: f32 = radii.iter().map(|r| r * r).sum::<f32>() * std::f32::consts::PI;
    if total > area * fill {
        let scale = (area * fill / total).sqrt();
        for r in &mut radii {
            *r = (*r * scale).max(7.0);
        }
    }
    for _ in 0..iterations {
        let mut worst = 0.0f32;
        for a in 0..n {
            for b in (a + 1)..n {
                let dx = centers[b][0] - centers[a][0];
                let dy = centers[b][1] - centers[a][1];
                let want = radii[a] + radii[b] + 2.0;
                let d2 = dx * dx + dy * dy;
                if d2 >= want * want {
                    continue;
                }
                let d = d2.sqrt().max(1e-3);
                let push = (want - d) * 0.9;
                let (ux, uy) = (dx / d, dy / d);
                let wa =
                    radii[b] * radii[b] / (radii[a] * radii[a] + radii[b] * radii[b]).max(1e-6);
                centers[a][0] -= ux * push * wa;
                centers[a][1] -= uy * push * wa;
                centers[b][0] += ux * push * (1.0 - wa);
                centers[b][1] += uy * push * (1.0 - wa);
                worst = worst.max(push);
            }
        }
        for i in 0..n {
            centers[i][0] += (anchors[i][0] - centers[i][0]) * 0.025;
            centers[i][1] += (anchors[i][1] - centers[i][1]) * 0.025;
            let pad = radii[i] * 0.5;
            centers[i][0] = centers[i][0].clamp(bounds.0[0] + pad, bounds.1[0] - pad);
            centers[i][1] = centers[i][1].clamp(bounds.0[1] + pad, bounds.1[1] - pad);
        }
        if worst < 0.4 {
            break;
        }
    }
    (centers, radii)
}

/// Greedy label de-collision: `rects` are candidates in priority order
/// (most important first); returns the indices that fit without touching an
/// already-kept label. Shared by the pages view and the cluster overview so
/// both cap text the same way.
pub fn keep_non_overlapping(rects: &[egui::Rect]) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for (i, r) in rects.iter().enumerate() {
        if kept.iter().all(|&k| !rects[k].intersects(*r)) {
            kept.push(i);
        }
    }
    kept
}

pub fn cluster_display_name<'a>(key: &'a str, locale: Locale) -> String {
    if key.is_empty() {
        t(locale, Key::GraphRootCluster).to_string()
    } else {
        key.to_string()
    }
}

pub fn render_clusters(
    painter: &Painter,
    rect: Rect,
    response: &Response,
    graph: &Graph,
    metrics: &GraphMetrics,
    state: &mut GraphState,
    locale: Locale,
) {
    let mut cmin = [f32::INFINITY; 2];
    let mut cmax = [f32::NEG_INFINITY; 2];
    for centroid in &metrics.centroids {
        for axis in 0..2 {
            cmin[axis] = cmin[axis].min(centroid[axis]);
            cmax[axis] = cmax[axis].max(centroid[axis]);
        }
    }
    let span_x = (cmax[0] - cmin[0]).max(1e-3);
    let span_y = (cmax[1] - cmin[1]).max(1e-3);
    let fit = |p: [f32; 2]| -> [f32; 2] {
        let fx = (p[0] - cmin[0]) / span_x;
        let fy = (p[1] - cmin[1]) / span_y;
        [
            rect.left() + (0.08 + 0.84 * fx.clamp(0.0, 1.0)) * rect.width(),
            rect.top() + (0.08 + 0.84 * fy.clamp(0.0, 1.0)) * rect.height(),
        ]
    };
    let cluster_radius =
        |count: usize| -> f32 { (9.0 + (count as f32).sqrt() * 1.7).min(30.0) };
    let count = graph.clusters.len();
    let anchors: Vec<[f32; 2]> = (0..count).map(|i| fit(metrics.centroids[i])).collect();
    let raw_radii: Vec<f32> = (0..count)
        .map(|i| cluster_radius(graph.clusters[i].members.len()))
        .collect();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    (rect.width() as u32).hash(&mut hasher);
    (rect.height() as u32).hash(&mut hasher);
    for (a, r) in anchors.iter().zip(&raw_radii) {
        ((a[0] * 2.0) as i32).hash(&mut hasher);
        ((a[1] * 2.0) as i32).hash(&mut hasher);
        ((*r * 4.0) as i32).hash(&mut hasher);
    }
    let fingerprint = hasher.finish();
    let (centers, radii) = match &state.pack_cache {
        Some((key, centers, radii)) if *key == fingerprint => (centers.clone(), radii.clone()),
        _ => {
            let packed = pack_bubbles(
                &anchors,
                &raw_radii,
                (
                    [rect.left() + 4.0, rect.top() + 4.0],
                    [rect.right() - 4.0, rect.bottom() - 4.0],
                ),
                0.42,
                120,
            );
            state.pack_cache = Some((fingerprint, packed.0.clone(), packed.1.clone()));
            packed
        }
    };
    let center = |index: usize| -> egui::Pos2 { pos2(centers[index][0], centers[index][1]) };
    let focus = state.focus;
    let dim_cluster = |index: u32| -> f32 {
        match focus {
            Some(focus) if focus != index => 0.3,
            _ => 1.0,
        }
    };

    if state.edges {
        for &(a, b, weight) in &graph.cluster_edges {
            if graph.clusters[a as usize].members.is_empty()
                || graph.clusters[b as usize].members.is_empty()
            {
                continue;
            }
            let dim = dim_cluster(a).min(dim_cluster(b));
            let width = (0.8 + (weight as f32).ln().max(0.0)).clamp(0.8, 3.0);
            painter.line_segment(
                [center(a as usize), center(b as usize)],
                Stroke::new(width, theme::fade(theme::accent(), 0.4 * dim)),
            );
        }
    }
    let mut order: Vec<u32> = (0..count as u32).collect();
    order.sort_by_key(|&index| graph.clusters[index as usize].members.len());
    let mut hovered_cluster: Option<u32> = None;
    if let Some(pointer) = response.hover_pos() {
        let mut best_d2 = f32::INFINITY;
        for &index in &order {
            let i = index as usize;
            let d2 = pointer.distance_sq(center(i));
            if d2 <= (radii[i] + 6.0).powi(2) && d2 < best_d2 {
                best_d2 = d2;
                hovered_cluster = Some(index);
            }
        }
    }
    for &index in &order {
        let i = index as usize;
        let pos = center(i);
        let node_radius = radii[i];
        let color = cluster_color(index);
        let dim = dim_cluster(index);
        if hovered_cluster == Some(index) || focus == Some(index) {
            painter.circle_filled(pos, node_radius + 5.0, theme::fade(color, 0.2 * dim));
        }
        painter.circle_filled(pos, node_radius, theme::fade(color, 0.82 * dim));
        painter.circle_stroke(
            pos,
            node_radius,
            Stroke::new(1.0, theme::fade(theme::bg(), 0.8)),
        );
    }

    let mut top: Vec<u32> = order.clone();
    top.sort_by_key(|&index| std::cmp::Reverse(graph.clusters[index as usize].members.len()));
    let label_font = FontId::new(10.5, theme::medium());
    let mut label_rects: Vec<egui::Rect> = Vec::new();
    let mut label_texts: Vec<String> = Vec::new();
    for &index in top.iter().take(14) {
        let i = index as usize;
        let count = graph.clusters[i].members.len();
        let name = cluster_display_name(&graph.clusters[i].key, locale);
        let text = format!("{} · {}", truncate_chars(&name, 22), count);
        let galley = painter.layout_no_wrap(text.clone(), label_font.clone(), theme::muted());
        let size = galley.size();
        let pos = center(i);
        let min = pos2(pos.x - size.x / 2.0 - 2.0, pos.y + radii[i] + 3.0);
        label_rects.push(Rect::from_min_size(min, size + vec2(4.0, 2.0)));
        label_texts.push(text);
    }
    for i in keep_non_overlapping(&label_rects) {
        let index = top[i];
        painter.text(
            label_rects[i].min + vec2(2.0, 0.0),
            egui::Align2::LEFT_TOP,
            &label_texts[i],
            label_font.clone(),
            theme::fade(theme::muted(), dim_cluster(index)),
        );
    }
    if response.clicked() {
        if let Some(index) = hovered_cluster {
            focus_cluster(state, rect, metrics, index);
        } else {
            state.focus = None;
        }
    }
}

pub fn draw_cluster_panel(
    ui: &mut Ui,
    painter: &Painter,
    rect: Rect,
    graph: &Graph,
    metrics: &GraphMetrics,
    state: &mut GraphState,
    locale: Locale,
) {
    if graph.clusters.len() <= 1 {
        return;
    }
    let mut top: Vec<u32> = (0..graph.clusters.len() as u32).collect();
    top.sort_by_key(|&index| std::cmp::Reverse(graph.clusters[index as usize].members.len()));
    let rows = top.len().min(8);
    let panel_rect = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.top() + 12.0),
        vec2(
            216.0,
            12.0 + (rows + usize::from(state.focus.is_some())) as f32 * 21.0 + 10.0,
        ),
    );
    painter.rect(
        panel_rect,
        theme::RADIUS,
        theme::fade(theme::panel(), 0.94),
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    let mut panel = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(panel_rect.shrink(8.0))
            .layout(Layout::top_down(Align::Min)),
    );
    panel.label(
        RichText::new(t(locale, Key::GraphClustersPanel))
            .size(9.5)
            .color(theme::faint()),
    );
    if let Some(focus) = state.focus {
        if panel
            .add(
                egui::Button::new(
                    RichText::new(format!("← {}", t(locale, Key::All)))
                        .size(10.5)
                        .color(theme::accent_ink()),
                )
                .fill(Color32::TRANSPARENT)
                .frame(false),
            )
            .clicked()
        {
            state.focus = None;
        }
        let _ = focus;
    }
    let focus = state.focus;
    let dim_cluster = |index: u32| -> f32 {
        match focus {
            Some(focus) if focus != index => 0.3,
            _ => 1.0,
        }
    };
    for &index in top.iter().take(8) {
        let cluster = &graph.clusters[index as usize];
        let name = cluster_display_name(&cluster.key, locale);
        let row_label = format!("{} · {}", truncate_chars(&name, 16), cluster.members.len());
        let dot = cluster_color(index);
        let button = egui::Button::new(
            RichText::new(format!("● {}", row_label))
                .size(10.5)
                .color(theme::fade(dot, dim_cluster(index))),
        )
        .fill(Color32::TRANSPARENT)
        .frame(false);
        if panel.add(button).clicked() {
            focus_cluster(state, rect, metrics, index);
        }
    }
}
