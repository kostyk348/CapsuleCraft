//! Graph metrics: spatial grid, clustering, and communities.

use std::collections::{BTreeSet, HashMap};

use crate::mime::Capsule;

use super::{folder_key, ideal_distance, Graph, GraphCluster, GraphEdge};

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
    /// Incident edges and neighbors per node slot: `slot -> Vec<(neighbor_slot, edge_index)>`.
    pub adjacency: Vec<Vec<(u32, usize)>>,
}

impl GraphMetrics {
    /// Breadth-first search finding all nodes reachable from `start` within `depth` hops (inclusive).
    pub fn neighborhood_bfs(&self, start: u32, depth: usize) -> std::collections::HashSet<u32> {
        use std::collections::{HashSet, VecDeque};
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back((start, 0));

        while let Some((curr, d)) = queue.pop_front() {
            if d < depth {
                if let Some(neighbors) = self.adjacency.get(curr as usize) {
                    for &(next, _) in neighbors {
                        if visited.insert(next) {
                            queue.push_back((next, d + 1));
                        }
                    }
                }
            }
        }
        visited
    }
}

pub fn build_metrics(graph: &Graph) -> GraphMetrics {
    let positions: Vec<[f32; 2]> = graph.nodes.iter().map(|node| node.pos).collect();
    let ideal = ideal_distance(graph.nodes.len());
    let mut centroids = vec![[0.0f32; 2]; graph.clusters.len()];
    let mut bounds = vec![([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]); graph.clusters.len()];
    let mut sums = vec![[0.0f64; 2]; graph.clusters.len()];
    for (slot, pos) in positions.iter().enumerate() {
        if let Some(&cluster) = graph.cluster_of.get(slot) {
            let cluster = cluster as usize;
            if cluster < graph.clusters.len() {
                sums[cluster][0] += pos[0] as f64;
                sums[cluster][1] += pos[1] as f64;
                bounds[cluster].0[0] = bounds[cluster].0[0].min(pos[0]);
                bounds[cluster].0[1] = bounds[cluster].0[1].min(pos[1]);
                bounds[cluster].1[0] = bounds[cluster].1[0].max(pos[0]);
                bounds[cluster].1[1] = bounds[cluster].1[1].max(pos[1]);
            }
        }
    }
    for (index, cluster) in graph.clusters.iter().enumerate() {
        if !cluster.members.is_empty() {
            let n = cluster.members.len() as f64;
            centroids[index] = [(sums[index][0] / n) as f32, (sums[index][1] / n) as f32];
        }
    }

    let mut slot_of: HashMap<usize, u32> = HashMap::with_capacity(graph.nodes.len());
    for (slot, node) in graph.nodes.iter().enumerate() {
        slot_of.insert(node.capsule, slot as u32);
    }
    let mut adjacency: Vec<Vec<(u32, usize)>> = vec![Vec::new(); graph.nodes.len()];
    for (edge_idx, edge) in graph.edges.iter().enumerate() {
        if let (Some(&src_slot), Some(&tgt_slot)) =
            (slot_of.get(&edge.source), slot_of.get(&edge.target))
        {
            adjacency[src_slot as usize].push((tgt_slot, edge_idx));
            adjacency[tgt_slot as usize].push((src_slot, edge_idx));
        }
    }

    GraphMetrics {
        grid: NodeGrid::build(&positions, ideal * 3.0),
        centroids,
        bounds,
        adjacency,
    }
}

/// Folder clusters over the allowed nodes. Nodes of a flat vault all land in
/// the `""` root cluster — the view then falls back to communities for color.
pub(crate) fn assign_clusters(capsules: &[Capsule], graph: &mut Graph) {
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
pub(crate) fn aggregate_cluster_edges(graph: &Graph) -> Vec<(u32, u32, u32)> {
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
pub(crate) fn label_propagation(n: usize, edges: &[GraphEdge]) -> Vec<u32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_query_and_nearest_are_exact() {
        let positions: Vec<[f32; 2]> = (0..100)
            .map(|i| [(i % 10) as f32 * 0.1, (i / 10) as f32 * 0.1])
            .collect();
        let grid = NodeGrid::build(&positions, 0.1);
        let inside = grid.query([0.15, 0.15], [0.35, 0.35]);
        assert_eq!(inside.len(), 9);
        for &slot in &inside {
            let p = positions[slot as usize];
            assert!(p[0] >= 0.1 && p[0] <= 0.5 && p[1] >= 0.1 && p[1] <= 0.5);
        }
        let nearest = grid.nearest(&positions, [0.12, 0.11], 0.05);
        assert_eq!(nearest, Some(11));
    }

    #[test]
    fn adjacency_map_matches_edges() {
        use super::super::{EdgeKind, Graph, GraphCluster, GraphEdge, GraphNode};
        let nodes = vec![
            GraphNode {
                capsule: 10,
                degree: 1,
                pos: [0.0, 0.0],
            },
            GraphNode {
                capsule: 20,
                degree: 2,
                pos: [0.5, 0.5],
            },
            GraphNode {
                capsule: 30,
                degree: 1,
                pos: [1.0, 1.0],
            },
        ];
        let edges = vec![
            GraphEdge {
                source: 10,
                target: 20,
                kind: EdgeKind::PageParent,
                label: None,
            },
            GraphEdge {
                source: 20,
                target: 30,
                kind: EdgeKind::Reply,
                label: None,
            },
        ];
        let graph = Graph {
            nodes,
            edges,
            clusters: vec![GraphCluster {
                key: "".into(),
                members: vec![0, 1, 2],
            }],
            cluster_of: vec![0, 0, 0],
            ..Graph::default()
        };
        let metrics = build_metrics(&graph);
        assert_eq!(metrics.adjacency.len(), 3);
        // Slot 0 (capsule 10) connects to slot 1 (capsule 20) via edge 0
        assert_eq!(metrics.adjacency[0], vec![(1, 0)]);
        // Slot 1 (capsule 20) connects to slot 0 (edge 0) and slot 2 (edge 1)
        assert_eq!(metrics.adjacency[1], vec![(0, 0), (2, 1)]);
        // Slot 2 (capsule 30) connects to slot 1 (edge 1)
        assert_eq!(metrics.adjacency[2], vec![(1, 1)]);
    }

    #[test]
    fn neighborhood_bfs_respects_depth() {
        let metrics = GraphMetrics {
            adjacency: vec![
                vec![(1, 0)],
                vec![(0, 0), (2, 1)],
                vec![(1, 1), (3, 2)],
                vec![(2, 2)],
            ],
            ..GraphMetrics::default()
        };
        // Depth 0: only start node
        assert_eq!(metrics.neighborhood_bfs(0, 0), [0].into_iter().collect());
        // Depth 1: node 0 and its neighbor 1
        assert_eq!(metrics.neighborhood_bfs(0, 1), [0, 1].into_iter().collect());
        // Depth 2: nodes 0, 1, 2
        assert_eq!(
            metrics.neighborhood_bfs(0, 2),
            [0, 1, 2].into_iter().collect()
        );
        // Depth 3: nodes 0, 1, 2, 3
        assert_eq!(
            metrics.neighborhood_bfs(0, 3),
            [0, 1, 2, 3].into_iter().collect()
        );
    }
}
