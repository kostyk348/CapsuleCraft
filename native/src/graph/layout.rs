//! Graph layout engine: spring relaxation, Barnes–Hut repulsion, and background worker.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use super::metrics::NodeGrid;
use super::{EdgeKind, Graph, SYNC_LAYOUT_MAX};

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

pub(crate) type Spring = (usize, usize, f32);

pub(crate) fn spring_list(graph: &Graph) -> Vec<Spring> {
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
pub(crate) fn relax(
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
    let feasible = (0.78 / n as f32).sqrt();
    separate(&mut positions, ideal.min(feasible) * 0.9, 16);
    positions
}

/// Push apart every pair of nodes closer than `min_dist`. Grid-accelerated
/// (only near pairs are compared) and deterministic: fixed order, and
/// coincident points split along the golden-angle direction of the slot.
pub(crate) fn separate(positions: &mut [[f32; 2]], min_dist: f32, iterations: usize) {
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

    for (slot, _pos) in positions.iter().enumerate() {
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
        let old = tree[node].point as usize;
        tree[node].point = -1;
        descend(tree, node, old, positions, depth);
    }
    tree[node].add_mass(x, y);
    descend(tree, node, slot, positions, depth);
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincident_nodes_get_separated() {
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
}
