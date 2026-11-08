//! Relationship graph view: a zoomable canvas over page parents, replies,
//! references and tag clusters that stays readable at vault scale.
//!
//! Three LOD modes (`graph::pick_mode`), picked per frame from the zoom
//! level and the number of nodes inside the viewport:
//! * **Clusters** (zoomed far out) — one node per folder cluster, edges
//!   aggregated between clusters, labels only for the largest ones.
//! * **Density** — when thousands of nodes share the viewport, they collapse
//!   into screen-space density bins coloured by their cluster.
//! * **Pages** — individual nodes, edges, labels near the zoom cap.
//!
//! Layout for big vaults runs on a background thread (`layout_in_background`);
//! the view paints spiral/cached positions immediately and swaps in
//! snapshots as the thread reports them, so a 200k-page vault never
//! freezes the UI.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, Ui};

use crate::graph::{self, EdgeKind, Graph, GraphMetrics, LodMode, PositionCache, SYNC_LAYOUT_MAX};
use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mime::Capsule;
use crate::theme;
use crate::{ghost, hud, menu_row, popover_frame, tag_chip, truncate_chars};

use super::{empty_state, header, View, ViewAction, VIEW_PAD};

const LABEL_MAX: usize = 240;
const NEAR_EDGE_CAP: usize = 6000;

#[derive(Default)]
pub struct GraphState {
    pub tag: Option<String>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub edges: bool,
    /// Focused cluster: members render at full strength, everything else
    /// fades; entering a cluster centres and zooms onto its bounds.
    pub focus: Option<u32>,
    /// Background layout progress channel, when a big vault is relaxing.
    layout_rx: Option<std::sync::mpsc::Receiver<graph::LayoutMsg>>,
    /// Vault signature the running job was started for — a vault edit
    /// invalidates in-flight snapshots, so they are dropped instead of
    /// being applied to a graph with different nodes.
    layout_sig: Option<u64>,
    layout_done: usize,
    layout_total: usize,
    /// Layout is far too expensive to redo every frame: keep the last graph
    /// plus the vault signature it was built from. Rebuild only when either
    /// changes (edits, imports, tag filter).
    cached: Option<(
        u64,
        Option<String>,
        std::sync::Arc<Graph>,
        std::sync::Arc<GraphMetrics>,
    )>,
    /// Message-ID → position, persisted next to `ui_state.json`.
    cache: Option<PositionCache>,
    cache_path: Option<std::path::PathBuf>,
    /// Packed bubble layout for the cluster overview, keyed by an input
    /// fingerprint — the packing relaxation must not run every frame.
    pack_cache: Option<(u64, Vec<[f32; 2]>, Vec<f32>)>,
}

/// Cheap vault fingerprint: page count plus the shape of every page's links
/// and tags. O(n) per frame, enough to catch any edit that could change the
/// graph; the expensive layout runs only when this value moves.
fn vault_signature(capsules: &[Capsule]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    capsules.len().hash(&mut hasher);
    for capsule in capsules {
        capsule.message_id.hash(&mut hasher);
        capsule.parent_id.hash(&mut hasher);
        capsule.in_reply_to.hash(&mut hasher);
        capsule.references.len().hash(&mut hasher);
        capsule.tags.len().hash(&mut hasher);
        capsule.file_name.hash(&mut hasher);
        for r in &capsule.references {
            r.hash(&mut hasher);
        }
    }
    hasher.finish()
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            tag: None,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            edges: true,
            focus: None,
            layout_rx: None,
            layout_sig: None,
            layout_done: 0,
            layout_total: 0,
            cached: None,
            cache: None,
            cache_path: None,
            pack_cache: None,
        }
    }
}

/// Golden-angle hue walk: neighbouring clusters get distinct colours without
/// a palette table and without randomness.
fn cluster_color(index: u32) -> Color32 {
    let hue = (index as f32 * 0.618_034) % 1.0;
    let (r, g, b) = hsv_to_rgb(hue, 0.52, 0.86);
    Color32::from_rgb(r, g, b)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
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
fn pack_bubbles(
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
        // Over budget: shrink every bubble by the same factor (area ∝ r²,
        // so the radius scale is the square root of the area scale), with a
        // floor so tiny clusters stay clickable.
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
                // The smaller bubble gives way: displacement weights come
                // from the other bubble's area.
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
            // Anchor pull keeps the layout's geography; the clamp keeps the
            // bubble inside the canvas.
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
fn keep_non_overlapping(rects: &[egui::Rect]) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for (i, r) in rects.iter().enumerate() {
        if kept.iter().all(|&k| !rects[k].intersects(*r)) {
            kept.push(i);
        }
    }
    kept
}

fn cluster_display_name<'a>(key: &'a str, locale: Locale) -> String {
    if key.is_empty() {
        t(locale, Key::GraphRootCluster).to_string()
    } else {
        key.to_string()
    }
}

/// Centre the view on a cluster and zoom so its bounding box fills ~70% of
/// the shorter canvas side.
fn focus_cluster(state: &mut GraphState, rect: Rect, metrics: &GraphMetrics, index: u32) {
    let centre = metrics.centroids[index as usize];
    let (min, max) = metrics.bounds[index as usize];
    let span = (max[0] - min[0]).max(max[1] - min[1]).max(0.05);
    state.focus = Some(index);
    state.zoom = (1.75 / span).clamp(0.35, 3.0);
    let radius = (rect.width().min(rect.height()) * 0.40) * state.zoom;
    // to_screen(centre) must land on rect.center(): centre+pan + c*radius.
    state.pan = vec2(-centre[0] * radius, -centre[1] * radius);
}

pub fn draw(
    ui: &mut Ui,
    capsules: &[Capsule],
    state: &mut GraphState,
    locale: Locale,
    selected: Option<usize>,
) -> ViewAction {
    let mut action = ViewAction::default();
    let signature = vault_signature(capsules);
    let tag = state.tag.clone();
    let rebuild = !matches!(
        &state.cached,
        Some((sig, cached_tag, _, _)) if *sig == signature && *cached_tag == tag
    );
    if rebuild {
        if state.cache.is_none() {
            let path = state
                .cache_path
                .clone()
                .unwrap_or_else(|| crate::vault::app_data_dir().join("graph_layout.json"));
            state.cache = PositionCache::load(&path);
            state.cache_path = Some(path);
        }
        let built = std::sync::Arc::new(graph::build_with_cache(
            capsules,
            state.tag.as_deref(),
            state.cache.as_ref(),
        ));
        let node_count = built.nodes.len();
        let metrics = std::sync::Arc::new(graph::build_metrics(&built));
        state.cached = Some((signature, tag.clone(), built.clone(), metrics.clone()));
        // Big vault: the first relaxation happens on a thread; the canvas
        // paints spiral/cache positions until snapshots arrive.
        if node_count >= SYNC_LAYOUT_MAX && state.layout_rx.is_none() {
            state.layout_sig = Some(signature);
            state.layout_rx = Some(graph::layout_in_background(
                built,
                graph::auto_iterations(node_count),
            ));
        }
    }
    let (mut graph, mut metrics) = match &state.cached {
        Some((_, _, graph, metrics)) => (graph.clone(), metrics.clone()),
        None => {
            let built = std::sync::Arc::new(graph::build(capsules, state.tag.as_deref()));
            let metrics = std::sync::Arc::new(graph::build_metrics(&built));
            (built, metrics)
        }
    };

    // Drain the background layout channel: apply the freshest snapshot so
    // the picture sharpens while the thread keeps working. Snapshots from a
    // superseded vault signature are dropped on the floor.
    if state.layout_rx.is_some() && state.layout_sig != Some(signature) {
        state.layout_rx = None;
        state.layout_sig = None;
    }
    if let Some(rx) = &state.layout_rx {
        let mut latest: Option<(usize, usize, Vec<[f32; 2]>)> = None;
        let mut done: Option<Vec<[f32; 2]>> = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                graph::LayoutMsg::Progress {
                    done: step,
                    total,
                    positions,
                } => latest = Some((step, total, positions)),
                graph::LayoutMsg::Done(positions) => done = Some(positions),
            }
        }
        let apply = done.clone().or_else(|| latest.clone().map(|(_, _, p)| p));
        if let Some(positions) = apply {
            let mut updated = (*graph).clone();
            for (node, pos) in updated.nodes.iter_mut().zip(positions) {
                node.pos = pos;
            }
            let updated = std::sync::Arc::new(updated);
            metrics = std::sync::Arc::new(graph::build_metrics(&updated));
            state.cached = Some((signature, tag.clone(), updated.clone(), metrics.clone()));
            graph = updated;
        }
        if let Some((step, total, _)) = latest {
            state.layout_done = step;
            state.layout_total = total;
        }
        if let Some(_final_positions) = done {
            state.layout_rx = None;
            state.layout_done = state.layout_total;
            let cache = PositionCache::from_graph(&graph, capsules);
            if let Some(path) = &state.cache_path.clone() {
                let _ = cache.save(path);
            }
            state.cache = Some(cache);
        }
    }

    header(
        ui,
        t(locale, Key::GraphTitle),
        &format!(
            "{} {} · {} {} · {} {}",
            graph.nodes.len(),
            t(locale, Key::Nodes),
            graph.edges.len(),
            t(locale, Key::Edges),
            graph.clusters.len(),
            t(locale, Key::GraphClustersPanel),
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
                state.focus = None;
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
                    state.focus = None;
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
            state.focus = None;
        }
        ui.add_space(8.0);
        let hint = if state.layout_rx.is_some() {
            format!(
                "{} · {}",
                t(locale, Key::GraphLayingOut),
                t(locale, Key::GraphHint)
            )
        } else {
            t(locale, Key::GraphHint).to_string()
        };
        let _ = hud(ui, &hint, theme::faint());
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
            state.zoom = (state.zoom * factor).clamp(0.02, 3.0);
        }
    }

    painter.rect_filled(rect, 0.0, theme::bg());

    let centre = rect.center() + state.pan;
    let radius = (rect.width().min(rect.height()) * 0.40) * state.zoom;
    let to_screen = |pos: [f32; 2]| -> egui::Pos2 {
        pos2(centre.x + pos[0] * radius, centre.y + pos[1] * radius)
    };
    let to_graph =
        |p: egui::Pos2| -> [f32; 2] { [(p.x - centre.x) / radius, (p.y - centre.y) / radius] };

    // Visible node set via the spatial grid — never touch all nodes per frame.
    let world_min = to_graph(rect.min);
    let world_max = to_graph(rect.max);
    let (world_min, world_max) = (
        [
            world_min[0].min(world_max[0]),
            world_min[1].min(world_max[1]),
        ],
        [
            world_min[0].max(world_max[0]),
            world_min[1].max(world_max[1]),
        ],
    );
    let positions: Vec<[f32; 2]> = graph.nodes.iter().map(|node| node.pos).collect();
    let visible = metrics.grid.query(world_min, world_max);
    let mode = graph::pick_mode(state.zoom, visible.len());
    let visible_set: HashSet<u32> = visible.iter().copied().collect();
    let focus = state.focus;

    // Alpha multiplier for "focused cluster members only".
    let dim_node = |slot: u32| -> f32 {
        match focus {
            Some(focus) if graph.cluster_of.get(slot as usize) != Some(&focus) => 0.22,
            _ => 1.0,
        }
    };
    let dim_cluster = |index: u32| -> f32 {
        match focus {
            Some(focus) if focus != index => 0.3,
            _ => 1.0,
        }
    };

    // ---- LOD: clusters -----------------------------------------------------
    // The overview fits the cluster-centroid cloud to the canvas: at this
    // zoom the world itself is a dot, so world-space placement would pile
    // every cluster onto one blob. Screen-space fit keeps the map readable
    // at any zoom below the band cut-off.
    if mode == LodMode::Clusters && graph.clusters.len() > 1 {
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
        // Packing is O(k²) per iteration: cache the result by an input
        // fingerprint and only recompute when the centroids, sizes or the
        // canvas actually change (layout snapshots stream in, so this does
        // refresh as the layout converges).
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
        // Labels: the largest clusters only, so the overview stays a map and
        // not a wall of text — and they may not sit on top of each other.
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
                focus_cluster(state, rect, &metrics, index);
            } else {
                state.focus = None;
            }
        }
    }

    // ---- LOD: density bins --------------------------------------------------
    if mode == LodMode::Density {
        let cell = ((rect.width() * rect.height()) / visible.len().max(1) as f32)
            .sqrt()
            .clamp(14.0, 44.0);
        let mut bins: HashMap<(i32, i32), (u32, u32)> = HashMap::new(); // cell → (count, a cluster)
        for &slot in &visible {
            let p = to_screen(positions[slot as usize]);
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
            // Approximate hover: the bin under the cursor names its cluster.
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
                    egui::StrokeKind::Inside,
                );
                painter.galley(box_rect.min + vec2(8.0, 6.0), galley, theme::ink());
            }
        }
        if response.double_clicked() {
            // Enter the bin's cluster if there is a clear dominant one.
            if let Some(pointer) = response.interact_pointer_pos() {
                let key = (
                    (pointer.x / cell).floor() as i32,
                    (pointer.y / cell).floor() as i32,
                );
                if let Some((_, cluster)) = bins.get(&key) {
                    focus_cluster(&mut *state, rect, &metrics, *cluster);
                }
            }
        }
    }

    // ---- LOD: pages ----------------------------------------------------------
    if mode == LodMode::Pages {
        if state.edges {
            let mut drawn = 0usize;
            for edge in &graph.edges {
                if drawn >= NEAR_EDGE_CAP {
                    break;
                }
                let (a, b) = (edge.source as u32, edge.target as u32);
                if !visible_set.contains(&a) || !visible_set.contains(&b) {
                    continue;
                }
                drawn += 1;
                let pa = to_screen(positions[a as usize]);
                let pb = to_screen(positions[b as usize]);
                let color = match edge.kind {
                    EdgeKind::PageParent => theme::fade(theme::accent(), 0.55),
                    EdgeKind::Reply => theme::fade(theme::ink(), 0.3),
                    EdgeKind::Reference => theme::fade(theme::ink(), 0.18),
                    EdgeKind::TagCluster => theme::fade(theme::accent_ink(), 0.12),
                };
                painter.line_segment(
                    [pa, pb],
                    Stroke::new(1.0, theme::fade(color, dim_node(a).min(dim_node(b)))),
                );
            }
        }
        let mut hovered: Option<u32> = None;
        let pointer = response.hover_pos();
        if let Some(pointer) = pointer {
            hovered = metrics
                .grid
                .nearest(&positions, to_graph(pointer), 10.0 / radius);
        }
        let mut order: Vec<u32> = visible.clone();
        order.sort_by_key(|&slot| graph.nodes[slot as usize].degree);
        for &slot in &order {
            let node = &graph.nodes[slot as usize];
            let pos = to_screen(positions[slot as usize]);
            let node_radius = (4.5 + (node.degree as f32).sqrt() * 1.8).clamp(4.5, 12.0)
                * state.zoom.clamp(0.7, 1.15);
            let is_selected = selected == Some(node.capsule);
            let base = cluster_color(graph.cluster_of[slot as usize]);
            let color = if is_selected {
                theme::accent_ink()
            } else {
                base
            };
            let dim = dim_node(slot);
            if is_selected || hovered == Some(slot) {
                painter.circle_filled(pos, node_radius + 4.0, theme::fade(color, 0.18 * dim));
            }
            painter.circle_filled(pos, node_radius, theme::fade(color, dim));
            painter.circle_stroke(
                pos,
                node_radius,
                Stroke::new(1.0, theme::fade(theme::bg(), 0.8)),
            );
        }
        // Labels: the most connected visible pages, capped per frame, and
        // de-collided — a label that would sit on an already-placed one is
        // skipped, the cap alone left a wall of overlapping text. The
        // hovered page is named by its tooltip instead.
        if state.zoom >= 0.55 {
            let size = (10.5 * state.zoom.clamp(0.85, 1.1)).clamp(9.0, 12.0);
            let font = FontId::new(size, egui::FontFamily::Proportional);
            let mut top: Vec<u32> = visible.clone();
            top.sort_by_key(|&slot| std::cmp::Reverse(graph.nodes[slot as usize].degree));
            let mut candidates: Vec<(u32, String, egui::Rect)> = Vec::new();
            for &slot in &top {
                if candidates.len() >= LABEL_MAX {
                    break;
                }
                if hovered == Some(slot) {
                    continue; // the tooltip already names it
                }
                let node = &graph.nodes[slot as usize];
                let pos = to_screen(positions[slot as usize]);
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
                let (slot, label, rect) = &candidates[i];
                let node = &graph.nodes[*slot as usize];
                painter.text(
                    rect.min + vec2(2.0, 0.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    font.clone(),
                    if selected == Some(node.capsule) {
                        theme::ink()
                    } else {
                        theme::fade(theme::muted(), 0.9 * dim_node(*slot))
                    },
                );
            }
        }
        if let Some(slot) = hovered {
            let node = &graph.nodes[slot as usize];
            let capsule = &capsules[node.capsule];
            let anchor = to_screen(positions[slot as usize]);
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
                egui::StrokeKind::Inside,
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

    // ---- Cluster panel -------------------------------------------------------
    if graph.clusters.len() > 1 {
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
                focus_cluster(state, rect, &metrics, index);
            }
        }
    }

    // ---- Band chip + edge legend --------------------------------------------
    let band = match mode {
        LodMode::Clusters => t(locale, Key::GraphBandClusters).to_string(),
        LodMode::Density => format!("{} · {}", t(locale, Key::GraphBandDensity), visible.len()),
        LodMode::Pages => format!("{} · {}", t(locale, Key::GraphBandPages), visible.len()),
    };
    let chip_galley = painter.layout_no_wrap(band, FontId::new(9.5, theme::mono()), theme::faint());
    painter.galley(
        pos2(
            rect.right() - chip_galley.size().x - 12.0,
            rect.bottom() - 26.0,
        ),
        chip_galley,
        theme::faint(),
    );

    if mode == LodMode::Pages {
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

    // ---- Layout progress -------------------------------------------------
    if state.layout_rx.is_some() {
        let text = format!(
            "{} {}/{}",
            t(locale, Key::GraphLayingOut),
            state.layout_done,
            state.layout_total.max(1)
        );
        let galley = painter.layout_no_wrap(text, FontId::new(10.5, theme::mono()), theme::faint());
        let pos = pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 12.0);
        let box_rect = Rect::from_min_size(pos, galley.size() + vec2(16.0, 10.0));
        painter.rect(
            box_rect,
            theme::RADIUS,
            theme::fade(theme::panel(), 0.94),
            theme::hairline(),
            egui::StrokeKind::Inside,
        );
        painter.galley(box_rect.min + vec2(8.0, 5.0), galley, theme::faint());
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
                    theme::panel(),
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
                    theme::faint(),
                );
                card.add_space(5.0);
                card.add(
                    egui::Label::new(
                        RichText::new(truncate_chars(&capsule.subject, 34))
                            .family(theme::medium())
                            .size(12.5)
                            .color(theme::ink()),
                    )
                    .wrap(),
                );
                card.add_space(4.0);
                card.add(
                    egui::Label::new(
                        RichText::new(truncate_chars(&capsule.from, 32))
                            .size(10.5)
                            .color(theme::muted()),
                    )
                    .truncate(),
                );
                if !capsule.tags.is_empty() {
                    card.add_space(5.0);
                    card.horizontal_wrapped(|ui| {
                        for tag in capsule.tags.iter().take(3) {
                            let _ = tag_chip(ui, tag, theme::muted());
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
        assert!(state.focus.is_none());
    }

    #[test]
    fn cluster_colors_are_distinct_and_stable() {
        let a = cluster_color(0);
        let b = cluster_color(1);
        assert_ne!(a, b);
        assert_eq!(a, cluster_color(0));
    }

    #[test]
    fn cluster_names_localize_root() {
        assert!(!cluster_display_name("", Locale::En).is_empty());
        assert_eq!(cluster_display_name("docs", Locale::En), "docs");
    }

    #[test]
    fn density_threshold_matches_model() {
        // Below the threshold the painter draws individual pages.
        assert_ne!(
            graph::pick_mode(1.0, graph::DENSITY_VISIBLE_MAX),
            LodMode::Density
        );
        // Above it — bins.
        assert_eq!(
            graph::pick_mode(1.0, graph::DENSITY_VISIBLE_MAX + 1),
            LodMode::Density
        );
    }

    #[test]
    fn bubbles_pack_without_overlap() {
        // 24 equal bubbles of r=40 anchored on a dense grid inside a
        // 900×600 canvas: after packing no two bubbles overlap (2 px
        // tolerance) and every bubble stays inside the canvas.
        let mut anchors = Vec::new();
        for i in 0..24 {
            let col = i % 6;
            let row = i / 6;
            anchors.push([60.0 + col as f32 * 110.0, 60.0 + row as f32 * 130.0]);
        }
        let radii = vec![40.0f32; 24];
        let bounds = ([0.0f32, 0.0], [900.0, 600.0]);
        let (centers, radii) = pack_bubbles(&anchors, &radii, bounds, 0.42, 120);
        for a in 0..centers.len() {
            for b in (a + 1)..centers.len() {
                let dx = centers[a][0] - centers[b][0];
                let dy = centers[a][1] - centers[b][1];
                let dist = (dx * dx + dy * dy).sqrt();
                assert!(
                    dist + 2.0 >= radii[a] + radii[b],
                    "bubbles {a} and {b} overlap: {dist} < {} + {}",
                    radii[a],
                    radii[b]
                );
            }
            let pad = radii[a] * 0.5;
            assert!(centers[a][0] >= bounds.0[0] + pad - 0.01);
            assert!(centers[a][0] <= bounds.1[0] - pad + 0.01);
            assert!(centers[a][1] >= bounds.0[1] + pad - 0.01);
            assert!(centers[a][1] <= bounds.1[1] - pad + 0.01);
        }
    }

    #[test]
    fn bubble_radii_shrink_to_area_budget() {
        // 20 bubbles of r=50 cannot fit 0.42 of a 400×300 canvas: the
        // packing must scale them down, but never below the click floor.
        let anchors: Vec<[f32; 2]> = (0..20)
            .map(|i| [30.0 + (i % 5) as f32 * 80.0, 30.0 + (i / 5) as f32 * 70.0])
            .collect();
        let radii = vec![50.0f32; 20];
        let (centers, radii) =
            pack_bubbles(&anchors, &radii, ([0.0, 0.0], [400.0, 300.0]), 0.42, 90);
        let total: f32 = radii.iter().map(|r| r * r).sum::<f32>() * std::f32::consts::PI;
        assert!(total <= 0.42 * 400.0 * 300.0 * 1.25, "area {total}");
        assert!(radii.iter().all(|r| *r >= 7.0));
        assert!(centers.iter().all(|c| c[0].is_finite() && c[1].is_finite()));
    }

    #[test]
    fn packing_keeps_geography_and_is_deterministic() {
        // Two anchor clouds far apart: packed bubbles must stay near their
        // own cloud (geography survives), and the result is reproducible.
        let mut anchors: Vec<[f32; 2]> = (0..8)
            .map(|i| [40.0 + (i % 4) as f32 * 30.0, 40.0 + (i / 4) as f32 * 30.0])
            .collect();
        anchors
            .extend((0..8).map(|i| [700.0 + (i % 4) as f32 * 30.0, 400.0 + (i / 4) as f32 * 30.0]));
        let radii = vec![26.0f32; 16];
        let bounds = ([0.0f32, 0.0], [900.0, 600.0]);
        let (left, left_r) = pack_bubbles(&anchors, &radii, bounds, 0.42, 80);
        let (right, right_r) = pack_bubbles(&anchors, &radii, bounds, 0.42, 80);
        assert_eq!(left, right);
        assert_eq!(left_r, right_r);
        for i in 0..8 {
            assert!(
                left[i][0] < 450.0,
                "left bubble drifted right: {}",
                left[i][0]
            );
        }
        for i in 8..16 {
            assert!(
                left[i][0] > 450.0,
                "right bubble drifted left: {}",
                left[i][0]
            );
        }
    }

    #[test]
    fn label_filter_skips_overlaps() {
        let r = |min: [f32; 2], max: [f32; 2]| {
            Rect::from_min_max(pos2(min[0], min[1]), pos2(max[0], max[1]))
        };
        let rects = vec![
            r([0.0, 0.0], [100.0, 20.0]),   // kept
            r([50.0, 0.0], [150.0, 20.0]),  // overlaps the first — dropped
            r([0.0, 30.0], [80.0, 50.0]),   // disjoint — kept
            r([90.0, 25.0], [200.0, 45.0]), // disjoint — kept
        ];
        assert_eq!(keep_non_overlapping(&rects), vec![0, 2, 3]);
        assert!(keep_non_overlapping(&[]).is_empty());
    }
}
