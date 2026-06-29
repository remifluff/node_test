//! Layered layout via [rust-sugiyama](https://crates.io/crates/rust-sugiyama), with a
//! port-alignment post-pass for straight vertical patch cables.

use std::collections::{HashMap, HashSet};

use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use rust_sugiyama::configure::{Config as SugiyamaConfig, CrossingMinimization, RankingType};
use rust_sugiyama::from_vertices_and_edges;

use crate::config::{FlowDirection, LayoutConfig};
use crate::graph::{LayoutGraph, LayoutNodeId, LayoutResult, Point};
use crate::ports::{outlet_world_x, port_x_offset};

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

            normalize_component_origin(&mut local);
            align_ports_in_component(graph, &mut local);

            let (min_x, min_y, max_x, _max_y) = component_bounds(&local, graph);
            let dx = component_offset.x - min_x;
            let dy = component_offset.y - min_y;
            for (id, point) in local {
                positions.insert(
                    id,
                    Point {
                        x: point.x + dx,
                        y: config.snap(point.y + dy),
                    },
                );
            }

            component_offset.x = config.snap(max_x + dx + config.column_gap());
            component_offset.y = config.origin.y;
        }

        LayoutResult {
            positions,
            sizes: HashMap::new(),
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

fn component_bounds(
    positions: &HashMap<LayoutNodeId, Point>,
    graph: &LayoutGraph,
) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (&id, pos) in positions {
        let node = graph.node(id).expect("node");
        min_x = min_x.min(pos.x);
        min_y = min_y.min(pos.y);
        max_x = max_x.max(pos.x + node.size.0);
        max_y = max_y.max(pos.y + node.size.1);
    }

    if min_x.is_infinite() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (min_x, min_y, max_x, max_y)
    }
}

fn align_ports_in_component(graph: &LayoutGraph, positions: &mut HashMap<LayoutNodeId, Point>) {
    let incoming = build_incoming(graph);
    let order = topo_sort_nodes(graph);

    for id in order {
        let Some(edges_in) = incoming.get(&id) else {
            continue;
        };
        if edges_in.len() != 1 {
            continue;
        }
        let edge = edges_in[0];
        let parent = edge.from;
        let Some(parent_pos) = positions.get(&parent).copied() else {
            continue;
        };
        let Some(parent_node) = graph.node(parent) else {
            continue;
        };
        let Some(child_node) = graph.node(id) else {
            continue;
        };
        let parent_w = parent_node.size.0;
        let child_w = child_node.size.0;
        let out_x = outlet_world_x(
            parent_pos.x,
            parent_w,
            edge.from_port,
            parent_node.outlets.max(1),
        );
        let child_x = out_x - port_x_offset(child_w, edge.to_port, child_node.inlets.max(1));
        if let Some(child_pos) = positions.get_mut(&id) {
            child_pos.x = child_x;
        }
    }
}

fn build_incoming(graph: &LayoutGraph) -> HashMap<LayoutNodeId, Vec<&crate::graph::LayoutEdge>> {
    let mut incoming: HashMap<LayoutNodeId, Vec<&crate::graph::LayoutEdge>> = HashMap::new();
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
    use crate::graph::{LayoutEdge, LayoutGraph, LayoutNode, NodeKind};
    use crate::ports::{inlet_world_x, outlet_world_x};

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
}
