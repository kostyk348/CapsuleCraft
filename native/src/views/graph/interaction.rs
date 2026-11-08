//! Graph interaction: coordinate transformations, focus, pan, and zoom.

use egui::{pos2, vec2, Pos2, Rect, Response, Ui, Vec2};

use crate::graph::GraphMetrics;

use super::GraphState;

#[derive(Clone, Copy, Debug)]
pub struct GraphTransform {
    pub centre: Pos2,
    pub radius: f32,
}

impl GraphTransform {
    pub fn new(rect: Rect, pan: Vec2, zoom: f32) -> Self {
        let centre = rect.center() + pan;
        let radius = (rect.width().min(rect.height()) * 0.40) * zoom;
        Self { centre, radius }
    }

    pub fn to_screen(&self, pos: [f32; 2]) -> Pos2 {
        pos2(
            self.centre.x + pos[0] * self.radius,
            self.centre.y + pos[1] * self.radius,
        )
    }

    pub fn to_graph(&self, p: Pos2) -> [f32; 2] {
        [
            (p.x - self.centre.x) / self.radius,
            (p.y - self.centre.y) / self.radius,
        ]
    }

    pub fn world_bounds(&self, rect: Rect) -> ([f32; 2], [f32; 2]) {
        let min = self.to_graph(rect.min);
        let max = self.to_graph(rect.max);
        (
            [min[0].min(max[0]), min[1].min(max[1])],
            [min[0].max(max[0]), min[1].max(max[1])],
        )
    }
}

/// Centre the view on a cluster and zoom so its bounding box fills ~70% of
/// the shorter canvas side.
pub fn focus_cluster(state: &mut GraphState, rect: Rect, metrics: &GraphMetrics, index: u32) {
    let centre = metrics.centroids[index as usize];
    let (min, max) = metrics.bounds[index as usize];
    let span = (max[0] - min[0]).max(max[1] - min[1]).max(0.05);
    state.focus = Some(index);
    state.zoom = (1.75 / span).clamp(0.35, 3.0);
    let radius = (rect.width().min(rect.height()) * 0.40) * state.zoom;
    // to_screen(centre) must land on rect.center(): centre+pan + c*radius.
    state.pan = vec2(-centre[0] * radius, -centre[1] * radius);
}

/// Handle pan (drag) and zoom (scroll) on the graph canvas.
pub fn handle_pan_and_zoom(ui: &Ui, response: &Response, state: &mut GraphState) {
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
}
