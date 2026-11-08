//! Relationship graph: clustering, LOD model and layouts.
//!
//! The web view used to run d3-force in the browser. Here the same forces
//! are applied by hand: repulsion between nodes, springs on edges, a weak
//! pull to the centre and a little damping. Positions start from a
//! golden-angle spiral (or the persisted position cache), so the same vault
//! always produces the same picture — no random seed, no frame-rate
//! dependency, and a layout that can be unit-tested.
//!
//! Scale contract (v2.10.0): the model carries folder clusters and label-
//! propagation communities; repulsion switches to a Barnes–Hut quadtree for
//! large vaults and can run on a background thread, reporting partial
//! positions through a channel so a 200k-page vault never freezes the UI.
//! Rendering picks one of three LOD modes (`pick_mode`) from the zoom level
//! and the number of nodes inside the viewport.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};

use crate::mime::Capsule;

/// Above this node count the initial relaxation moves to a background
/// thread; below it `build()` relaxes synchronously like it always did.
pub const SYNC_LAYOUT_MAX: usize = 384;
/// Viewport node count at which the renderer drops from per-node drawing to
/// screen-space density bins.
pub const DENSITY_VISIBLE_MAX: usize = 6000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    PageParent,
    Reply,
    Reference,
    TagCluster,
}

impl EdgeKind {
    pub fn label(self) -> &'static str {
        match self {
            EdgeKind::PageParent => "page",
            EdgeKind::Reply => "reply",
            EdgeKind::Reference => "reference",
            EdgeKind::TagCluster => "tag",
        }
    }
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub source: usize,
    pub target: usize,
    pub kind: EdgeKind,
    /// Display label: tag name for clusters, nothing otherwise.
    pub label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GraphNode {
    /// Index into the capsule slice.
    pub capsule: usize,
    pub degree: usize,
    /// Normalised to `-1..=1` for both axes.
    pub pos: [f32; 2],
}

/// One folder of the vault (the parent directory of every `file_name`).
/// The vault root is the `""` key; the view maps it to a localized label.
#[derive(Clone, Debug)]
pub struct GraphCluster {
    pub key: String,
    /// Node slots (`graph.nodes` indices) inside this folder.
    pub members: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub tags: Vec<String>,
    /// Folder clusters, ordered by key. Empty when the vault is flat.
    pub clusters: Vec<GraphCluster>,
    /// Node slot → cluster index for every node.
    pub cluster_of: Vec<u32>,
    /// Edges aggregated between clusters: `(a, b, weight)` with `a < b`,
    /// deduplicated and ordered — the coarse view draws only these.
    pub cluster_edges: Vec<(u32, u32, u32)>,
    /// Label-propagation community per node slot: the fallback grouping for
    /// flat vaults and the colour source when folders don't discriminate.
    pub community_of: Vec<u32>,
    /// Number of distinct communities after compaction.
    pub communities: usize,
}

impl Graph {
    pub fn node_for(&self, capsule: usize) -> Option<&GraphNode> {
        self.nodes.iter().find(|node| node.capsule == capsule)
    }
}

fn index_by_id(capsules: &[Capsule]) -> HashMap<&str, usize> {
    capsules
        .iter()
        .enumerate()
        .map(|(index, capsule)| (capsule.message_id.as_str(), index))
        .collect()
}

/// Folder key of a capsule: the parent directory of its relative path, or
/// `""` for the vault root. Always uses `/` separators (`file_name` is
/// normalised that way on load).
pub fn folder_key(file_name: &str) -> &str {
    match file_name.rfind('/') {
        Some(index) => &file_name[..index],
        None => "",
    }
}

/// Build the full graph for a vault, optionally restricted to one tag.
pub fn build(capsules: &[Capsule], tag_filter: Option<&str>) -> Graph {
    build_with_cache(capsules, tag_filter, None)
}

/// Same, but seeds positions from a persisted [`PositionCache`] when a page
/// already has coordinates from a previous session.
pub fn build_with_cache(
    capsules: &[Capsule],
    tag_filter: Option<&str>,
    cache: Option<&PositionCache>,
) -> Graph {
    let allowed: HashSet<usize> = capsules
        .iter()
        .enumerate()
        .filter(|(_, capsule)| match tag_filter {
            Some(tag) => capsule.tags.iter().any(|t| t == tag),
            None => true,
        })
        .map(|(index, _)| index)
        .collect();

    let by_id = index_by_id(capsules);
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut seen: HashSet<(usize, usize, EdgeKind)> = HashSet::new();

    for (index, capsule) in capsules.iter().enumerate() {
        if !allowed.contains(&index) {
            continue;
        }
        if let Some(parent) = capsule
            .parent_id
            .as_deref()
            .and_then(|id| by_id.get(id).copied())
        {
            if allowed.contains(&parent) && parent != index {
                push_edge(
                    &mut edges,
                    &mut seen,
                    parent,
                    index,
                    EdgeKind::PageParent,
                    None,
                );
            }
        }
        if let Some(parent) = capsule
            .in_reply_to
            .as_deref()
            .and_then(|id| by_id.get(id).copied())
        {
            if allowed.contains(&parent) && parent != index {
                push_edge(&mut edges, &mut seen, parent, index, EdgeKind::Reply, None);
            }
        }
        for reference in &capsule.references {
            if let Some(target) = by_id.get(reference.trim()).copied() {
                if allowed.contains(&target) && target != index {
                    push_edge(
                        &mut edges,
                        &mut seen,
                        target,
                        index,
                        EdgeKind::Reference,
                        None,
                    );
                }
            }
        }
    }

    // Tag clusters: connect consecutive pages that share a tag. Linear rather
    // than a complete graph, otherwise a popular tag becomes a hairball.
    let mut tags: BTreeSet<String> = BTreeSet::new();
    for (index, capsule) in capsules.iter().enumerate() {
        if allowed.contains(&index) {
            tags.extend(capsule.tags.iter().cloned());
        }
    }

    let mut by_tag: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, capsule) in capsules.iter().enumerate() {
        if !allowed.contains(&index) {
            continue;
        }
        for tag in &capsule.tags {
            by_tag.entry(tag.as_str()).or_default().push(index);
        }
    }
    let mut tag_names: Vec<&str> = by_tag.keys().copied().collect();
    tag_names.sort();
    for tag in tag_names {
        let members = &by_tag[tag];
        for pair in members.windows(2) {
            push_edge(
                &mut edges,
                &mut seen,
                pair[0],
                pair[1],
                EdgeKind::TagCluster,
                Some(tag.to_string()),
            );
        }
    }

    let mut degrees: HashMap<usize, usize> = HashMap::new();
    for edge in &edges {
        *degrees.entry(edge.source).or_insert(0) += 1;
        *degrees.entry(edge.target).or_insert(0) += 1;
    }

    let mut indices: Vec<usize> = allowed.iter().copied().collect();
    indices.sort();
    let count = indices.len().max(1);
    let nodes: Vec<GraphNode> = indices
        .iter()
        .enumerate()
        .map(|(slot, capsule)| {
            // Golden-angle spiral: even coverage without randomness. A cached
            // position from a previous session wins, so the graph does not
            // jump between launches.
            let angle = slot as f32 * 2.399_963_1;
            let radius = (slot as f32 + 1.0).sqrt() / (count as f32).sqrt();
            let mut pos = [radius * angle.cos(), radius * angle.sin()];
            if let Some(cached) =
                cache.and_then(|cache| cache.0.get(&capsules[*capsule].message_id).copied())
            {
                pos = cached;
            }
            GraphNode {
                capsule: *capsule,
                degree: degrees.get(capsule).copied().unwrap_or(0),
                pos,
            }
        })
        .collect();

    let mut graph = Graph {
        nodes,
        edges,
        tags: tags.into_iter().collect(),
        ..Graph::default()
    };
    assign_clusters(capsules, &mut graph);
    graph.community_of = label_propagation(graph.nodes.len(), &graph.edges);
    graph.communities = graph
        .community_of
        .iter()
        .copied()
        .max()
        .map_or(0, |m| m as usize + 1);
    graph.cluster_edges = aggregate_cluster_edges(&graph);

    if graph.nodes.len() < SYNC_LAYOUT_MAX {
        graph.layout(360);
    }
    graph
}

/// Folder clusters over the allowed nodes. Nodes of a flat vault all land in
/// the `""` root cluster — the view then falls back to communities for color.
fn assign_clusters(capsules: &[Capsule], graph: &mut Graph) {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for node in &graph.nodes {
        keys.insert(folder_key(&capsules[node.capsule].file_name).to_string());
    }
    let index_of: HashMap<String, u32> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key.clone(), index as u32))
        .collect();
    graph.clusters = keys
        .into_iter()
        .map(|key| GraphCluster {
            key,
            members: Vec::new(),
        })
        .collect();
    graph.cluster_of = graph
        .nodes
        .iter()
        .map(|node| index_of[folder_key(&capsules[node.capsule].file_name)])
        .collect();
    for (slot, cluster) in graph.cluster_of.iter().enumerate() {
        graph.clusters[*cluster as usize].members.push(slot as u32);
    }
}

/// Merge every edge into its cluster pair `(a < b, weight)`. Deterministic:
/// pairs are keyed and then sorted.
fn aggregate_cluster_edges(graph: &Graph) -> Vec<(u32, u32, u32)> {
    let mut pairs: HashMap<(u32, u32), u32> = HashMap::new();
    for edge in &graph.edges {
        let a = graph.cluster_of[edge.source];
        let b = graph.cluster_of[edge.target];
        if a == b {
            continue;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        *pairs.entry(key).or_insert(0) += 1;
    }
    let mut out: Vec<(u32, u32, u32)> = pairs
        .into_iter()
        .map(|((a, b), weight)| (a, b, weight))
        .collect();
    out.sort_unstable();
    out
}

/// Label propagation: every node takes the majority community of its
/// neighbours, seeded from its own id. Deterministic tie-break (smallest id),
/// 12 iterations max, compacted so ids are `0..k`.
fn label_propagation(n: usize, edges: &[GraphEdge]) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    let mut adjacency: Vec<Vec<u32>> = vec![Vec::new(); n];
    for edge in edges {
        adjacency[edge.source].push(edge.target as u32);
        adjacency[edge.target].push(edge.source as u32);
    }
    let mut comm: Vec<u32> = (0..n as u32).collect();
    for _ in 0..12 {
        let mut next = comm.clone();
        for slot in 0..n {
            let mut counts: HashMap<u32, u32> = HashMap::new();
            for &neighbour in &adjacency[slot] {
                *counts.entry(comm[neighbour as usize]).or_insert(0) += 1;
            }
            // (count desc, community id asc) — deterministic even when the
            // neighbourhood vote is a draw.
            if let Some((best, _)) = counts.iter().max_by(|a, b| {
                (*a.1, std::cmp::Reverse(*a.0)).cmp(&(*b.1, std::cmp::Reverse(*b.0)))
            }) {
                next[slot] = *best;
            }
        }
        if next == comm {
            break;
        }
        comm = next;
    }
    // Compact: first appearance order.
    let mut renumber: HashMap<u32, u32> = HashMap::new();
    let mut out = Vec::with_capacity(n);
    for id in comm {
        let next_id = renumber.len() as u32;
        out.push(*renumber.entry(id).or_insert(next_id));
    }
    out
}

/// Uniform spatial hash over node positions, used by the renderer to cull
/// and hit-test without touching every node each frame.
#[derive(Clone, Debug, Default)]
pub struct NodeGrid {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

impl NodeGrid {
    pub fn build(positions: &[[f32; 2]], cell: f32) -> Self {
        let cell = cell.max(1e-3);
        let mut grid = Self {
            cell,
            cells: HashMap::new(),
        };
        for (slot, pos) in positions.iter().enumerate() {
            grid.cells
                .entry((
                    (pos[0] / cell).floor() as i32,
                    (pos[1] / cell).floor() as i32,
                ))
                .or_default()
                .push(slot as u32);
        }
        grid
    }

    /// Every node whose position falls inside the axis-aligned rectangle.
    /// Nodes hash into exactly one cell, so the result has no duplicates.
    pub fn query(&self, min: [f32; 2], max: [f32; 2]) -> Vec<u32> {
        let mut out = Vec::new();
        let (x0, y0) = (
            (min[0] / self.cell).floor() as i32,
            (min[1] / self.cell).floor() as i32,
        );
        let (x1, y1) = (
            (max[0] / self.cell).floor() as i32,
            (max[1] / self.cell).floor() as i32,
        );
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                if let Some(bucket) = self.cells.get(&(cx, cy)) {
                    out.extend_from_slice(bucket);
                }
            }
        }
        out
    }

    /// Closest node to `pos` within `radius` (world units).
    pub fn nearest(&self, positions: &[[f32; 2]], pos: [f32; 2], radius: f32) -> Option<u32> {
        let span = (radius / self.cell).ceil() as i32;
        let (cx, cy) = (
            (pos[0] / self.cell).floor() as i32,
            (pos[1] / self.cell).floor() as i32,
        );
        let mut best: Option<u32> = None;
        let mut best_d2 = radius * radius;
        for oy in -span..=span {
            for ox in -span..=span {
                let Some(bucket) = self.cells.get(&(cx + ox, cy + oy)) else {
                    continue;
                };
                for &slot in bucket {
                    let other = slot as usize;
                    let d2 = (positions[other][0] - pos[0]).powi(2)
                        + (positions[other][1] - pos[1]).powi(2);
                    if d2 <= best_d2 {
                        best_d2 = d2;
                        best = Some(slot);
                    }
                }
            }
        }
        best
    }
}

/// Per-cluster aggregates the coarse LOD levels draw: centroid + bounding
/// box. Rebuilt whenever the graph or its positions change, never per frame.
#[derive(Clone, Debug, Default)]
pub struct GraphMetrics {
    pub grid: NodeGrid,
    pub centroids: Vec<[f32; 2]>,
    /// Bounding box `(min, max)` per cluster, in world units.
    pub bounds: Vec<([f32; 2], [f32; 2])>,
}

pub fn build_metrics(graph: &Graph) -> GraphMetrics {
    let positions: Vec<[f32; 2]> = graph.nodes.iter().map(|node| node.pos).collect();
    let ideal = ideal_distance(graph.nodes.len());
    let mut centroids = vec![[0.0f32; 2]; graph.clusters.len()];
    let mut bounds = vec![([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]); graph.clusters.len()];
    let mut sums = vec![[0.0f64; 2]; graph.clusters.len()];
    for (slot, pos) in positions.iter().enumerate() {
        let cluster = graph.cluster_of[slot] as usize;
        sums[cluster][0] += pos[0] as f64;
        sums[cluster][1] += pos[1] as f64;
        bounds[cluster].0[0] = bounds[cluster].0[0].min(pos[0]);
        bounds[cluster].0[1] = bounds[cluster].0[1].min(pos[1]);
        bounds[cluster].1[0] = bounds[cluster].1[0].max(pos[0]);
        bounds[cluster].1[1] = bounds[cluster].1[1].max(pos[1]);
    }
    for (index, cluster) in graph.clusters.iter().enumerate() {
        if !cluster.members.is_empty() {
            let n = cluster.members.len() as f64;
            centroids[index] = [(sums[index][0] / n) as f32, (sums[index][1] / n) as f32];
        }
    }
    GraphMetrics {
        grid: NodeGrid::build(&positions, ideal * 3.0),
        centroids,
        bounds,
    }
}

/// Rendering LOD: the three modes a frame can be painted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodMode {
    /// Zoomed far out: one node per folder cluster + aggregated edges.
    Clusters,
    /// Too many nodes in the viewport for individual dots: screen-space
    /// density bins.
    Density,
    /// Individual nodes, edges and (near zoom) labels.
    Pages,
}

pub fn pick_mode(zoom: f32, visible: usize) -> LodMode {
    if zoom < 0.35 {
        LodMode::Clusters
    } else if visible > DENSITY_VISIBLE_MAX {
        LodMode::Density
    } else {
        LodMode::Pages
    }
}

pub fn ideal_distance(n: usize) -> f32 {
    (1.0 / n.max(1) as f32).sqrt().clamp(0.06, 0.35)
}

pub fn auto_iterations(n: usize) -> usize {
    match n {
        0..=1999 => 160,
        0..=19_999 => 90,
        _ => 60,
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

type Spring = (usize, usize, f32);

fn spring_list(graph: &Graph) -> Vec<Spring> {
    let slot_of: HashMap<usize, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(slot, node)| (node.capsule, slot))
        .collect();
    graph
        .edges
        .iter()
        .filter_map(|edge| {
            let a = *slot_of.get(&edge.source)?;
            let b = *slot_of.get(&edge.target)?;
            let rest = match edge.kind {
                EdgeKind::PageParent => 0.42,
                EdgeKind::Reply => 0.5,
                EdgeKind::Reference => 0.6,
                EdgeKind::TagCluster => 0.8,
            };
            Some((a, b, rest))
        })
        .collect()
}

/// Progress callback: `(done, total, positions)`.
pub type ProgressFn<'a> = &'a (dyn Fn(usize, usize, &[[f32; 2]]) + Sync + Send);

/// `iterations` of Fruchterman–Reingold style relaxation. Exact O(n²)
/// repulsion below 384 nodes (pretty, unchanged from the old behaviour),
/// Barnes–Hut above it. `report` receives periodic snapshots for the
/// background thread; `None` runs silently. Deterministic either way.
fn relax(
    seeds: &[[f32; 2]],
    springs: &[Spring],
    iterations: usize,
    report: Option<ProgressFn<'_>>,
) -> Vec<[f32; 2]> {
    let seeds: Vec<[f32; 2]> = seeds.to_vec();
    let n = seeds.len();
    if n < 2 {
        return seeds;
    }
    let mut positions = seeds;
    let mut velocities = vec![[0.0f32; 2]; n];
    let ideal = ideal_distance(n);
    let report_every = (iterations / 4).clamp(10, 40);

    let mut settled = 0u32;
    for step in 0..iterations {
        let cooling = 1.0 - step as f32 / iterations as f32;
        let mut forces = vec![[0.0f32; 2]; n];

        if n < SYNC_LAYOUT_MAX {
            for a in 0..n {
                for b in (a + 1)..n {
                    let dx = positions[a][0] - positions[b][0];
                    let dy = positions[a][1] - positions[b][1];
                    let dist2 = (dx * dx + dy * dy).max(1e-4);
                    let force = (ideal * ideal) / dist2;
                    let inv = 1.0 / dist2.sqrt();
                    let fx = dx * inv * force;
                    let fy = dy * inv * force;
                    forces[a][0] += fx;
                    forces[a][1] += fy;
                    forces[b][0] -= fx;
                    forces[b][1] -= fy;
                }
            }
        } else {
            barnes_hut_repulsion(&positions, ideal, &mut forces);
        }

        // Springs.
        for (a, b, rest) in springs {
            let dx = positions[*b][0] - positions[*a][0];
            let dy = positions[*b][1] - positions[*a][1];
            let dist = (dx * dx + dy * dy).sqrt().max(1e-3);
            let force = (dist - rest) * 0.45;
            let ux = dx / dist;
            let uy = dy / dist;
            forces[*a][0] += ux * force;
            forces[*a][1] += uy * force;
            forces[*b][0] -= ux * force;
            forces[*b][1] -= uy * force;
        }

        for i in 0..n {
            // Weak gravity so disconnected islands stay on screen.
            forces[i][0] += -positions[i][0] * 0.06;
            forces[i][1] += -positions[i][1] * 0.06;
            velocities[i][0] = (velocities[i][0] + forces[i][0] * 0.5) * 0.82;
            velocities[i][1] = (velocities[i][1] + forces[i][1] * 0.5) * 0.82;
            let max_step = 0.12 * cooling;
            positions[i][0] += velocities[i][0].clamp(-max_step, max_step);
            positions[i][1] += velocities[i][1].clamp(-max_step, max_step);
        }

        // A settled layout never gets better: bail out early instead of
        // burning the full iteration budget every rebuild.
        let max_disp = velocities
            .iter()
            .map(|v| v[0].abs().max(v[1].abs()))
            .fold(0.0f32, f32::max);
        if step >= 40 && max_disp < 1e-3 {
            settled += 1;
            if settled >= 3 {
                break;
            }
        } else {
            settled = 0;
        }

        if let Some(report) = report {
            if (step + 1) % report_every == 0 || step + 1 == iterations {
                report(step + 1, iterations, &positions);
            }
        }
    }

    normalise(&mut positions);
    // De-clump: repulsion keeps nodes apart during relaxation, but parent
    // chains still converge into piles where dots stack on one pixel. Push
    // overlapping nodes apart so the pages view shows a scatter, not a ball.
    // The target has to be feasible — n discs only spread so far in a unit
    // box — so the ideal distance is capped by the packing budget.
    let feasible = (0.78 / n as f32).sqrt();
    separate(&mut positions, ideal.min(feasible) * 0.9, 16);
    positions
}

/// Push apart every pair of nodes closer than `min_dist`. Grid-accelerated
/// (only near pairs are compared) and deterministic: fixed order, and
/// coincident points split along the golden-angle direction of the slot.
fn separate(positions: &mut [[f32; 2]], min_dist: f32, iterations: usize) {
    let n = positions.len();
    if n < 2 || min_dist <= 0.0 {
        return;
    }
    let min2 = min_dist * min_dist;
    for _ in 0..iterations {
        let grid = NodeGrid::build(positions, min_dist);
        let mut moved = 0.0f32;
        for slot in 0..n {
            let pos = positions[slot];
            let near = grid.query(
                [pos[0] - min_dist, pos[1] - min_dist],
                [pos[0] + min_dist, pos[1] + min_dist],
            );
            for other in near {
                let other = other as usize;
                if other <= slot {
                    continue;
                }
                let dx = positions[slot][0] - positions[other][0];
                let dy = positions[slot][1] - positions[other][1];
                let d2 = dx * dx + dy * dy;
                if d2 >= min2 {
                    continue;
                }
                if d2 > 1e-12 {
                    let d = d2.sqrt();
                    let push = (min_dist - d) * 0.55;
                    let (ux, uy) = (dx / d, dy / d);
                    positions[slot][0] += ux * push;
                    positions[slot][1] += uy * push;
                    positions[other][0] -= ux * push;
                    positions[other][1] -= uy * push;
                } else {
                    // Coincident: opposite pushes cancel out, so each node
                    // walks its own golden-angle direction — deterministic.
                    let (sa, so) = (slot as f32 * 2.399_963_2, other as f32 * 2.399_963_2);
                    let push = min_dist * 0.55;
                    positions[slot][0] += sa.sin() * push;
                    positions[slot][1] += sa.cos() * push;
                    positions[other][0] += so.sin() * push;
                    positions[other][1] += so.cos() * push;
                }
                moved += min_dist;
            }
        }
        if moved < 1e-4 {
            break;
        }
    }
}

impl Graph {
    /// Blocking relaxation — small graphs and tests. Large vaults should use
    /// [`layout_in_background`] instead.
    pub fn layout(&mut self, iterations: usize) {
        let springs = spring_list(self);
        let seeds: Vec<[f32; 2]> = self.nodes.iter().map(|node| node.pos).collect();
        let positions = relax(&seeds, &springs, iterations, None);
        for (node, pos) in self.nodes.iter_mut().zip(positions) {
            node.pos = pos;
        }
    }
}

/// Progress message from the background layout thread. `positions` is a full
/// snapshot in node-slot order.
pub enum LayoutMsg {
    Progress {
        done: usize,
        total: usize,
        positions: Vec<[f32; 2]>,
    },
    Done(Vec<[f32; 2]>),
}

/// Run the relaxation on a background thread for big vaults. The UI keeps
/// drawing (spiral/cache positions) and swaps in snapshots as they arrive.
pub fn layout_in_background(
    graph: std::sync::Arc<Graph>,
    iterations: usize,
) -> Receiver<LayoutMsg> {
    let (tx, rx): (Sender<LayoutMsg>, _) = std::sync::mpsc::channel();
    let seeds: Vec<[f32; 2]> = graph.nodes.iter().map(|node| node.pos).collect();
    let springs = spring_list(&graph);
    std::thread::Builder::new()
        .name("graph-layout".into())
        .spawn(move || {
            let report = |done: usize, total: usize, positions: &[[f32; 2]]| {
                let _ = tx.send(LayoutMsg::Progress {
                    done,
                    total,
                    positions: positions.to_vec(),
                });
            };
            let final_positions = relax(&seeds, &springs, iterations, Some(&report));
            let _ = tx.send(LayoutMsg::Done(final_positions));
        })
        .expect("spawn graph layout thread");
    rx
}

// ---------------------------------------------------------------------------
// Barnes–Hut quadtree
// ---------------------------------------------------------------------------

const BH_THETA: f32 = 1.15;
const BH_MAX_DEPTH: u8 = 48;

#[derive(Clone, Copy)]
struct Quad {
    children: [i32; 4],
    mass: f32,
    /// Centre of mass.
    com_x: f32,
    com_y: f32,
    point: i32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Quad {
    fn leaf(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            children: [-1; 4],
            mass: 0.0,
            com_x: 0.0,
            com_y: 0.0,
            point: -1,
            x0,
            y0,
            x1,
            y1,
        }
    }

    fn side(&self) -> f32 {
        self.x1 - self.x0
    }

    fn child_index(&self, x: f32, y: f32) -> usize {
        let mx = (self.x0 + self.x1) / 2.0;
        let my = (self.y0 + self.y1) / 2.0;
        match (x >= mx, y >= my) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }

    fn child_bounds(&self, index: usize) -> (f32, f32, f32, f32) {
        let mx = (self.x0 + self.x1) / 2.0;
        let my = (self.y0 + self.y1) / 2.0;
        match index {
            0 => (self.x0, self.y0, mx, my),
            1 => (mx, self.y0, self.x1, my),
            2 => (self.x0, my, mx, self.y1),
            _ => (mx, my, self.x1, self.y1),
        }
    }

    fn add_mass(&mut self, x: f32, y: f32) {
        let new_mass = self.mass + 1.0;
        self.com_x = (self.com_x * self.mass + x) / new_mass;
        self.com_y = (self.com_y * self.mass + y) / new_mass;
        self.mass = new_mass;
    }
}

fn barnes_hut_repulsion(positions: &[[f32; 2]], ideal: f32, forces: &mut [[f32; 2]]) {
    let n = positions.len();
    debug_assert_eq!(n, forces.len());
    // Square bounds, slightly padded, centred on the point cloud.
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for pos in positions {
        for axis in 0..2 {
            min[axis] = min[axis].min(pos[axis]);
            max[axis] = max[axis].max(pos[axis]);
        }
    }
    let side = (max[0] - min[0]).max(max[1] - min[1]).max(1e-3) * 1.02;
    let cx = (min[0] + max[0]) / 2.0;
    let cy = (min[1] + max[1]) / 2.0;
    let (x0, y0, x1, y1) = (
        cx - side / 2.0,
        cy - side / 2.0,
        cx + side / 2.0,
        cy + side / 2.0,
    );

    let mut tree: Vec<Quad> = Vec::with_capacity(n * 2);
    tree.push(Quad::leaf(x0, y0, x1, y1));

    // Insert in slot order → deterministic tree → deterministic forces.
    for (slot, pos) in positions.iter().enumerate() {
        let _ = pos;
        insert_point(&mut tree, 0, slot, positions, 0);
    }
    let mut stack: Vec<usize> = Vec::with_capacity(64);
    for (slot, pos) in positions.iter().enumerate() {
        stack.clear();
        stack.push(0);
        while let Some(node) = stack.pop() {
            let quad = tree[node];
            if quad.mass == 0.0 {
                continue;
            }
            if quad.point >= 0 {
                let other = quad.point as usize;
                if other != slot {
                    apply_repulsion(pos, positions[other], 1.0, ideal, &mut forces[slot][..]);
                }
                continue;
            }
            let dx = quad.com_x - pos[0];
            let dy = quad.com_y - pos[1];
            let dist2 = dx * dx + dy * dy;
            let side = quad.side();
            // Far enough → treat the whole cell as one body; too close or
            // degenerate (all points stacked) → descend.
            if dist2 > 0.0 && side * side < BH_THETA * BH_THETA * dist2 {
                apply_repulsion(
                    pos,
                    [quad.com_x, quad.com_y],
                    quad.mass,
                    ideal,
                    &mut forces[slot][..],
                );
                continue;
            }
            for &child in &quad.children {
                if child >= 0 {
                    stack.push(child as usize);
                }
            }
        }
    }
}

fn apply_repulsion(from: &[f32; 2], to: [f32; 2], mass: f32, ideal: f32, force: &mut [f32]) {
    let dx = from[0] - to[0];
    let dy = from[1] - to[1];
    let dist2 = (dx * dx + dy * dy).max(1e-4);
    let f = mass * (ideal * ideal) / dist2;
    let inv = 1.0 / dist2.sqrt();
    force[0] += dx * inv * f;
    force[1] += dy * inv * f;
}

fn insert_point(tree: &mut Vec<Quad>, node: usize, slot: usize, positions: &[[f32; 2]], depth: u8) {
    let xy = positions[slot];
    let (x, y) = (xy[0], xy[1]);
    // Depth guard: coincident points would otherwise recurse forever. A
    // saturated micro-cell just accumulates mass at its centre of mass.
    if depth >= BH_MAX_DEPTH {
        tree[node].add_mass(x, y);
        return;
    }
    if tree[node].point == -1 && tree[node].mass == 0.0 {
        tree[node].point = slot as i32;
        tree[node].add_mass(x, y);
        return;
    }
    if tree[node].point >= 0 {
        // Split the leaf: the previous point moves one level down (its mass
        // is already aggregated into this node), then the newcomer follows.
        let old = tree[node].point as usize;
        tree[node].point = -1;
        descend(tree, node, old, positions, depth);
    }
    tree[node].add_mass(x, y);
    descend(tree, node, slot, positions, depth);
}

/// Route one point into the child quadrant of `node` (mass is the caller's
/// business — `insert_point` counts it at the right level).
fn descend(tree: &mut Vec<Quad>, node: usize, slot: usize, positions: &[[f32; 2]], depth: u8) {
    let xy = positions[slot];
    let (x, y) = (xy[0], xy[1]);
    let index = tree[node].child_index(x, y);
    if tree[node].children[index] < 0 {
        let (x0, y0, x1, y1) = tree[node].child_bounds(index);
        tree.push(Quad::leaf(x0, y0, x1, y1));
        let child = tree.len() - 1;
        tree[node].children[index] = child as i32;
    }
    let child = tree[node].children[index] as usize;
    insert_point(tree, child, slot, positions, depth + 1);
}

/// Scale positions into `-1..=1`, preserving aspect ratio.
fn normalise(positions: &mut [[f32; 2]]) {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for pos in positions.iter() {
        for axis in 0..2 {
            min[axis] = min[axis].min(pos[axis]);
            max[axis] = max[axis].max(pos[axis]);
        }
    }
    let span = (max[0] - min[0]).max(max[1] - min[1]);
    if !span.is_finite() || span <= f32::EPSILON {
        for pos in positions.iter_mut() {
            *pos = [0.0, 0.0];
        }
        return;
    }
    for pos in positions.iter_mut() {
        let cx = (min[0] + max[0]) / 2.0;
        let cy = (min[1] + max[1]) / 2.0;
        pos[0] = ((pos[0] - cx) / span) * 2.0;
        pos[1] = ((pos[1] - cy) / span) * 2.0;
    }
}

// ---------------------------------------------------------------------------
// Position cache
// ---------------------------------------------------------------------------

/// Message-ID → world position, persisted next to `ui_state.json` so the
/// graph opens where it was left and a re-layout starts from something
/// reasonable instead of a bare spiral.
#[derive(Clone, Debug, Default)]
pub struct PositionCache(pub HashMap<String, [f32; 2]>);

impl PositionCache {
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        let entries: Vec<(String, [f32; 2])> = serde_json::from_str(&raw).ok()?;
        Some(Self(entries.into_iter().collect()))
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut entries: Vec<(&String, &[f32; 2])> = self.0.iter().map(|(k, v)| (k, v)).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let json = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string());
        std::fs::write(path, json)
    }

    pub fn from_graph(graph: &Graph, capsules: &[Capsule]) -> Self {
        Self(
            graph
                .nodes
                .iter()
                .map(|node| (capsules[node.capsule].message_id.clone(), node.pos))
                .collect(),
        )
    }
}

fn push_edge(
    edges: &mut Vec<GraphEdge>,
    seen: &mut HashSet<(usize, usize, EdgeKind)>,
    source: usize,
    target: usize,
    kind: EdgeKind,
    label: Option<String>,
) {
    if !seen.insert((source, target, kind)) {
        return;
    }
    edges.push(GraphEdge {
        source,
        target,
        kind,
        label,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capsule(id: &str, parent: Option<&str>, reply: Option<&str>, tags: &[&str]) -> Capsule {
        let mut capsule = Capsule::blank(id.to_string(), format!("{id}.eml"), id.to_string());
        capsule.parent_id = parent.map(str::to_string);
        capsule.in_reply_to = reply.map(str::to_string);
        capsule.tags = tags.iter().map(|tag| tag.to_string()).collect();
        capsule
    }

    #[test]
    fn builds_parent_reply_and_tag_edges() {
        let capsules = vec![
            capsule("a", None, None, &["#plan"]),
            capsule("b", Some("a"), None, &["#plan"]),
            capsule("c", Some("b"), Some("a"), &["#plan", "#spec"]),
            capsule("d", None, None, &["#spec"]),
        ];
        let graph = build(&capsules, None);
        assert_eq!(graph.nodes.len(), 4);
        assert!(graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::PageParent && edge.source == 0 && edge.target == 1));
        assert!(graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Reply && edge.source == 0 && edge.target == 2));
        assert!(graph.edges.iter().any(
            |edge| edge.kind == EdgeKind::TagCluster && edge.label.as_deref() == Some("#spec")
        ));
        assert_eq!(graph.tags, vec!["#plan".to_string(), "#spec".to_string()]);
    }

    #[test]
    fn tag_filter_keeps_only_members() {
        let capsules = vec![
            capsule("a", None, None, &["#plan"]),
            capsule("b", Some("a"), None, &["#plan"]),
            capsule("c", None, None, &["#other"]),
        ];
        let graph = build(&capsules, Some("#plan"));
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.tags, vec!["#plan".to_string()]);
    }

    #[test]
    fn layout_is_deterministic_and_bounded() {
        let capsules = vec![
            capsule("a", None, None, &["#x"]),
            capsule("b", Some("a"), None, &["#x"]),
            capsule("c", Some("b"), None, &["#x"]),
            capsule("d", None, None, &["#x"]),
        ];
        let first = build(&capsules, None);
        let second = build(&capsules, None);
        for (a, b) in first.nodes.iter().zip(second.nodes.iter()) {
            assert!((a.pos[0] - b.pos[0]).abs() < 1e-6);
            assert!((a.pos[1] - b.pos[1]).abs() < 1e-6);
            assert!(a.pos[0].abs() <= 1.0001);
            assert!(a.pos[1].abs() <= 1.0001);
        }
    }

    fn folder_capsule(id: &str, file_name: &str) -> Capsule {
        let mut capsule = Capsule::blank(id.to_string(), file_name.to_string(), id.to_string());
        capsule.message_id = format!("{id}@folders.test");
        capsule
    }

    #[test]
    fn folder_key_derives_clusters() {
        assert_eq!(folder_key("root.eml"), "");
        assert_eq!(folder_key("modules/module_main.eml"), "modules");
        assert_eq!(folder_key("a/b/c.eml"), "a/b");
    }

    #[test]
    fn clusters_group_pages_by_folder_and_aggregate_edges() {
        let mut a = folder_capsule("a", "docs/a.eml");
        let mut b = folder_capsule("b", "docs/b.eml");
        let mut c = folder_capsule("c", "src/c.eml");
        let d = folder_capsule("d", "d.eml");
        b.parent_id = Some(a.message_id.clone());
        c.in_reply_to = Some(a.message_id.clone());
        let capsules = vec![a, b, c, d];
        let graph = build(&capsules, None);
        assert_eq!(graph.clusters.len(), 3);
        let docs = graph
            .clusters
            .iter()
            .find(|cluster| cluster.key == "docs")
            .expect("docs cluster");
        assert_eq!(docs.members.len(), 2);
        // The b←a parent edge is intra-docs; the only cross-folder edge is
        // the c→a reply, so docs↔src carries weight 1 and root stays alone.
        assert_eq!(graph.cluster_edges.len(), 1);
        assert_eq!(graph.cluster_edges[0].2, 1);
    }

    #[test]
    fn communities_split_dense_groups() {
        // Two triangles joined by one bridge: label propagation must end up
        // with exactly two communities.
        let mut capsules = Vec::new();
        for id in ["a", "b", "c", "d", "e", "f"] {
            capsules.push(capsule(id, None, None, &[]));
        }
        // a↔b↔c↔a and d↔e↔f↔d plus the bridge c→d.
        capsules[1].in_reply_to = Some("a".into());
        capsules[2].in_reply_to = Some("b".into());
        capsules[0].in_reply_to = Some("c".into());
        capsules[4].in_reply_to = Some("d".into());
        capsules[5].in_reply_to = Some("e".into());
        capsules[3].in_reply_to = Some("f".into());
        capsules[2].references.push("d".into());
        let graph = build(&capsules, None);
        let comm_a = graph.community_of[0];
        let comm_d = graph.community_of[3];
        assert_ne!(comm_a, comm_d, "two dense groups must not merge");
        assert_eq!(graph.community_of[0], graph.community_of[1]);
        assert_eq!(graph.community_of[3], graph.community_of[4]);
    }

    #[test]
    fn grid_query_and_nearest_are_exact() {
        let positions: Vec<[f32; 2]> = (0..100)
            .map(|i| [(i % 10) as f32 * 0.1, (i / 10) as f32 * 0.1])
            .collect();
        let grid = NodeGrid::build(&positions, 0.1);
        // floor(0.35/0.1) = 3 → cells (1..=3)² → exactly the nine nodes at
        // x,y ∈ {0.1, 0.2, 0.3}.
        let inside = grid.query([0.15, 0.15], [0.35, 0.35]);
        assert_eq!(inside.len(), 9);
        for &slot in &inside {
            let p = positions[slot as usize];
            assert!(p[0] >= 0.1 && p[0] <= 0.5 && p[1] >= 0.1 && p[1] <= 0.5);
        }
        // Query point (0.12, 0.11): slot 11 sits at (0.1, 0.1) — the closest
        // inside the 0.05-radius window; slot 1 at (0.1, 0.0) is farther.
        let nearest = grid.nearest(&positions, [0.12, 0.11], 0.05);
        assert_eq!(nearest, Some(11));
    }

    #[test]
    fn barnes_hut_is_deterministic_and_bounded() {
        // Chain of 600: exact path is unused (≥384), so compare two BH runs
        // for determinism and keep positions bounded.
        let capsules: Vec<Capsule> = (0..600)
            .map(|i| {
                let mut capsule = capsule(&format!("p{i}"), None, None, &[]);
                if i > 0 {
                    capsule.parent_id = Some(format!("p{}", i - 1));
                }
                capsule
            })
            .collect();
        let first = build(&capsules, None);
        let second = build(&capsules, None);
        for (a, b) in first.nodes.iter().zip(second.nodes.iter()) {
            assert_eq!(a.pos, b.pos);
            // The de-clump pass may push a pile slightly past the old unit
            // box; the view fits the content bounds, so a small overshoot
            // is fine — it just must stay bounded.
            assert!(a.pos[0].abs() <= 1.5 && a.pos[1].abs() <= 1.5);
        }
    }

    #[test]
    fn coincident_nodes_get_separated() {
        // 60 nodes piled on one point: after the pass every pair sits at
        // (almost) the requested distance and the pile has spread out.
        let mut positions = vec![[0.5f32, 0.5]; 60];
        let min_dist = (0.78f32 / 60.0).sqrt() * 0.9;
        separate(&mut positions, min_dist, 40);
        let mut worst = f32::INFINITY;
        for a in 0..positions.len() {
            for b in (a + 1)..positions.len() {
                let dx = positions[a][0] - positions[b][0];
                let dy = positions[a][1] - positions[b][1];
                worst = worst.min((dx * dx + dy * dy).sqrt());
            }
        }
        assert!(worst > min_dist * 0.6, "worst pair {worst} vs {min_dist}");
        let span = positions
            .iter()
            .fold(0.0f32, |m, p| m.max(p[0].abs().max(p[1].abs())));
        assert!(span > 0.25, "pile did not spread, span {span}");
    }

    #[test]
    fn separate_is_deterministic() {
        let mut a: Vec<[f32; 2]> = (0..120)
            .map(|i| [(i % 10) as f32 * 0.01, (i / 10) as f32 * 0.01])
            .collect();
        let mut b = a.clone();
        separate(&mut a, 0.05, 12);
        separate(&mut b, 0.05, 12);
        assert_eq!(a, b);
    }

    #[test]
    fn background_layout_matches_sync_result() {
        let capsules: Vec<Capsule> = (0..500)
            .map(|i| {
                let mut capsule = capsule(&format!("b{i}"), None, None, &[]);
                if i > 0 {
                    capsule.parent_id = Some(format!("b{}", i - 1));
                }
                capsule
            })
            .collect();
        let graph = std::sync::Arc::new(build(&capsules, None));
        let rx = layout_in_background(graph.clone(), 60);
        let mut final_positions = None;
        while let Ok(msg) = rx.recv() {
            match msg {
                LayoutMsg::Progress { .. } => {}
                LayoutMsg::Done(positions) => {
                    final_positions = Some(positions);
                    break;
                }
            }
        }
        let from_thread = final_positions.expect("thread must finish");
        let mut sync = (*graph).clone();
        sync.layout(60);
        for (a, b) in from_thread.iter().zip(sync.nodes.iter()) {
            assert_eq!(*a, b.pos);
        }
    }

    #[test]
    fn position_cache_roundtrip() {
        let mut cache = PositionCache::default();
        cache.0.insert("a@x".into(), [0.25, -0.5]);
        let path = std::env::temp_dir().join(format!("cc_graph_cache_{}.json", std::process::id()));
        cache.save(&path).expect("save");
        let loaded = PositionCache::load(&path).expect("load");
        assert_eq!(loaded.0["a@x"], [0.25, -0.5]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cached_positions_seed_the_build() {
        // ≥ SYNC_LAYOUT_MAX nodes: build() skips the synchronous relaxation,
        // so a cached position survives verbatim into the model.
        let capsules: Vec<Capsule> = (0..400)
            .map(|i| capsule(&format!("c{i}"), None, None, &[]))
            .collect();
        let mut cache = PositionCache::default();
        cache.0.insert("c0".into(), [0.75, 0.75]);
        let graph = build_with_cache(&capsules, None, Some(&cache));
        let first = graph
            .nodes
            .iter()
            .find(|node| node.capsule == 0)
            .expect("node c0");
        assert_eq!(first.pos, [0.75, 0.75]);
    }

    #[test]
    fn pick_mode_thresholds() {
        assert_eq!(pick_mode(0.1, 3), LodMode::Clusters);
        assert_eq!(pick_mode(0.34, 0), LodMode::Clusters);
        // Zoomed in but everything still on screen → density bins, because
        // 200k individual dots would stall the painter.
        assert_eq!(pick_mode(1.0, DENSITY_VISIBLE_MAX + 1), LodMode::Density);
        assert_eq!(pick_mode(0.4, 40), LodMode::Pages);
        assert_eq!(pick_mode(3.0, 12), LodMode::Pages);
    }

    /// The scale smoke: 12k pages across 24 folders, chains + cross-folder
    /// references — the model pipeline (build, clusters, communities, grid)
    /// stays fast and correct far past the size a demo vault reaches, and a
    /// 1.5k slice layouts through the Barnes–Hut path.
    #[test]
    fn twelve_k_pages_build_query_and_slice_layout() {
        let folders = 24;
        let per_folder = 500;
        let capsules: Vec<Capsule> = (0..folders * per_folder)
            .map(|i| {
                let folder = format!("f{}", i / per_folder);
                let mut capsule = Capsule::blank(
                    format!("s{i}@stress.local"),
                    format!("{folder}/p{}.eml", i % per_folder),
                    format!("Stress page {i}"),
                );
                if i % per_folder > 0 {
                    capsule.parent_id = Some(format!("s{}@stress.local", i - 1));
                }
                if i >= per_folder {
                    capsule
                        .references
                        .push(format!("s{}@stress.local", i - per_folder));
                }
                capsule
            })
            .collect();
        let graph = build(&capsules, None);
        assert_eq!(graph.nodes.len(), folders * per_folder);
        assert_eq!(graph.clusters.len(), folders);
        assert!(graph.cluster_edges.len() >= folders - 1);
        let metrics = build_metrics(&graph);
        let positions: Vec<[f32; 2]> = graph.nodes.iter().map(|n| n.pos).collect();
        // The grid must find the whole graph in one full-extent query…
        assert_eq!(
            metrics.grid.query([-2.0, -2.0], [2.0, 2.0]).len(),
            graph.nodes.len()
        );
        // …and hit-test near a known node.
        let hit = metrics
            .grid
            .nearest(&positions, graph.nodes[7777].pos, 1e-3);
        assert!(hit.is_some());
        // A 1.5k slice exercises the BH layout path end to end.
        let mut slice = build(&capsules[..1500], None);
        slice.layout(auto_iterations(1500));
        assert!(slice
            .nodes
            .iter()
            .all(|node| node.pos[0].abs() <= 1.0001 && node.pos[1].abs() <= 1.0001));
    }
}
