//! Relationship graph and its deterministic layout.
//!
//! The web view runs d3-force in the browser. Here the same forces are applied
//! by hand: repulsion between nodes, springs on edges, a weak pull to the
//! centre and a little damping. Positions start from a golden-angle spiral, so
//! the same vault always produces the same picture — no random seed, no
//! frame-rate dependency, and a layout that can be unit-tested.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::mime::Capsule;

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

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub tags: Vec<String>,
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

/// Build the full graph for a vault, optionally restricted to one tag.
pub fn build(capsules: &[Capsule], tag_filter: Option<&str>) -> Graph {
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
            // Golden-angle spiral: even coverage without randomness.
            let angle = slot as f32 * 2.399_963_1;
            let radius = (slot as f32 + 1.0).sqrt() / (count as f32).sqrt();
            GraphNode {
                capsule: *capsule,
                degree: degrees.get(capsule).copied().unwrap_or(0),
                pos: [radius * angle.cos(), radius * angle.sin()],
            }
        })
        .collect();

    let mut graph = Graph {
        nodes,
        edges,
        tags: tags.into_iter().collect(),
    };
    graph.layout(360);
    graph
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

impl Graph {
    /// `iterations` of Fruchterman–Reingold style relaxation.
    pub fn layout(&mut self, iterations: usize) {
        if self.nodes.len() < 2 {
            return;
        }
        let n = self.nodes.len();
        let slot_of: HashMap<usize, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(slot, node)| (node.capsule, slot))
            .collect();
        let springs: Vec<(usize, usize, f32)> = self
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
            .collect();

        let mut positions: Vec<[f32; 2]> = self.nodes.iter().map(|node| node.pos).collect();
        let mut velocities = vec![[0.0f32, 0.0f32]; n];
        let ideal = (1.0 / n as f32).sqrt().clamp(0.06, 0.35);

        for step in 0..iterations {
            let cooling = 1.0 - step as f32 / iterations as f32;
            let mut forces = vec![[0.0f32, 0.0f32]; n];

            // Repulsion (n is a vault page count, not a social graph).
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

            // Springs.
            for (a, b, rest) in &springs {
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
        }

        normalise(&mut positions);
        for (node, pos) in self.nodes.iter_mut().zip(positions) {
            node.pos = pos;
        }
    }
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
}
