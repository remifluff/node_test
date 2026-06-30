use egui::{pos2, FontFamily, FontId, Pos2, Rect, Vec2};

use patch_graph::PatchGraph;

pub const FONT_PT: f32 = 10.0;
pub const BOX_H: f32 = 22.0;
pub const PORT_R: f32 = 3.5;
pub const PORT_SIZE_FACTOR: f32 = 1.7 * 1.2 * 1.5;
pub const GRID_STEP: f32 = 15.0;
pub const LABEL_INSET_X: f32 = 3.0;
pub const END_CAP_W: f32 = 3.0;
pub const PORT_EDGE_INSET: f32 = END_CAP_W + 3.0;
pub const CHAR_W: f32 = 6.0;
pub const LINE_W: f32 = 1.0;
pub const CABLE_STROKE: f32 = 1.15;
pub const PORT_GRAB: f32 = 2.5;
pub const PORT_SNAP_GRAB: f32 = 6.0;
pub const CABLE_GRAB: f32 = 4.0;
pub const EDGE_HIT_W: f32 = 6.0;
pub const BODY_HIT_PAD: f32 = 2.0;
pub const MERGE_X: f32 = 8.0;
pub const MERGE_Y: f32 = 79.0;

pub fn font_id() -> FontId {
    FontId::new(FONT_PT, FontFamily::Monospace)
}

pub fn label_font(zoom: f32) -> FontId {
    FontId::new((FONT_PT * zoom).max(7.0), FontFamily::Monospace)
}

pub fn port_size(zoom: f32) -> f32 {
    (PORT_R * PORT_SIZE_FACTOR * zoom).max(2.0)
}

pub fn port_hit_radius() -> f32 {
    PORT_R + PORT_GRAB
}

pub fn port_snap_radius() -> f32 {
    PORT_R + PORT_SNAP_GRAB
}

pub fn port_edge_inset(zoom: f32) -> f32 {
    PORT_EDGE_INSET * zoom
}

pub fn port_span(node_rect: Rect, zoom: f32) -> f32 {
    (node_rect.width() - 2.0 * port_edge_inset(zoom)).max(0.0)
}

pub fn port_t_from_x(x: f32, node_rect: Rect, zoom: f32) -> f32 {
    let inset = port_edge_inset(zoom);
    let span = port_span(node_rect, zoom);
    if span <= 0.0 {
        0.0
    } else {
        ((x - node_rect.left() - inset) / span).clamp(0.0, 1.0)
    }
}

pub fn port_position_t(node_rect: Rect, t: f32, is_outlet: bool, zoom: f32) -> Pos2 {
    let inset = port_edge_inset(zoom);
    let span = port_span(node_rect, zoom);
    let x = node_rect.left() + inset + t.clamp(0.0, 1.0) * span;
    let y = if is_outlet {
        node_rect.bottom()
    } else {
        node_rect.top()
    };
    pos2(x, y)
}

pub fn port_position(node_rect: Rect, index: usize, count: usize, is_outlet: bool, zoom: f32) -> Pos2 {
    let t = if count <= 1 { 0.0 } else { index as f32 / (count as f32 - 1.0) };
    port_position_t(node_rect, t, is_outlet, zoom)
}

pub fn min_box_width(name: &str, inlets: usize) -> f32 {
    let label = strip_brackets(if name.is_empty() { "?" } else { name });
    let text_w = label.len() as f32 * CHAR_W;
    let label_w = END_CAP_W + LABEL_INSET_X + text_w + CHAR_W + END_CAP_W;
    let inlet_w = inlets.max(1) as f32 * GRID_STEP;
    label_w.max(inlet_w)
}

fn strip_brackets(name: &str) -> &str {
    name.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(name)
}

pub fn box_height(h: f32) -> f32 {
    h.max(BOX_H)
}

pub fn cable_control_points(from: Pos2, to: Pos2) -> [Pos2; 4] {
    if (from.x - to.x).abs() < 0.5 {
        return [from, from, to, to];
    }
    let sag = ((to - from).length() * 0.35).clamp(10.0, 72.0);
    [from, from + Vec2::new(0.0, sag), to + Vec2::new(0.0, -sag), to]
}

pub fn cable_hit_half_world(zoom: f32) -> f32 {
    (CABLE_GRAB / zoom).max(1.0)
}

pub fn body_hit_rect(node_rect: Rect) -> Rect {
    node_rect.expand(BODY_HIT_PAD)
}

/// Grid-based helpers for node/port geometry.
#[derive(Clone, Debug)]
pub struct Grid {
    node_count: usize,
    inlet_counts: Vec<usize>,
    outlet_counts: Vec<usize>,
    positions: Vec<Pos2>,
    sizes: Vec<Vec2>,
}

impl Grid {
    pub fn build(graph: &PatchGraph) -> Self {
        let node_count = graph.node_count();
        let mut inlet_counts = Vec::with_capacity(node_count);
        let mut outlet_counts = Vec::with_capacity(node_count);
        let mut positions = Vec::with_capacity(node_count);
        let mut sizes = Vec::with_capacity(node_count);
        for node_id in graph.node_indices() {
            let node = &graph[node_id];
            inlet_counts.push(node.object.inlets());
            outlet_counts.push(node.object.outlets());
            positions.push(node.pos);
            sizes.push(node.size);
        }
        Self { node_count, inlet_counts, outlet_counts, positions, sizes }
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn inlet_count(&self, i: usize) -> usize {
        self.inlet_counts.get(i).copied().unwrap_or(0)
    }

    pub fn outlet_count(&self, i: usize) -> usize {
        self.outlet_counts.get(i).copied().unwrap_or(0)
    }

    fn node_rect(&self, i: usize) -> Option<Rect> {
        let pos = self.positions.get(i)?;
        let size = self.sizes.get(i)?;
        Some(Rect::from_min_size(*pos, *size))
    }

    pub fn inlet_port_pos(&self, node: usize, inlet: usize) -> Option<Pos2> {
        let rect = self.node_rect(node)?;
        let count = self.inlet_count(node);
        Some(port_position(rect, inlet, count, false, 1.0))
    }

    pub fn outlet_port_pos(&self, node: usize, outlet: usize) -> Option<Pos2> {
        let rect = self.node_rect(node)?;
        let count = self.outlet_count(node);
        Some(port_position(rect, outlet, count, true, 1.0))
    }
}
