//! Converts between the editor's `PatchGraph` and `patch_layout::LayoutGraph`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use petgraph::graph::NodeIndex;
use eframe::egui::Vec2;
use patch_layout::{
    LayoutConfig, LayoutEdge, LayoutGraph, LayoutNode, LayoutResult, NodeKind, Point,
    layout_patch,
};

use crate::graph::{EdgeData, Node, PatchGraph, PdObject};

#[derive(Clone, Default)]
pub struct LayoutPreview {
    pub positions: HashMap<usize, eframe::egui::Pos2>,
    pub sizes: HashMap<usize, Vec2>,
}

pub fn patch_to_layout(graph: &PatchGraph) -> LayoutGraph {
    let mut layout = LayoutGraph::new();

    for node_id in graph.node_indices() {
        let node = &graph[node_id];
        layout.add_node(to_layout_node(node_id, node));
    }

    for edge_id in graph.edge_indices() {
        let Some((from, to)) = graph.edge_endpoints(edge_id) else {
            continue;
        };
        let edge = &graph[edge_id];
        layout.add_edge(to_layout_edge(from, to, edge));
    }

    layout
}

/// Hash of patch topology and node sizes — layout ignores live positions.
pub fn layout_topology_fingerprint(graph: &PatchGraph) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut node_ids: Vec<_> = graph.node_indices().collect();
    node_ids.sort_by_key(|id| id.index());

    for node_id in node_ids {
        node_id.index().hash(&mut hasher);
        let node = &graph[node_id];
        node.size.x.to_bits().hash(&mut hasher);
        node.size.y.to_bits().hash(&mut hasher);
        pd_object_kind(&node.object).hash(&mut hasher);
        node.object.inlets().hash(&mut hasher);
        node.object.outlets().hash(&mut hasher);
        delay_pair_hex(&node.object).hash(&mut hasher);
    }

    let mut edge_keys: Vec<(usize, usize, usize, usize)> = Vec::new();
    for edge_id in graph.edge_indices() {
        let Some((from, to)) = graph.edge_endpoints(edge_id) else {
            continue;
        };
        let edge = &graph[edge_id];
        edge_keys.push((
            from.index(),
            to.index(),
            edge.from_port,
            edge.to_port,
        ));
    }
    edge_keys.sort_unstable();
    for key in edge_keys {
        key.hash(&mut hasher);
    }

    hasher.finish()
}

fn layout_result_to_preview(result: LayoutResult) -> LayoutPreview {
    LayoutPreview {
        positions: result
            .positions
            .into_iter()
            .map(|(id, point)| (id, eframe::egui::pos2(point.x, point.y)))
            .collect(),
        sizes: result
            .sizes
            .into_iter()
            .map(|(id, (w, h))| (id, eframe::egui::vec2(w, h)))
            .collect(),
    }
}

pub fn layout_preview(graph: &PatchGraph) -> LayoutPreview {
    let layout = patch_to_layout(graph);
    let result = layout_patch(&layout, &LayoutConfig::default());
    layout_result_to_preview(result)
}

pub fn layout_preview_positions(graph: &PatchGraph) -> HashMap<usize, eframe::egui::Pos2> {
    layout_preview(graph).positions
}

/// Returns cached organized layout; recomputes only when topology changes.
pub fn layout_preview_cached<'a>(
    graph: &PatchGraph,
    cache: &'a mut Option<(u64, LayoutPreview)>,
) -> &'a LayoutPreview {
    let fingerprint = layout_topology_fingerprint(graph);
    if cache.as_ref().is_none_or(|(fp, _)| *fp != fingerprint) {
        *cache = Some((fingerprint, layout_preview(graph)));
    }
    &cache.as_ref().expect("layout cache populated").1
}

pub fn apply_layout_to_patch(graph: &mut PatchGraph, result: &LayoutResult) {
    for (id, point) in &result.positions {
        let node_id = NodeIndex::new(*id);
        if let Some(node) = graph.node_weight_mut(node_id) {
            node.pos = eframe::egui::pos2(point.x, point.y);
        }
    }
    for (id, (w, h)) in &result.sizes {
        let node_id = NodeIndex::new(*id);
        if let Some(node) = graph.node_weight_mut(node_id) {
            node.size = eframe::egui::vec2(*w, *h);
        }
    }
}

pub fn organize_patch(graph: &mut PatchGraph, config: &LayoutConfig) {
    let layout = patch_to_layout(graph);
    let result = layout_patch(&layout, config);
    apply_layout_to_patch(graph, &result);
}

fn to_layout_node(node_id: NodeIndex, node: &Node) -> LayoutNode {
    let mut layout_node = LayoutNode::new(
        node_id.index(),
        (node.size.x, node.size.y),
        pd_object_kind(&node.object),
        node.object.inlets(),
        node.object.outlets(),
    );
    layout_node.pos = Point {
        x: node.pos.x,
        y: node.pos.y,
    };

    if let Some(hex) = delay_pair_hex(&node.object) {
        layout_node = layout_node.with_delay_pair(hex);
    }

    layout_node
}

fn to_layout_edge(from: NodeIndex, to: NodeIndex, edge: &EdgeData) -> LayoutEdge {
    LayoutEdge::new(from.index(), edge.from_port, to.index(), edge.to_port)
}

fn pd_object_kind(object: &PdObject) -> NodeKind {
    match object {
        PdObject::In => NodeKind::Source,
        PdObject::Out => NodeKind::Sink,
        PdObject::Param => NodeKind::Param,
        PdObject::Combine => NodeKind::Combine,
        PdObject::Send { .. } => NodeKind::Send,
        PdObject::Receive { .. } => NodeKind::Receive,
        PdObject::DelayIn { .. } => NodeKind::DelayIn,
        PdObject::DelayOut { .. } => NodeKind::DelayOut,
        PdObject::Comment { .. } => NodeKind::Comment,
        _ => NodeKind::Default,
    }
}

fn delay_pair_hex(object: &PdObject) -> Option<u8> {
    match object {
        PdObject::DelayIn { id: Some(hex) }
        | PdObject::DelayOut { id: Some(hex) }
        | PdObject::Send { id: Some(hex) }
        | PdObject::Receive { id: Some(hex) } => Some(*hex),
        _ => None,
    }
}
