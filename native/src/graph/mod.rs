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

pub mod layout;
pub mod metrics;

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::mime::Capsule;

#[allow(unused_imports)]
pub use layout::{auto_iterations, ideal_distance, layout_in_background, LayoutMsg, ProgressFn};
#[allow(unused_imports)]
pub use metrics::{build_metrics, GraphMetrics, NodeGrid};

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

    /// Blocking relaxation — small graphs and tests. Large vaults should use
    /// [`layout_in_background`] instead.
    pub fn layout(&mut self, iterations: usize) {
        let springs = layout::spring_list(self);
        let seeds: Vec<[f32; 2]> = self.nodes.iter().map(|node| node.pos).collect();
        let positions = layout::relax(&seeds, &springs, iterations, None);
        for (node, pos) in self.nodes.iter_mut().zip(positions) {
            node.pos = pos;
        }
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
    metrics::assign_clusters(capsules, &mut graph);
    graph.community_of = metrics::label_propagation(graph.nodes.len(), &graph.edges);
    graph.communities = graph
        .community_of
        .iter()
        .copied()
        .max()
        .map_or(0, |m| m as usize + 1);
    graph.cluster_edges = metrics::aggregate_cluster_edges(&graph);

    if graph.nodes.len() < SYNC_LAYOUT_MAX {
        graph.layout(360);
    }
    graph
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
        assert_eq!(graph.cluster_edges.len(), 1);
        assert_eq!(graph.cluster_edges[0].2, 1);
    }

    #[test]
    fn communities_split_dense_groups() {
        let mut capsules = Vec::new();
        for id in ["a", "b", "c", "d", "e", "f"] {
            capsules.push(capsule(id, None, None, &[]));
        }
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
    fn barnes_hut_is_deterministic_and_bounded() {
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
            assert!(a.pos[0].abs() <= 1.5 && a.pos[1].abs() <= 1.5);
        }
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
        assert_eq!(pick_mode(1.0, DENSITY_VISIBLE_MAX + 1), LodMode::Density);
        assert_eq!(pick_mode(0.4, 40), LodMode::Pages);
        assert_eq!(pick_mode(3.0, 12), LodMode::Pages);
    }

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
        assert_eq!(
            metrics.grid.query([-2.0, -2.0], [2.0, 2.0]).len(),
            graph.nodes.len()
        );
        let hit = metrics
            .grid
            .nearest(&positions, graph.nodes[7777].pos, 1e-3);
        assert!(hit.is_some());
        let mut slice = build(&capsules[..1500], None);
        slice.layout(auto_iterations(1500));
        assert!(slice
            .nodes
            .iter()
            .all(|node| node.pos[0].abs() <= 1.0001 && node.pos[1].abs() <= 1.0001));
    }
}
