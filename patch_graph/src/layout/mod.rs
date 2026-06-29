//! Automatic spatial layout for directed acyclic patch graphs.

mod blocks;
mod config;
mod layout_graph;
mod layered;
mod nav;
mod ports;
mod sugiyama;

pub use blocks::{build_unit_layout, is_passthrough, FanInPair, LayoutUnit, UnitLayout};
pub use config::{FlowDirection, LayoutConfig};
pub use layout_graph::{
    LayoutEdge, LayoutGraph, LayoutNavCell, LayoutNode, LayoutResult, NodeKind, Point,
};
pub use layered::LayeredDagLayout;
pub use ports::{
    dual_inlet_node_width, dual_inlet_node_x, inlet_world_x, outlet_world_x, port_x_offset,
};
pub use sugiyama::SugiyamaLayout;

/// Layout engine trait — additional algorithms implement this.
pub trait LayoutEngine {
    fn layout(&self, graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult;
}

impl LayoutEngine for LayeredDagLayout {
    fn layout(&self, graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
        layered::layout(graph, config)
    }
}

impl LayoutEngine for SugiyamaLayout {
    fn layout(&self, graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
        sugiyama::layout(graph, config)
    }
}

/// Convenience entry point using the Sugiyama layered layout engine.
pub fn layout_patch(graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
    let mut result = SugiyamaLayout.layout(graph, config);
    let (nav, rows) = nav::build_nav_grid(&result.positions);
    result.nav = nav;
    result.rows = rows;
    result
}

/// Apply computed positions and resized dimensions back into node records.
pub fn apply_positions(graph: &mut LayoutGraph, result: &LayoutResult) {
    for (id, point) in &result.positions {
        if let Some(node) = graph.node_mut(*id) {
            node.pos = *point;
        }
    }
    for (id, size) in &result.sizes {
        if let Some(node) = graph.node_mut(*id) {
            node.size = *size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layout_graph::LayoutNode;

    fn demo_graph() -> LayoutGraph {
        let mut g = LayoutGraph::new();
        let in0 = g.add_node(LayoutNode::new(0, (48.0, 20.0), NodeKind::Source, 0, 1));
        let param = g.add_node(LayoutNode::new(1, (56.0, 20.0), NodeKind::Param, 1, 1));
        let mul = g.add_node(LayoutNode::new(2, (40.0, 20.0), NodeKind::Default, 2, 1));
        let out0 = g.add_node(LayoutNode::new(3, (48.0, 20.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(in0, 0, param, 0));
        g.add_edge(LayoutEdge::new(param, 0, mul, 0));
        g.add_edge(LayoutEdge::new(mul, 0, out0, 0));
        g
    }

    #[test]
    fn sources_above_sinks_in_flow() {
        let graph = demo_graph();
        let result = layout_patch(&graph, &LayoutConfig::default());
        let y_in = result.positions[&0].y;
        let y_out = result.positions[&3].y;
        assert!(
            y_in <= y_out,
            "source ({y_in}) should be above or level with sink ({y_out}) in the vertical stack"
        );
    }

    #[test]
    fn layout_is_deterministic() {
        let graph = demo_graph();
        let a = layout_patch(&graph, &LayoutConfig::default());
        let b = layout_patch(&graph, &LayoutConfig::default());
        assert_eq!(a.positions, b.positions);
    }
}
