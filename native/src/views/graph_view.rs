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
        let fit = |p: [f32; 2]| -> egui::Pos2 {
            let fx = (p[0] - cmin[0]) / span_x;
            let fy = (p[1] - cmin[1]) / span_y;
            pos2(
                rect.left() + (0.08 + 0.84 * fx.clamp(0.0, 1.0)) * rect.width(),
                rect.top() + (0.08 + 0.84 * fy.clamp(0.0, 1.0)) * rect.height(),
            )
        };
        let cluster_radius =
            |count: usize| -> f32 { (9.0 + (count as f32).sqrt() * 1.7).min(30.0) };
        if state.edges {
            for &(a, b, weight) in &graph.cluster_edges {
                if graph.clusters[a as usize].members.is_empty()
                    || graph.clusters[b as usize].members.is_empty()
                {
                    continue;
                }
                let pa = fit(metrics.centroids[a as usize]);
                let pb = fit(metrics.centroids[b as usize]);
                let dim = dim_cluster(a).min(dim_cluster(b));
                let width = (0.8 + (weight as f32).ln().max(0.0)).clamp(0.8, 3.0);
                painter.line_segment(
                    [pa, pb],
                    Stroke::new(width, theme::fade(theme::accent(), 0.4 * dim)),
                );
            }
        }
        let mut order: Vec<u32> = (0..graph.clusters.len() as u32).collect();
        order.sort_by_key(|&index| graph.clusters[index as usize].members.len());
        let mut hovered_cluster: Option<u32> = None;
        if let Some(pointer) = response.hover_pos() {
            let mut best_d2 = f32::INFINITY;
            for &index in &order {
                let pos = fit(metrics.centroids[index as usize]);
                let node_radius = cluster_radius(graph.clusters[index as usize].members.len());
                let d2 = pointer.distance_sq(pos);
                if d2 <= (node_radius + 6.0).powi(2) && d2 < best_d2 {
                    best_d2 = d2;
                    hovered_cluster = Some(index);
                }
            }
        }
        for &index in &order {
            let count = graph.clusters[index as usize].members.len();
            let pos = fit(metrics.centroids[index as usize]);
            let node_radius = cluster_radius(count);
            let color = cluster_color(index);
            let dim = dim_cluster(index);
            if hovered_cluster == Some(index) || focus == Some(index) {
                painter.circle_filled(pos, node_radius + 5.0, theme::fade(color, 0.2 * dim));
            }
            painter.circle_filled(pos, node_radius, theme::fade(color, dim));
            painter.circle_stroke(
                pos,
                node_radius,
                Stroke::new(1.0, theme::fade(theme::bg(), 0.8)),
            );
        }
        // Labels: the largest clusters only, so the overview stays a map and
        // not a wall of text.
        let mut top: Vec<u32> = order.clone();
        top.sort_by_key(|&index| std::cmp::Reverse(graph.clusters[index as usize].members.len()));
        for &index in top.iter().take(14) {
            let count = graph.clusters[index as usize].members.len();
            let pos = fit(metrics.centroids[index as usize]);
            let node_radius = cluster_radius(count);
            let name = cluster_display_name(&graph.clusters[index as usize].key, locale);
            painter.text(
                pos2(pos.x, pos.y + node_radius + 5.0),
                egui::Align2::CENTER_TOP,
                format!("{} · {}", truncate_chars(&name, 22), count),
                FontId::new(10.5, theme::medium()),
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
        // Labels: the most connected visible pages, capped per frame; the
        // hovered page is named by its tooltip instead.
        if state.zoom >= 0.55 {
            let size = (10.5 * state.zoom.clamp(0.85, 1.1)).clamp(9.0, 12.0);
            let mut top: Vec<u32> = visible.clone();
            top.sort_by_key(|&slot| std::cmp::Reverse(graph.nodes[slot as usize].degree));
            let mut labeled = 0usize;
            for &slot in &top {
                if labeled >= LABEL_MAX {
                    break;
                }
                if hovered == Some(slot) {
                    continue; // the tooltip already names it
                }
                labeled += 1;
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
                painter.text(
                    pos2(pos.x, pos.y + node_radius + 6.0),
                    egui::Align2::CENTER_TOP,
                    label,
                    FontId::new(size, egui::FontFamily::Proportional),
                    if selected == Some(node.capsule) {
                        theme::ink()
                    } else {
                        theme::fade(theme::muted(), 0.9 * dim_node(slot))
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
}
