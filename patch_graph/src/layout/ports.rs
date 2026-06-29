//! Port geometry matching the editor (`style.rs` at zoom = 1).

const PORT_EDGE_INSET: f32 = 6.0;

pub fn dual_inlet_node_width(min_width: f32, inlet0_world_x: f32, inlet1_world_x: f32) -> f32 {
    let span = (inlet1_world_x - inlet0_world_x).abs();
    (span + 2.0 * PORT_EDGE_INSET).max(min_width)
}

/// Top-left X for a dual-inlet node whose inlet 0 sits at `inlet0_world_x`.
pub fn dual_inlet_node_x(width: f32, inlet0_world_x: f32, inlet_count: usize) -> f32 {
    inlet0_world_x - port_x_offset(width, 0, inlet_count.max(2))
}

pub fn port_t(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        index as f32 / (count as f32 - 1.0)
    }
}

/// Horizontal offset from the node's top-left to the port center on its edge.
pub fn port_x_offset(width: f32, index: usize, count: usize) -> f32 {
    let span = (width - 2.0 * PORT_EDGE_INSET).max(0.0);
    PORT_EDGE_INSET + port_t(index, count) * span
}

pub fn outlet_world_x(node_x: f32, width: f32, port: usize, outlet_count: usize) -> f32 {
    node_x + port_x_offset(width, port, outlet_count.max(1))
}

pub fn inlet_world_x(node_x: f32, width: f32, port: usize, inlet_count: usize) -> f32 {
    node_x + port_x_offset(width, port, inlet_count.max(1))
}
