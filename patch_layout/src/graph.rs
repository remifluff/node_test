use std::collections::HashMap;

/// Stable node key supplied by the editor (typically `NodeIndex::index()`).
pub type LayoutNodeId = usize;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Semantic hint for layout heuristics — does not change topology.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeKind {
    #[default]
    Default,
    Source,
    Sink,
    Param,
    Combine,
    Send,
    Receive,
    DelayIn,
    DelayOut,
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayPairGroup {
    Hex(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub id: LayoutNodeId,
    pub size: (f32, f32),
    pub pos: Point,
    pub kind: NodeKind,
    pub inlets: usize,
    pub outlets: usize,
    /// Links `DelayIn` / `DelayOut` pairs that share no graph edge.
    pub delay_pair: Option<DelayPairGroup>,
}

fn default_port_counts(kind: NodeKind) -> (usize, usize) {
    match kind {
        NodeKind::Source | NodeKind::DelayIn => (0, 1),
        NodeKind::Sink | NodeKind::DelayOut => (1, 0),
        NodeKind::Param => (1, 1),
        NodeKind::Combine => (2, 1),
        NodeKind::Send => (1, 0),
        NodeKind::Receive => (0, 1),
        NodeKind::Comment => (0, 0),
        NodeKind::Default => (1, 1),
    }
}

impl LayoutNode {
    pub fn new(
        id: LayoutNodeId,
        size: (f32, f32),
        kind: NodeKind,
        inlets: usize,
        outlets: usize,
    ) -> Self {
        Self {
            id,
            size,
            pos: Point::default(),
            kind,
            inlets,
            outlets,
            delay_pair: None,
        }
    }

    pub fn new_simple(id: LayoutNodeId, size: (f32, f32), kind: NodeKind) -> Self {
        let (inlets, outlets) = default_port_counts(kind);
        Self::new(id, size, kind, inlets, outlets)
    }

    pub fn with_delay_pair(mut self, hex: u8) -> Self {
        self.delay_pair = Some(DelayPairGroup::Hex(hex));
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutEdge {
    pub from: LayoutNodeId,
    pub from_port: usize,
    pub to: LayoutNodeId,
    pub to_port: usize,
}

impl LayoutEdge {
    pub fn new(from: LayoutNodeId, from_port: usize, to: LayoutNodeId, to_port: usize) -> Self {
        Self {
            from,
            from_port,
            to,
            to_port,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutResult {
    pub positions: HashMap<LayoutNodeId, Point>,
    /// Resized node dimensions (e.g. stretched combine boxes for straight inlet wires).
    pub sizes: HashMap<LayoutNodeId, (f32, f32)>,
}

/// Topology + sizes for layout. Positions on input are ignored unless
/// `LayoutConfig::preserve_existing` is set (future).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutGraph {
    nodes: HashMap<LayoutNodeId, LayoutNode>,
    edges: Vec<LayoutEdge>,
}

impl LayoutGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: LayoutNode) -> LayoutNodeId {
        let id = node.id;
        self.nodes.insert(id, node);
        id
    }

    pub fn add_edge(&mut self, edge: LayoutEdge) {
        self.edges.push(edge);
    }

    pub fn node(&self, id: LayoutNodeId) -> Option<&LayoutNode> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: LayoutNodeId) -> Option<&mut LayoutNode> {
        self.nodes.get_mut(&id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &LayoutNode> {
        self.nodes.values()
    }

    pub fn edges(&self) -> &[LayoutEdge] {
        &self.edges
    }

    pub fn node_ids(&self) -> impl Iterator<Item = LayoutNodeId> + '_ {
        self.nodes.keys().copied()
    }

    pub fn sorted_node_ids(&self) -> Vec<LayoutNodeId> {
        let mut ids: Vec<_> = self.node_ids().collect();
        ids.sort_unstable();
        ids
    }
}
