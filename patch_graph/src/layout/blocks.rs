use std::collections::{HashMap, HashSet};

use crate::layout::layout_graph::{LayoutEdge, LayoutGraph, LayoutNode, LayoutNodeId};

/// Two passthrough columns feeding a dual-inlet combiner, placed side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanInPair {
    pub left: usize,
    pub right: usize,
    pub combine: usize,
}

/// A layout unit: lone node, vertical column, or dual-inlet combiner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutUnit {
    pub nodes: Vec<LayoutNodeId>,
    /// Vertically stacked pass-through chain (same port X, increasing Y).
    pub column: bool,
    /// Two-inlet combiner: stretched/resized at placement time.
    pub dual_inlet: bool,
}

impl LayoutUnit {
    pub fn singleton(id: LayoutNodeId, dual_inlet: bool) -> Self {
        Self {
            nodes: vec![id],
            column: false,
            dual_inlet,
        }
    }

    pub fn column(nodes: Vec<LayoutNodeId>) -> Self {
        Self {
            nodes,
            column: true,
            dual_inlet: false,
        }
    }

    pub fn primary(&self) -> LayoutNodeId {
        self.nodes[0]
    }

    pub fn head(&self) -> LayoutNodeId {
        self.primary()
    }

    pub fn tail(&self) -> LayoutNodeId {
        *self.nodes.last().expect("unit has at least one node")
    }

    pub fn is_column(&self) -> bool {
        self.column
    }

    /// Back-compat alias used in tests.
    pub fn is_block(&self) -> bool {
        self.column && self.nodes.len() >= 2
    }

    pub fn is_dual_inlet(&self) -> bool {
        self.dual_inlet
    }
}

#[derive(Clone, Debug)]
pub struct UnitLayout {
    pub units: Vec<LayoutUnit>,
    pub node_to_unit: HashMap<LayoutNodeId, usize>,
    pub fanin_pairs: Vec<FanInPair>,
}

impl UnitLayout {
    pub fn unit(&self, id: LayoutNodeId) -> &LayoutUnit {
        &self.units[self.node_to_unit[&id]]
    }

    pub fn unit_index(&self, id: LayoutNodeId) -> usize {
        self.node_to_unit[&id]
    }

    pub fn fanin_pair_for(&self, unit_ix: usize) -> Option<&FanInPair> {
        self.fanin_pairs.iter().find(|pair| {
            pair.left == unit_ix || pair.right == unit_ix || pair.combine == unit_ix
        })
    }

    pub fn fanin_pair_left(&self, unit_ix: usize) -> Option<&FanInPair> {
        self.fanin_pairs.iter().find(|pair| pair.left == unit_ix)
    }
}

pub fn is_dual_inlet_combiner(node: &LayoutNode) -> bool {
    node.inlets >= 2 && node.outlets == 1
}

pub fn is_passthrough(node: &LayoutNode) -> bool {
    node.inlets == 1 && node.outlets == 1
}

fn is_linear_passthrough(
    graph: &LayoutGraph,
    id: LayoutNodeId,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
) -> bool {
    let Some(node) = graph.node(id) else {
        return false;
    };
    if !is_passthrough(node) {
        return false;
    }
    incoming.get(&id).map(|e| e.len()).unwrap_or(0) <= 1
        && outgoing.get(&id).map(|e| e.len()).unwrap_or(0) <= 1
}

/// Maximal chains of connected pass-through nodes (1 in, 1 out, degree ≤ 1).
pub fn find_vertical_columns(
    graph: &LayoutGraph,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
) -> Vec<Vec<LayoutNodeId>> {
    let mut columns = Vec::new();
    let mut in_column = HashSet::new();

    for start in graph.sorted_node_ids() {
        if in_column.contains(&start)
            || !is_linear_passthrough(graph, start, incoming, outgoing)
        {
            continue;
        }

        let mut head = start;
        loop {
            let preds: Vec<LayoutNodeId> = incoming
                .get(&head)
                .into_iter()
                .flat_map(|edges| edges.iter().map(|e| e.from))
                .collect();
            if preds.len() == 1
                && is_linear_passthrough(graph, preds[0], incoming, outgoing)
                && !in_column.contains(&preds[0])
            {
                head = preds[0];
            } else {
                break;
            }
        }

        let mut column = vec![head];
        in_column.insert(head);
        let mut tail = head;
        loop {
            let succs: Vec<LayoutNodeId> = outgoing
                .get(&tail)
                .into_iter()
                .flat_map(|edges| edges.iter().map(|e| e.to))
                .collect();
            if succs.len() == 1 && is_linear_passthrough(graph, succs[0], incoming, outgoing) {
                tail = succs[0];
                if in_column.contains(&tail) {
                    break;
                }
                column.push(tail);
                in_column.insert(tail);
            } else {
                break;
            }
        }

        columns.push(column);
    }

    columns.sort_by_key(|column| column[0]);
    columns
}

/// Pair two upstream columns that feed a dual-inlet combiner (inlet 0 left, inlet 1 right).
pub fn find_combine_fanin_pairs(
    units: &UnitLayout,
    _graph: &LayoutGraph,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
) -> Vec<FanInPair> {
    let mut pairs = Vec::new();

    for (combine_ix, unit) in units.units.iter().enumerate() {
        if !unit.dual_inlet {
            continue;
        }
        let combine_id = unit.primary();
        let Some(edges) = incoming.get(&combine_id) else {
            continue;
        };
        if edges.len() < 2 {
            continue;
        }

        let mut feeders: Vec<(usize, usize)> = edges
            .iter()
            .map(|edge| (units.node_to_unit[&edge.from], edge.to_port))
            .collect();
        feeders.sort_by_key(|(_, port)| *port);
        feeders.dedup_by_key(|(unit_ix, _)| *unit_ix);

        if feeders.len() >= 2 {
            pairs.push(FanInPair {
                left: feeders[0].0,
                right: feeders[1].0,
                combine: combine_ix,
            });
        }
    }

    pairs
}

pub fn build_unit_layout(
    graph: &LayoutGraph,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
) -> UnitLayout {
    let columns = find_vertical_columns(graph, incoming, outgoing);
    let mut node_to_unit = HashMap::new();
    let mut units = Vec::new();

    for column in columns {
        let index = units.len();
        for &id in &column {
            node_to_unit.insert(id, index);
        }
        units.push(LayoutUnit::column(column));
    }

    for id in graph.sorted_node_ids() {
        if node_to_unit.contains_key(&id) {
            continue;
        }
        let dual_inlet = graph
            .node(id)
            .is_some_and(is_dual_inlet_combiner);
        node_to_unit.insert(id, units.len());
        units.push(LayoutUnit::singleton(id, dual_inlet));
    }

    let fanin_pairs = find_combine_fanin_pairs(
        &UnitLayout {
            units: units.clone(),
            node_to_unit: node_to_unit.clone(),
            fanin_pairs: Vec::new(),
        },
        graph,
        incoming,
    );

    UnitLayout {
        units,
        node_to_unit,
        fanin_pairs,
    }
}

pub fn node_layout_width(graph: &LayoutGraph, id: LayoutNodeId) -> f32 {
    graph.node(id).map(|n| n.size.0.max(48.0)).unwrap_or(48.0)
}

pub fn effective_node_width(
    graph: &LayoutGraph,
    id: LayoutNodeId,
    sizes: Option<&HashMap<LayoutNodeId, (f32, f32)>>,
) -> f32 {
    sizes
        .and_then(|s| s.get(&id).map(|(w, _)| *w))
        .unwrap_or_else(|| node_layout_width(graph, id))
        .max(48.0)
}

pub fn effective_node_height(
    graph: &LayoutGraph,
    id: LayoutNodeId,
    sizes: Option<&HashMap<LayoutNodeId, (f32, f32)>>,
) -> f32 {
    sizes
        .and_then(|s| s.get(&id).map(|(_, h)| *h))
        .unwrap_or_else(|| node_layout_height(graph, id))
        .max(22.0)
}

pub fn node_layout_height(graph: &LayoutGraph, id: LayoutNodeId) -> f32 {
    graph.node(id).map(|n| n.size.1.max(22.0)).unwrap_or(22.0)
}

pub fn translate_unit(
    unit: &LayoutUnit,
    dx: f32,
    dy: f32,
    positions: &mut HashMap<LayoutNodeId, crate::layout::layout_graph::Point>,
) {
    if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
        return;
    }
    for &id in &unit.nodes {
        if let Some(pos) = positions.get_mut(&id) {
            pos.x += dx;
            pos.y += dy;
        }
    }
}

pub fn unit_span_width(
    graph: &LayoutGraph,
    unit: &LayoutUnit,
    node_gap: f32,
    sizes: Option<&HashMap<LayoutNodeId, (f32, f32)>>,
) -> f32 {
    if unit.dual_inlet {
        let id = unit.primary();
        if sizes.is_some_and(|s| s.contains_key(&id)) {
            return effective_node_width(graph, id, sizes);
        }
        let w = node_layout_width(graph, id);
        return w + node_gap + w;
    }
    if unit.nodes.is_empty() {
        return 0.0;
    }
    unit.nodes
        .iter()
        .map(|&id| effective_node_width(graph, id, sizes))
        .fold(0.0f32, f32::max)
}

pub fn place_unit(
    graph: &LayoutGraph,
    unit: &LayoutUnit,
    anchor: crate::layout::layout_graph::Point,
    row_gap: f32,
    positions: &mut HashMap<LayoutNodeId, crate::layout::layout_graph::Point>,
) {
    if unit.dual_inlet {
        positions.insert(unit.primary(), anchor);
        return;
    }

    if unit.nodes.len() <= 1 {
        let id = unit.head();
        positions.insert(id, anchor);
        return;
    }

    // Vertical column: nodes stacked top-to-bottom at the same port X.
    let mut y = anchor.y;
    for &id in &unit.nodes {
        positions.insert(id, crate::layout::layout_graph::Point { x: anchor.x, y });
        y += node_layout_height(graph, id) + row_gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_graph::{LayoutGraph, LayoutNode, NodeKind};

    fn incoming(graph: &LayoutGraph) -> HashMap<LayoutNodeId, Vec<&LayoutEdge>> {
        let mut map: HashMap<LayoutNodeId, Vec<&LayoutEdge>> = HashMap::new();
        for edge in graph.edges() {
            map.entry(edge.to).or_default().push(edge);
        }
        map
    }

    fn outgoing(graph: &LayoutGraph) -> HashMap<LayoutNodeId, Vec<&LayoutEdge>> {
        let mut map: HashMap<LayoutNodeId, Vec<&LayoutEdge>> = HashMap::new();
        for edge in graph.edges() {
            map.entry(edge.from).or_default().push(edge);
        }
        map
    }

    #[test]
    fn detects_passthrough_chain() {
        let mut g = LayoutGraph::new();
        let i = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p1 = g.add_node(LayoutNode::new(1, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p2 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let o = g.add_node(LayoutNode::new(3, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i, 0, p1, 0));
        g.add_edge(LayoutEdge::new(p1, 0, p2, 0));
        g.add_edge(LayoutEdge::new(p2, 0, o, 0));

        let units = build_unit_layout(&g, &incoming(&g), &outgoing(&g));

        assert_eq!(units.units.len(), 3);
        assert!(units.unit(p1).is_column());
        assert_eq!(units.unit(p1).nodes, vec![p1, p2]);
        assert!(!units.unit(i).is_column());
    }

    #[test]
    fn marks_combine_as_dual_inlet() {
        let mut g = LayoutGraph::new();
        let c = g.add_node(LayoutNode::new(0, (40.0, 22.0), NodeKind::Combine, 2, 1));
        let units = build_unit_layout(&g, &incoming(&g), &outgoing(&g));
        assert!(units.unit(c).is_dual_inlet());
    }

    #[test]
    fn finds_combine_fanin_pair() {
        let mut g = LayoutGraph::new();
        let p0 = g.add_node(LayoutNode::new(0, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p1 = g.add_node(LayoutNode::new(1, (56.0, 22.0), NodeKind::Param, 1, 1));
        let combine = g.add_node(LayoutNode::new(2, (40.0, 22.0), NodeKind::Combine, 2, 1));
        g.add_edge(LayoutEdge::new(p0, 0, combine, 0));
        g.add_edge(LayoutEdge::new(p1, 0, combine, 1));

        let units = build_unit_layout(&g, &incoming(&g), &outgoing(&g));
        assert_eq!(units.fanin_pairs.len(), 1);
        assert_eq!(units.fanin_pairs[0].combine, units.unit_index(combine));
    }
}
