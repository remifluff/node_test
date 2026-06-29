//! Layered layout via [rust-sugiyama](https://crates.io/crates/rust-sugiyama), with a
//! port-alignment post-pass for straight vertical patch cables.

use std::collections::{HashMap, HashSet};

use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use rust_sugiyama::configure::{Config as SugiyamaConfig, CrossingMinimization, RankingType};
use rust_sugiyama::from_vertices_and_edges;

use crate::layout::blocks::is_dual_inlet_combiner;
use crate::layout::config::{FlowDirection, LayoutConfig};
use crate::layout::layout_graph::{LayoutEdge, LayoutGraph, LayoutNodeId, LayoutResult, Point};
use crate::layout::ports::{dual_inlet_node_width, dual_inlet_node_x, outlet_world_x, port_x_offset};

#[derive(Clone, Copy, Debug, Default)]
pub struct SugiyamaLayout;

pub fn layout(graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
    SugiyamaLayout.layout_impl(graph, config)
}

impl SugiyamaLayout {
    fn layout_impl(&self, graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
        let node_ids = graph.sorted_node_ids();
        if node_ids.is_empty() {
            return LayoutResult::default();
        }

        let vertices: Vec<(u32, (f64, f64))> = node_ids
            .iter()
            .map(|&id| {
                let node = graph.node(id).expect("node");
                (id as u32, (node.size.0 as f64, node.size.1 as f64))
            })
            .collect();

        let edges = unique_edges(graph);
        let sugiyama_config = to_sugiyama_config(config);
        let components = from_vertices_and_edges(&vertices, &edges, &sugiyama_config);

        let mut positions = HashMap::new();
        let mut sizes = HashMap::new();
        let mut component_offset = config.origin;

        for (component, _width, _height) in components {
            if component.is_empty() {
                continue;
            }

            let mut local: HashMap<LayoutNodeId, Point> = HashMap::new();
            for (id, (cx, cy)) in &component {
                let node = graph.node(*id).expect("node");
                local.insert(
                    *id,
                    center_to_top_left(*cx, *cy, node.size.0, node.size.1, config.flow),
                );
            }

            let mut local_sizes = HashMap::new();
            normalize_component_origin(&mut local);
            snap_root_columns(graph, config, &mut local);
            align_ports_in_component(graph, &mut local, &mut local_sizes);
            assign_grid_rows(graph, config, &mut local);

            let (min_x, min_y, max_x, _max_y) = component_bounds(&local, graph, &local_sizes);
            let dx = component_offset.x - min_x;
            let dy = component_offset.y - min_y;
            for (id, point) in local {
                positions.insert(
                    id,
                    Point {
                        x: point.x + dx,
                        y: point.y + dy,
                    },
                );
            }
            for (id, size) in local_sizes {
                sizes.insert(id, size);
            }

            component_offset.x = config.snap(max_x + dx + config.column_gap());
            component_offset.y = config.origin.y;
        }

        LayoutResult {
            positions,
            sizes,
            ..Default::default()
        }
    }
}

fn unique_edges(graph: &LayoutGraph) -> Vec<(u32, u32)> {
    let mut seen = HashSet::new();
    let mut edges = Vec::new();
    for edge in graph.edges() {
        let key = (edge.from, edge.to);
        if seen.insert(key) {
            edges.push((edge.from as u32, edge.to as u32));
        }
    }
    edges
}

fn to_sugiyama_config(config: &LayoutConfig) -> SugiyamaConfig {
    SugiyamaConfig {
        minimum_length: 1,
        vertex_spacing: config.row_gap().max(config.node_gap) as f64,
        dummy_vertices: false,
        dummy_size: 1.0,
        ranking_type: RankingType::MinimizeEdgeLength,
        c_minimization: CrossingMinimization::Barycenter,
        transpose: true,
    }
}

/// rust-sugiyama returns node centres; the editor uses top-left corners.
fn center_to_top_left(cx: f64, cy: f64, w: f32, h: f32, flow: FlowDirection) -> Point {
    let (x, y) = match flow {
        FlowDirection::TopToBottom => (cx - w as f64 * 0.5, cy - h as f64 * 0.5),
        FlowDirection::LeftToRight => (cy - w as f64 * 0.5, cx - h as f64 * 0.5),
    };
    Point {
        x: x as f32,
        y: y as f32,
    }
}

fn normalize_component_origin(positions: &mut HashMap<LayoutNodeId, Point>) {
    let min_x = positions
        .values()
        .map(|p| p.x)
        .fold(f32::INFINITY, f32::min);
    let min_y = positions
        .values()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);
    for point in positions.values_mut() {
        point.x -= min_x;
        point.y -= min_y;
    }
}

fn node_width(
    graph: &LayoutGraph,
    id: LayoutNodeId,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) -> f32 {
    sizes
        .get(&id)
        .map(|(w, _)| *w)
        .unwrap_or_else(|| graph.node(id).expect("node").size.0)
}

fn component_bounds(
    positions: &HashMap<LayoutNodeId, Point>,
    graph: &LayoutGraph,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (&id, pos) in positions {
        let node = graph.node(id).expect("node");
        let w = node_width(graph, id, sizes);
        min_x = min_x.min(pos.x);
        min_y = min_y.min(pos.y);
        max_x = max_x.max(pos.x + w);
        max_y = max_y.max(pos.y + node.size.1);
    }

    if min_x.is_infinite() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (min_x, min_y, max_x, max_y)
    }
}

/// Snap spine roots to grid columns; child X is set later by port alignment.
fn snap_root_columns(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    positions: &mut HashMap<LayoutNodeId, Point>,
) {
    let incoming = build_incoming(graph);
    for (&id, pos) in positions.iter_mut() {
        let is_root = incoming.get(&id).is_none_or(|edges| edges.is_empty());
        if is_root {
            pos.x = config.snap(pos.x);
        }
    }
}

/// Map sugiyama layers to grid-snapped row Y with consistent vertical spacing.
fn assign_grid_rows(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    positions: &mut HashMap<LayoutNodeId, Point>,
) {
    let layers = extract_layers(positions);
    if layers.is_empty() {
        return;
    }

    let mut row_y = Vec::with_capacity(layers.len());
    let mut y = 0.0;
    for (layer_idx, _) in layers.iter().enumerate() {
        row_y.push(config.snap(y));
        let max_h = positions
            .iter()
            .filter(|(_, pos)| layer_index(pos.y, &layers) == layer_idx)
            .map(|(id, _)| graph.node(*id).expect("node").size.1)
            .fold(0.0f32, f32::max);
        y += max_h + config.row_gap();
    }

    for pos in positions.values_mut() {
        let idx = layer_index(pos.y, &layers);
        pos.y = row_y[idx];
    }
}

fn extract_layers(positions: &HashMap<LayoutNodeId, Point>) -> Vec<f32> {
    let mut ys: Vec<f32> = positions.values().map(|p| p.y).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut layers: Vec<f32> = Vec::new();
    for y in ys {
        let is_new_layer = match layers.last() {
            None => true,
            Some(&last) => (y - last).abs() > 0.5,
        };
        if is_new_layer {
            layers.push(y);
        }
    }
    layers
}

fn layer_index(y: f32, layers: &[f32]) -> usize {
    layers
        .iter()
        .position(|&layer_y| (y - layer_y).abs() <= 0.5)
        .unwrap_or(layers.len().saturating_sub(1))
}

fn leftmost_parent_edge<'a>(
    edges_in: &[&'a LayoutEdge],
    positions: &HashMap<LayoutNodeId, Point>,
) -> &'a LayoutEdge {
    edges_in
        .iter()
        .copied()
        .min_by(|a, b| {
            let ax = positions
                .get(&a.from)
                .map(|p| p.x)
                .unwrap_or(f32::INFINITY);
            let bx = positions
                .get(&b.from)
                .map(|p| p.x)
                .unwrap_or(f32::INFINITY);
            ax.partial_cmp(&bx)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("non-empty parent list")
}

fn align_child_to_parent_edge(
    graph: &LayoutGraph,
    child: LayoutNodeId,
    edge: &LayoutEdge,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) {
    let parent = edge.from;
    let Some(parent_pos) = positions.get(&parent).copied() else {
        return;
    };
    let Some(parent_node) = graph.node(parent) else {
        return;
    };
    let Some(child_node) = graph.node(child) else {
        return;
    };
    let parent_w = node_width(graph, parent, sizes);
    let child_w = node_width(graph, child, sizes);
    let out_x = outlet_world_x(
        parent_pos.x,
        parent_w,
        edge.from_port,
        parent_node.outlets.max(1),
    );
    let child_x = out_x - port_x_offset(child_w, edge.to_port, child_node.inlets.max(1));
    if let Some(child_pos) = positions.get_mut(&child) {
        child_pos.x = child_x;
    }
}

fn align_dual_inlet_combiner(
    graph: &LayoutGraph,
    combine_id: LayoutNodeId,
    edges_in: &[&LayoutEdge],
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
) -> bool {
    let node = graph.node(combine_id).expect("combine");
    let min_w = node.size.0;
    let h = node.size.1;
    let y = positions.get(&combine_id).map(|p| p.y).unwrap_or(0.0);

    let mut targets: Vec<(usize, f32)> = Vec::new();
    for edge in edges_in {
        let parent = edge.from;
        let Some(parent_pos) = positions.get(&parent) else {
            return false;
        };
        let parent_node = graph.node(parent).expect("parent");
        let parent_w = node_width(graph, parent, sizes);
        let out_x = outlet_world_x(
            parent_pos.x,
            parent_w,
            edge.from_port,
            parent_node.outlets.max(1),
        );
        targets.push((edge.to_port, out_x));
    }

    if targets.is_empty() {
        return false;
    }

    targets.sort_by_key(|(port, _)| *port);
    let in0 = targets.iter().find(|(port, _)| *port == 0).map(|(_, x)| *x);
    let in1 = targets.iter().find(|(port, _)| *port == 1).map(|(_, x)| *x);

    let (x, w) = match (in0, in1) {
        (Some(x0), Some(x1)) => {
            let w = dual_inlet_node_width(min_w, x0, x1);
            (dual_inlet_node_x(w, x0, node.inlets), w)
        }
        (Some(x0), None) => {
            let w = min_w;
            (x0 - port_x_offset(w, 0, node.inlets.max(2)), w)
        }
        _ => return false,
    };

    sizes.insert(combine_id, (w, h));
    positions.insert(combine_id, Point { x, y });
    true
}

fn align_ports_in_component(
    graph: &LayoutGraph,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
) {
    let incoming = build_incoming(graph);
    let order = topo_sort_nodes(graph);

    for id in order {
        let Some(edges_in) = incoming.get(&id) else {
            continue;
        };
        if edges_in.is_empty() {
            continue;
        }

        let node = graph.node(id).expect("node");
        if is_dual_inlet_combiner(node) && edges_in.len() >= 2 {
            let parents_ready = edges_in.iter().all(|edge| positions.contains_key(&edge.from));
            if parents_ready {
                align_dual_inlet_combiner(graph, id, edges_in, positions, sizes);
            }
            continue;
        }

        let edge = if edges_in.len() == 1 {
            edges_in[0]
        } else {
            leftmost_parent_edge(edges_in, positions)
        };
        align_child_to_parent_edge(graph, id, edge, positions, sizes);
    }
}

fn build_incoming(graph: &LayoutGraph) -> HashMap<LayoutNodeId, Vec<&crate::layout::layout_graph::LayoutEdge>> {
    let mut incoming: HashMap<LayoutNodeId, Vec<&crate::layout::layout_graph::LayoutEdge>> = HashMap::new();
    for edge in graph.edges() {
        incoming.entry(edge.to).or_default().push(edge);
    }
    incoming
}

fn topo_sort_nodes(graph: &LayoutGraph) -> Vec<LayoutNodeId> {
    let mut pet = DiGraph::new();
    let mut id_to_ix = HashMap::new();
    for id in graph.sorted_node_ids() {
        id_to_ix.insert(id, pet.add_node(id));
    }
    for edge in graph.edges() {
        if let (Some(&from), Some(&to)) = (id_to_ix.get(&edge.from), id_to_ix.get(&edge.to)) {
            pet.add_edge(from, to, ());
        }
    }
    toposort(&pet, None)
        .unwrap_or_else(|_| pet.node_indices().collect())
        .into_iter()
        .map(|ix| pet[ix])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_graph::{LayoutEdge, LayoutGraph, LayoutNode, NodeKind};
    use crate::layout::ports::{inlet_world_x, outlet_world_x};

    #[test]
    fn chain_ports_share_x_after_alignment() {
        let mut g = LayoutGraph::new();
        let a = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let b = g.add_node(LayoutNode::new(1, (56.0, 22.0), NodeKind::Param, 1, 1));
        let c = g.add_node(LayoutNode::new(2, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(a, 0, b, 0));
        g.add_edge(LayoutEdge::new(b, 0, c, 0));

        let config = LayoutConfig {
            flow: FlowDirection::TopToBottom,
            ..LayoutConfig::default()
        };
        let result = layout(&g, &config);

        for edge in g.edges() {
            let from = g.node(edge.from).unwrap();
            let to = g.node(edge.to).unwrap();
            let from_pos = result.positions[&edge.from];
            let to_pos = result.positions[&edge.to];
            let out_x = outlet_world_x(from_pos.x, from.size.0, edge.from_port, from.outlets);
            let in_x = inlet_world_x(to_pos.x, to.size.0, edge.to_port, to.inlets);
            assert!(
                (out_x - in_x).abs() < 0.01,
                "edge {}→{}: outlet x {out_x} should match inlet x {in_x}",
                edge.from,
                edge.to
            );
        }
    }

    #[test]
    fn sources_above_sinks_in_vertical_flow() {
        let mut g = LayoutGraph::new();
        let src = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let sink = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(src, 0, sink, 0));

        let config = LayoutConfig {
            flow: FlowDirection::TopToBottom,
            ..LayoutConfig::default()
        };
        let result = layout(&g, &config);
        assert!(
            result.positions[&0].y <= result.positions[&1].y,
            "source should be above sink"
        );
    }

    #[test]
    fn rows_and_roots_snap_to_grid() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p1 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p2 = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        g.add_edge(LayoutEdge::new(i0, 0, p1, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p2, 0));

        let config = LayoutConfig::default();
        let result = layout(&g, &config);
        let incoming = build_incoming(&g);

        for (&id, pos) in &result.positions {
            assert_eq!(
                pos.y,
                config.snap(pos.y),
                "node {id} Y should sit on the grid"
            );
            if incoming.get(&id).is_none_or(|edges| edges.is_empty()) {
                assert_eq!(
                    pos.x,
                    config.snap(pos.x),
                    "spine root {id} X should sit on the grid"
                );
            }
        }
    }

    #[test]
    fn multi_parent_aligns_under_leftmost() {
        let mut g = LayoutGraph::new();
        let left = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let right = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let child = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 2, 1));
        g.add_edge(LayoutEdge::new(left, 0, child, 0));
        g.add_edge(LayoutEdge::new(right, 0, child, 1));

        let config = LayoutConfig::default();
        let result = layout(&g, &config);

        let left_pos = result.positions[&left];
        let child_pos = result.positions[&child];
        let left_node = g.node(left).unwrap();
        let child_node = g.node(child).unwrap();
        let out_x = outlet_world_x(left_pos.x, left_node.size.0, 0, left_node.outlets);
        let in_x = inlet_world_x(child_pos.x, child_node.size.0, 0, child_node.inlets);
        assert!(
            (out_x - in_x).abs() < 0.01,
            "child should align under leftmost parent outlet"
        );
    }

    #[test]
    fn combine_stretches_between_parent_outlets() {
        let mut g = LayoutGraph::new();
        let left = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let right = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let combine = g.add_node(LayoutNode::new(2, (48.0, 22.0), NodeKind::Combine, 2, 1));
        g.add_edge(LayoutEdge::new(left, 0, combine, 0));
        g.add_edge(LayoutEdge::new(right, 0, combine, 1));

        let result = layout(&g, &LayoutConfig::default());

        for edge in g.edges() {
            let from = g.node(edge.from).unwrap();
            let to = g.node(edge.to).unwrap();
            let from_pos = result.positions[&edge.from];
            let to_pos = result.positions[&edge.to];
            let from_w = result
                .sizes
                .get(&edge.from)
                .map(|(w, _)| *w)
                .unwrap_or(from.size.0);
            let to_w = result
                .sizes
                .get(&edge.to)
                .map(|(w, _)| *w)
                .unwrap_or(to.size.0);
            let out_x = outlet_world_x(from_pos.x, from_w, edge.from_port, from.outlets);
            let in_x = inlet_world_x(to_pos.x, to_w, edge.to_port, to.inlets);
            assert!(
                (out_x - in_x).abs() < 0.01,
                "edge {}→{}: outlet x {out_x} should match inlet x {in_x}",
                edge.from,
                edge.to
            );
        }
    }
}
