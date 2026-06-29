use std::collections::{HashMap, HashSet};

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::blocks::{
    build_unit_layout, effective_node_height, effective_node_width, is_dual_inlet_combiner,
    node_layout_height, node_layout_width, place_unit, translate_unit, unit_span_width, UnitLayout,
};
use crate::config::LayoutConfig;
use crate::graph::{LayoutEdge, LayoutGraph, LayoutNodeId, LayoutResult, NodeKind, Point, DelayPairGroup};
use crate::ports::{dual_inlet_node_width, dual_inlet_node_x, outlet_world_x, port_x_offset};

/// Layered DAG layout prioritising straight vertical patch cables (aligned port X).
#[derive(Clone, Copy, Debug, Default)]
pub struct LayeredDagLayout;

pub fn layout(graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
    LayeredDagLayout.layout_impl(graph, config)
}

impl LayeredDagLayout {
    fn layout_impl(&self, graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
        if graph.nodes().next().is_none() {
            return LayoutResult::default();
        }
        straight_wire_layout(graph, config)
    }
}

/// DAG layout: every edge is a vertical line (parent outlet X == child inlet X).
fn straight_wire_layout(graph: &LayoutGraph, config: &LayoutConfig) -> LayoutResult {
    let components = find_layout_components(graph);
    let mut positions = HashMap::new();
    let mut sizes = HashMap::new();
    let mut offset_x = config.origin.x;

    for component in components {
        let component_set: HashSet<LayoutNodeId> = component.iter().copied().collect();
        let (mut comp_positions, comp_sizes) =
            layout_component(graph, config, &component_set);
        let (min_x, max_x, _, _) =
            component_bbox(graph, &comp_positions, &comp_sizes, &component_set);
        let dx = offset_x - min_x;
        for (id, point) in comp_positions.drain() {
            positions.insert(id, Point { x: point.x + dx, y: point.y });
        }
        sizes.extend(comp_sizes);
        offset_x += (max_x - min_x) + config.column_gap();
    }

    LayoutResult { positions, sizes }
}

/// Lay out one weakly connected component (plus delay/send/receive pair links).
fn layout_component(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    component: &HashSet<LayoutNodeId>,
) -> (HashMap<LayoutNodeId, Point>, HashMap<LayoutNodeId, (f32, f32)>) {
    let incoming = build_incoming(graph);
    let outgoing = build_outgoing(graph);
    let order = topo_sort_nodes(graph)
        .into_iter()
        .filter(|id| component.contains(id))
        .collect::<Vec<_>>();

    let mut positions: HashMap<LayoutNodeId, Point> = HashMap::new();
    let mut sizes: HashMap<LayoutNodeId, (f32, f32)> = HashMap::new();
    let root_columns = assign_spine_root_columns(graph, &incoming, component);

    loop {
        let mut progress = false;
        for &id in &order {
            if positions.contains_key(&id) {
                continue;
            }

            let node = graph.node(id).expect("node");
            let edges_in = incoming.get(&id).map(|v| v.as_slice()).unwrap_or(&[]);

            // Receive nodes are placed above their target in a second pass.
            if matches!(node.kind, NodeKind::Receive) {
                continue;
            }

            if is_dual_inlet_combiner(node) && edges_in.len() >= 2 {
                if !dual_inlet_parents_ready(
                    graph,
                    edges_in,
                    &positions,
                    &sizes,
                    &outgoing,
                    &root_columns,
                    config,
                ) {
                    continue;
                }
                let y = dual_inlet_row_y(graph, edges_in, &positions, &sizes, config);
                if let Some((x, w, h)) = dual_inlet_geometry(
                    graph,
                    &positions,
                    &incoming,
                    id,
                    &sizes,
                    &outgoing,
                    &root_columns,
                    config,
                ) {
                    sizes.insert(id, (w, h));
                    positions.insert(id, Point { x, y: config.snap(y) });
                    progress = true;
                }
                continue;
            }

            if edges_in.is_empty() {
                if is_combiner_only_feeder(graph, &outgoing, id) {
                    continue;
                }
                let w = node_layout_width(graph, id);
                let col = root_columns.get(&id).copied().unwrap_or(0);
                let x = config.origin.x + col as f32 * (w + config.column_gap());
                positions.insert(id, Point {
                    x,
                    y: config.origin.y,
                });
                progress = true;
                continue;
            }

            if edges_in.len() == 1 {
                let edge = edges_in[0];
                let parent = edge.from;
                if !positions.contains_key(&parent) {
                    if parent_is_receive(graph, parent) {
                        let receive_h = node_layout_height(graph, parent);
                        let col = root_columns.get(&id).copied().unwrap_or(0);
                        let w = node_layout_width(graph, id);
                        let x = config.origin.x + col as f32 * (w + config.column_gap());
                        let y = config.origin.y + receive_h + config.row_gap();
                        positions.insert(id, Point { x, y: config.snap(y) });
                        progress = true;
                    }
                    continue;
                }
                let x = aligned_child_x(graph, parent, id, edge, &positions, &sizes);
                let y = y_below_parent(graph, parent, &positions, &sizes, config);
                positions.insert(id, Point { x, y: config.snap(y) });
                progress = true;
            }
        }

        let pending = component
            .iter()
            .filter(|&&id| {
                let node = graph.node(id).expect("node");
                !matches!(node.kind, NodeKind::Receive) && !positions.contains_key(&id)
            })
            .count();
        if pending == 0 || !progress {
            break;
        }
    }

    finish_deferred_placement(
        graph,
        config,
        component,
        &order,
        &incoming,
        &outgoing,
        &mut positions,
        &mut sizes,
        &root_columns,
    );
    align_combiner_inlet_feeders(
        graph,
        config,
        component,
        &incoming,
        &outgoing,
        &root_columns,
        &mut positions,
        &mut sizes,
    );
    place_receive_nodes(graph, config, component, &outgoing, &mut positions, &sizes);
    finish_deferred_placement(
        graph,
        config,
        component,
        &order,
        &incoming,
        &outgoing,
        &mut positions,
        &mut sizes,
        &root_columns,
    );

    (positions, sizes)
}

fn signal_hex(graph: &LayoutGraph, id: LayoutNodeId) -> Option<u8> {
    match graph.node(id)?.delay_pair {
        Some(DelayPairGroup::Hex(hex)) => Some(hex),
        None => None,
    }
}

/// Fan-out source outlet for a `[receive]` that is not placed yet.
fn send_outlet_x_for_hex(
    graph: &LayoutGraph,
    hex: u8,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) -> Option<f32> {
    for id in graph.sorted_node_ids() {
        let node = graph.node(id)?;
        if !matches!(node.kind, NodeKind::Send) {
            continue;
        }
        if signal_hex(graph, id) != Some(hex) {
            continue;
        }
        let pos = positions.get(&id)?;
        let w = effective_node_width(graph, id, Some(sizes));
        return Some(outlet_world_x(pos.x, w, 0, node.outlets.max(1)));
    }
    None
}

fn is_combiner_only_feeder(
    graph: &LayoutGraph,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    id: LayoutNodeId,
) -> bool {
    let Some(edges) = outgoing.get(&id) else {
        return false;
    };
    !edges.is_empty()
        && edges.iter().all(|edge| {
            graph
                .node(edge.to)
                .is_some_and(is_dual_inlet_combiner)
        })
}

fn column_outlet_world_x(
    graph: &LayoutGraph,
    id: LayoutNodeId,
    from_port: usize,
    root_columns: &HashMap<LayoutNodeId, u32>,
    config: &LayoutConfig,
) -> Option<f32> {
    let node = graph.node(id)?;
    let col = root_columns.get(&id).copied().unwrap_or(0);
    let w = node_layout_width(graph, id);
    let x = config.origin.x + col as f32 * (w + config.column_gap());
    Some(outlet_world_x(x, w, from_port, node.outlets.max(1)))
}

fn parent_outlet_world_x(
    graph: &LayoutGraph,
    edge: &LayoutEdge,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    root_columns: &HashMap<LayoutNodeId, u32>,
    config: &LayoutConfig,
) -> Option<f32> {
    let parent = edge.from;
    let parent_node = graph.node(parent)?;
    if let Some(parent_pos) = positions.get(&parent) {
        let parent_w = effective_node_width(graph, parent, Some(sizes));
        return Some(outlet_world_x(
            parent_pos.x,
            parent_w,
            edge.from_port,
            parent_node.outlets.max(1),
        ));
    }
    if matches!(parent_node.kind, NodeKind::Receive) {
        let hex = signal_hex(graph, parent)?;
        return send_outlet_x_for_hex(graph, hex, positions, sizes);
    }
    if is_combiner_only_feeder(graph, outgoing, parent) {
        return column_outlet_world_x(graph, parent, edge.from_port, root_columns, config);
    }
    None
}

fn dual_inlet_parents_ready(
    graph: &LayoutGraph,
    edges_in: &[&LayoutEdge],
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    root_columns: &HashMap<LayoutNodeId, u32>,
    config: &LayoutConfig,
) -> bool {
    edges_in.iter().all(|edge| {
        parent_outlet_world_x(
            graph,
            edge,
            positions,
            sizes,
            outgoing,
            root_columns,
            config,
        )
        .is_some()
    })
}

fn reposition_feeder_above_target(
    graph: &LayoutGraph,
    edge: &LayoutEdge,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    config: &LayoutConfig,
) {
    let parent = edge.from;
    if parent_is_receive(graph, parent) {
        return;
    }
    let child = edge.to;
    let Some(child_pos) = positions.get(&child).copied() else {
        return;
    };
    let parent_node = graph.node(parent).expect("parent");
    let child_node = graph.node(child).expect("child");
    let child_w = effective_node_width(graph, child, Some(sizes));
    let parent_w = effective_node_width(graph, parent, Some(sizes));
    let inlet_x =
        child_pos.x + port_x_offset(child_w, edge.to_port, child_node.inlets.max(1));
    let outlet_off = port_x_offset(parent_w, edge.from_port, parent_node.outlets.max(1));
    let x = inlet_x - outlet_off;
    let parent_h = effective_node_height(graph, parent, Some(sizes));
    let y = child_pos.y - config.row_gap() - parent_h;
    positions.insert(parent, Point { x, y: config.snap(y) });
}

/// Snap direct combiner feeders (e.g. `[in]`) to sit one row above their inlet.
fn align_combiner_inlet_feeders(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    component: &HashSet<LayoutNodeId>,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    root_columns: &HashMap<LayoutNodeId, u32>,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
) {
    let mut combines: Vec<LayoutNodeId> = component
        .iter()
        .copied()
        .filter(|&id| graph.node(id).is_some_and(is_dual_inlet_combiner))
        .collect();
    combines.sort_unstable();

    for combine_id in combines {
        let Some(edges) = incoming.get(&combine_id) else {
            continue;
        };
        let Some(combine_y) = positions.get(&combine_id).map(|p| p.y) else {
            continue;
        };

        for edge in edges {
            reposition_feeder_above_target(graph, edge, positions, sizes, config);
        }

        if let Some((x, w, h)) = dual_inlet_geometry(
            graph,
            positions,
            incoming,
            combine_id,
            sizes,
            outgoing,
            root_columns,
            config,
        ) {
            sizes.insert(combine_id, (w, h));
            positions.insert(combine_id, Point { x, y: combine_y });
        }
    }
}

/// Second pass for nodes whose parent `[receive]` was placed after the main loop.
fn finish_deferred_placement(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    component: &HashSet<LayoutNodeId>,
    order: &[LayoutNodeId],
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
    root_columns: &HashMap<LayoutNodeId, u32>,
) {
    loop {
        let mut progress = false;
        for &id in order {
            if positions.contains_key(&id) {
                continue;
            }
            let node = graph.node(id).expect("node");
            if matches!(node.kind, NodeKind::Receive) {
                continue;
            }
            let edges_in = incoming.get(&id).map(|v| v.as_slice()).unwrap_or(&[]);

            if is_dual_inlet_combiner(node) && edges_in.len() >= 2 {
                if !dual_inlet_parents_ready(
                    graph,
                    edges_in,
                    positions,
                    sizes,
                    outgoing,
                    root_columns,
                    config,
                ) {
                    continue;
                }
                let y = edges_in
                    .iter()
                    .filter_map(|edge| y_below_feeder(graph, edge, positions, sizes, config))
                    .fold(config.origin.y, f32::max);
                if let Some((x, w, h)) = dual_inlet_geometry(
                    graph,
                    positions,
                    incoming,
                    id,
                    sizes,
                    outgoing,
                    root_columns,
                    config,
                ) {
                    sizes.insert(id, (w, h));
                    positions.insert(id, Point { x, y: config.snap(y) });
                    progress = true;
                }
                continue;
            }

            if edges_in.is_empty() {
                if is_combiner_only_feeder(graph, outgoing, id) {
                    continue;
                }
                let w = node_layout_width(graph, id);
                let col = root_columns.get(&id).copied().unwrap_or(0);
                let x = config.origin.x + col as f32 * (w + config.column_gap());
                positions.insert(id, Point { x, y: config.origin.y });
                progress = true;
                continue;
            }

            if edges_in.len() == 1 {
                let edge = edges_in[0];
                let parent = edge.from;
                if !positions.contains_key(&parent) {
                    if parent_is_receive(graph, parent) {
                        let receive_h = node_layout_height(graph, parent);
                        let col = root_columns.get(&id).copied().unwrap_or(0);
                        let w = node_layout_width(graph, id);
                        let x = config.origin.x + col as f32 * (w + config.column_gap());
                        let y = config.origin.y + receive_h + config.row_gap();
                        positions.insert(id, Point { x, y: config.snap(y) });
                        progress = true;
                    }
                    continue;
                }
                let x = aligned_child_x(graph, parent, id, edge, positions, sizes);
                let y = y_below_parent(graph, parent, positions, sizes, config);
                positions.insert(id, Point { x, y: config.snap(y) });
                progress = true;
            }
        }

        let pending = component
            .iter()
            .filter(|&&id| {
                graph
                    .node(id)
                    .is_some_and(|n| !matches!(n.kind, NodeKind::Receive))
                    && !positions.contains_key(&id)
            })
            .count();
        if pending == 0 || !progress {
            break;
        }
    }
}

fn parent_is_receive(graph: &LayoutGraph, id: LayoutNodeId) -> bool {
    graph
        .node(id)
        .is_some_and(|node| matches!(node.kind, NodeKind::Receive))
}

/// Top of a vertical spine: true source/root, or first node fed by `[receive]`.
fn is_spine_root(
    graph: &LayoutGraph,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    id: LayoutNodeId,
) -> bool {
    let Some(node) = graph.node(id) else {
        return false;
    };
    if matches!(node.kind, NodeKind::Receive) {
        return false;
    }
    let edges = incoming.get(&id).map(|v| v.as_slice()).unwrap_or(&[]);
    match edges.len() {
        0 => true,
        1 => parent_is_receive(graph, edges[0].from),
        _ => false,
    }
}

fn assign_spine_root_columns(
    graph: &LayoutGraph,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    component: &HashSet<LayoutNodeId>,
) -> HashMap<LayoutNodeId, u32> {
    let mut spines: Vec<LayoutNodeId> = component
        .iter()
        .copied()
        .filter(|&id| is_spine_root(graph, incoming, id))
        .collect();
    spines.sort_unstable();
    spines
        .into_iter()
        .enumerate()
        .map(|(col, id)| (id, col as u32))
        .collect()
}

fn place_receive_nodes(
    graph: &LayoutGraph,
    config: &LayoutConfig,
    component: &HashSet<LayoutNodeId>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) {
    let mut receives: Vec<LayoutNodeId> = component
        .iter()
        .copied()
        .filter(|&id| {
            graph
                .node(id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Receive))
        })
        .collect();
    receives.sort_by_key(|&id| {
        outgoing
            .get(&id)
            .and_then(|edges| edges.first())
            .map(|edge| edge.to)
            .unwrap_or(id)
    });

    for id in receives {
        let node = graph.node(id).expect("receive");
        let Some(edges_out) = outgoing.get(&id) else {
            continue;
        };
        let Some(edge) = edges_out.first() else {
            continue;
        };
        let target = edge.to;
        let Some(target_pos) = positions.get(&target) else {
            continue;
        };
        let target_node = graph.node(target).expect("target");
        let receive_w = node_layout_width(graph, id);
        let receive_h = node_layout_height(graph, id);
        let target_w = effective_node_width(graph, target, Some(sizes));
        let inlet_x = target_pos.x
            + port_x_offset(target_w, edge.to_port, target_node.inlets.max(1));
        let outlet_off = port_x_offset(receive_w, edge.from_port, node.outlets.max(1));
        let x = inlet_x - outlet_off;
        let y = target_pos.y - config.row_gap() - receive_h;
        positions.insert(id, Point { x, y: config.snap(y) });
    }
}

fn find_layout_components(graph: &LayoutGraph) -> Vec<Vec<LayoutNodeId>> {
    let ids = graph.sorted_node_ids();
    let mut parent: HashMap<LayoutNodeId, LayoutNodeId> =
        ids.iter().copied().map(|id| (id, id)).collect();

    fn find(parent: &mut HashMap<LayoutNodeId, LayoutNodeId>, id: LayoutNodeId) -> LayoutNodeId {
        let p = parent[&id];
        if p != id {
            let root = find(parent, p);
            parent.insert(id, root);
            root
        } else {
            id
        }
    }

    fn union(parent: &mut HashMap<LayoutNodeId, LayoutNodeId>, a: LayoutNodeId, b: LayoutNodeId) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            let (keep, drop) = if ra < rb { (ra, rb) } else { (rb, ra) };
            parent.insert(drop, keep);
        }
    }

    for edge in graph.edges() {
        union(&mut parent, edge.from, edge.to);
    }

    let mut by_hex: HashMap<u8, Vec<LayoutNodeId>> = HashMap::new();
    for &id in &ids {
        let Some(node) = graph.node(id) else {
            continue;
        };
        if let Some(DelayPairGroup::Hex(hex)) = node.delay_pair {
            by_hex.entry(hex).or_default().push(id);
        }
    }
    for nodes in by_hex.values() {
        if let Some(&first) = nodes.first() {
            for &id in nodes.iter().skip(1) {
                union(&mut parent, first, id);
            }
        }
    }

    let mut groups: HashMap<LayoutNodeId, Vec<LayoutNodeId>> = HashMap::new();
    for id in ids {
        let root = find(&mut parent, id);
        groups.entry(root).or_default().push(id);
    }

    let mut components: Vec<Vec<LayoutNodeId>> = groups.into_values().collect();
    components.sort_by_key(|nodes| nodes.iter().copied().min().unwrap_or(0));
    components
}

fn component_bbox(
    graph: &LayoutGraph,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    component: &HashSet<LayoutNodeId>,
) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for &id in component {
        let Some(point) = positions.get(&id) else {
            continue;
        };
        let w = effective_node_width(graph, id, Some(sizes));
        let h = effective_node_height(graph, id, Some(sizes));
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x + w);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y + h);
    }

    if min_x.is_infinite() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (min_x, max_x, min_y, max_y)
    }
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

fn aligned_child_x(
    graph: &LayoutGraph,
    parent: LayoutNodeId,
    child: LayoutNodeId,
    edge: &LayoutEdge,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) -> f32 {
    let parent_node = graph.node(parent).expect("parent");
    let child_node = graph.node(child).expect("child");
    let parent_pos = positions.get(&parent).expect("parent placed");
    let parent_w = effective_node_width(graph, parent, Some(sizes));
    let child_w = effective_node_width(graph, child, Some(sizes));
    let out_x = outlet_world_x(
        parent_pos.x,
        parent_w,
        edge.from_port,
        parent_node.outlets.max(1),
    );
    out_x - port_x_offset(child_w, edge.to_port, child_node.inlets.max(1))
}

fn y_below_parent(
    graph: &LayoutGraph,
    parent: LayoutNodeId,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    config: &LayoutConfig,
) -> f32 {
    let parent_pos = positions.get(&parent).expect("parent placed");
    parent_pos.y + effective_node_height(graph, parent, Some(sizes)) + config.row_gap()
}

fn y_below_feeder(
    graph: &LayoutGraph,
    edge: &LayoutEdge,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    config: &LayoutConfig,
) -> Option<f32> {
    let parent = edge.from;
    if positions.contains_key(&parent) {
        return Some(y_below_parent(graph, parent, positions, sizes, config));
    }
    if !parent_is_receive(graph, parent) {
        return None;
    }
    let hex = signal_hex(graph, parent)?;
    for id in graph.sorted_node_ids() {
        let node = graph.node(id)?;
        if !matches!(node.kind, NodeKind::Send) || signal_hex(graph, id) != Some(hex) {
            continue;
        }
        if !positions.contains_key(&id) {
            continue;
        }
        let below_send = y_below_parent(graph, id, positions, sizes, config);
        let receive_h = node_layout_height(graph, parent);
        return Some(below_send + receive_h + config.row_gap());
    }
    None
}

fn dual_inlet_row_y(
    graph: &LayoutGraph,
    edges_in: &[&LayoutEdge],
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    config: &LayoutConfig,
) -> f32 {
    edges_in
        .iter()
        .filter_map(|edge| y_below_feeder(graph, edge, positions, sizes, config))
        .fold(config.origin.y, f32::max)
}

/// Stretched combiner box so each inlet aligns with its parent outlet.
fn dual_inlet_geometry(
    graph: &LayoutGraph,
    positions: &HashMap<LayoutNodeId, Point>,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    combine_id: LayoutNodeId,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    root_columns: &HashMap<LayoutNodeId, u32>,
    config: &LayoutConfig,
) -> Option<(f32, f32, f32)> {
    let node = graph.node(combine_id)?;
    let min_w = node_layout_width(graph, combine_id);
    let h = node_layout_height(graph, combine_id);
    let edges = incoming.get(&combine_id)?;

    let mut targets: Vec<(usize, f32)> = Vec::new();
    for edge in edges {
        let Some(out_x) = parent_outlet_world_x(
            graph,
            edge,
            positions,
            sizes,
            outgoing,
            root_columns,
            config,
        ) else {
            return None;
        };
        targets.push((edge.to_port, out_x));
    }

    if targets.is_empty() {
        return None;
    }

    targets.sort_by_key(|(port, _)| *port);
    let in0 = targets.iter().find(|(port, _)| *port == 0).map(|(_, x)| *x);
    let in1 = targets.iter().find(|(port, _)| *port == 1).map(|(_, x)| *x);

    match (in0, in1) {
        (Some(x0), Some(x1)) => {
            let w = dual_inlet_node_width(min_w, x0, x1);
            let x = dual_inlet_node_x(w, x0, node.inlets);
            Some((x, w, h))
        }
        (Some(x0), None) => {
            let w = min_w;
            let x = x0 - port_x_offset(w, 0, node.inlets);
            Some((x, w, h))
        }
        _ => None,
    }
}

/// Left edge of a rank column — each prior rank advances by its widest layer span.
fn rank_column_start(
    graph: &LayoutGraph,
    units: &UnitLayout,
    layers: &HashMap<u32, Vec<usize>>,
    rank: u32,
    config: &LayoutConfig,
) -> f32 {
    let mut x = config.origin.x;
    for r in 0..rank {
        let Some(layer) = layers.get(&r) else {
            continue;
        };
        let layer_width = layer_width(graph, units, layer, config);
        x += layer_width.max(config.column_spacing) + config.column_gap();
    }
    x
}

fn layer_width(
    graph: &LayoutGraph,
    units: &UnitLayout,
    layer: &[usize],
    config: &LayoutConfig,
) -> f32 {
    if layer.is_empty() {
        return 0.0;
    }
    let units_width: f32 = layer
        .iter()
        .map(|&unit_ix| unit_span_width(graph, &units.units[unit_ix], config.node_gap, None))
        .sum();
    units_width + config.column_gap() * layer.len().saturating_sub(1) as f32
}

fn build_incoming(graph: &LayoutGraph) -> HashMap<LayoutNodeId, Vec<&LayoutEdge>> {
    let mut incoming: HashMap<LayoutNodeId, Vec<&LayoutEdge>> = HashMap::new();
    for edge in graph.edges() {
        incoming.entry(edge.to).or_default().push(edge);
    }
    incoming
}

fn build_outgoing(graph: &LayoutGraph) -> HashMap<LayoutNodeId, Vec<&LayoutEdge>> {
    let mut outgoing: HashMap<LayoutNodeId, Vec<&LayoutEdge>> = HashMap::new();
    for edge in graph.edges() {
        outgoing.entry(edge.from).or_default().push(edge);
    }
    outgoing
}

/// Stable unit order within each rank (by head node id).
fn stable_sort_layers(
    layers: &mut HashMap<u32, Vec<usize>>,
    max_rank: u32,
    units: &UnitLayout,
) {
    for rank in 0..=max_rank {
        if let Some(layer) = layers.get_mut(&rank) {
            layer.sort_by_key(|&unit_ix| units.units[unit_ix].head());
        }
    }
}

/// Single-pass placement: column X from ranks, Y from upstream units, port X when unambiguous.
fn assign_unit_positions(
    graph: &LayoutGraph,
    units: &UnitLayout,
    layers: &HashMap<u32, Vec<usize>>,
    max_rank: u32,
    node_ranks: &HashMap<LayoutNodeId, u32>,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    config: &LayoutConfig,
    positions: &mut HashMap<LayoutNodeId, Point>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
) {
    let mut unit_x: HashMap<usize, f32> = HashMap::new();
    for rank in 0..=max_rank {
        let Some(layer) = layers.get(&rank) else {
            continue;
        };
        let mut rank_x = rank_column_start(graph, units, layers, rank, config);
        for &unit_ix in layer {
            unit_x.insert(unit_ix, rank_x);
            rank_x += unit_span_width(graph, &units.units[unit_ix], config.node_gap, Some(sizes))
                + config.column_gap();
        }
    }

    let mut unit_order: Vec<usize> = (0..units.units.len()).collect();
    unit_order.sort_by_key(|&unit_ix| {
        let head = units.units[unit_ix].head();
        (node_ranks.get(&head).copied().unwrap_or(0), head)
    });

    let mut placed_units: HashSet<usize> = HashSet::new();

    for unit_ix in unit_order {
        if placed_units.contains(&unit_ix) {
            continue;
        }

        if let Some(pair) = units.fanin_pair_left(unit_ix) {
            let left_anchor = compute_unit_anchor(
                graph,
                units,
                pair.left,
                &unit_x,
                node_ranks,
                incoming,
                outgoing,
                config,
                positions,
                sizes,
            );
            let right_anchor = compute_unit_anchor(
                graph,
                units,
                pair.right,
                &unit_x,
                node_ranks,
                incoming,
                outgoing,
                config,
                positions,
                sizes,
            );
            let y = left_anchor.y.max(right_anchor.y);
            place_unit(
                graph,
                &units.units[pair.left],
                Point {
                    x: left_anchor.x,
                    y,
                },
                config.row_gap(),
                positions,
            );
            place_unit(
                graph,
                &units.units[pair.right],
                Point {
                    x: right_anchor.x,
                    y,
                },
                config.row_gap(),
                positions,
            );
            placed_units.insert(pair.left);
            placed_units.insert(pair.right);
            continue;
        }

        let anchor = compute_unit_anchor(
            graph,
            units,
            unit_ix,
            &unit_x,
            node_ranks,
            incoming,
            outgoing,
            config,
            positions,
            sizes,
        );
        place_unit(
            graph,
            &units.units[unit_ix],
            anchor,
            config.row_gap(),
            positions,
        );
        placed_units.insert(unit_ix);
    }
}

fn compute_unit_anchor(
    graph: &LayoutGraph,
    units: &UnitLayout,
    unit_ix: usize,
    unit_x: &HashMap<usize, f32>,
    node_ranks: &HashMap<LayoutNodeId, u32>,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    config: &LayoutConfig,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
) -> Point {
    let unit = &units.units[unit_ix];
    let head = unit.head();
    let rank = node_ranks.get(&head).copied().unwrap_or(0);
    let mut x = unit_x.get(&unit_ix).copied().unwrap_or(config.origin.x);
    let mut y = config.origin.y;

    let placement_id = if unit.dual_inlet { unit.primary() } else { head };

    if rank > 0 {
        if let Some(edges) = incoming.get(&placement_id) {
            for edge in edges {
                let parent_unit = &units.units[units.node_to_unit[&edge.from]];
                let parent_tail = parent_unit.tail();
                if let Some(parent_pos) = positions.get(&parent_tail) {
                    let parent_h = effective_node_height(graph, parent_tail, Some(sizes));
                    y = y.max(parent_pos.y + parent_h + config.row_gap());
                }
            }

            if unit.dual_inlet {
                let empty_roots = HashMap::new();
                if let Some((cx, cw, ch)) = dual_inlet_geometry(
                    graph,
                    positions,
                    incoming,
                    placement_id,
                    sizes,
                    outgoing,
                    &empty_roots,
                    config,
                ) {
                    sizes.insert(placement_id, (cw, ch));
                    x = cx;
                } else {
                    x = dual_inlet_aligned_x(
                        graph,
                        units,
                        positions,
                        incoming,
                        placement_id,
                        x,
                        Some(sizes),
                    );
                }
            } else if let Some(branch_x) = branch_x_for_dual_inlet_partner(
                graph,
                units,
                unit_ix,
                node_ranks,
                outgoing,
                positions,
                unit_x,
                sizes,
            ) {
                x = branch_x;
                y = dual_inlet_feeder_y(
                    graph,
                    units,
                    unit_ix,
                    outgoing,
                    positions,
                    x,
                    y,
                    config.row_gap(),
                );
            } else if !has_parallel_siblings(units, unit_ix, incoming, node_ranks) {
                if let Some(aligned_x) = port_aligned_head_x(graph, units, positions, unit, edges) {
                    x = aligned_x;
                }
            }
        }
    }

    Point { x, y }
}

fn align_combine_feeder_ranks(units: &UnitLayout, unit_ranks: &mut [u32]) {
    for pair in &units.fanin_pairs {
        let combine_rank = unit_ranks[pair.combine];
        let target = combine_rank.saturating_sub(1);
        unit_ranks[pair.left] = target;
        unit_ranks[pair.right] = target;
    }
}

/// Fallback X when parent positions for both inlets are not yet available.
fn dual_inlet_aligned_x(
    graph: &LayoutGraph,
    units: &UnitLayout,
    positions: &HashMap<LayoutNodeId, Point>,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    node_id: LayoutNodeId,
    fallback_x: f32,
    sizes: Option<&HashMap<LayoutNodeId, (f32, f32)>>,
) -> f32 {
    let Some(node) = graph.node(node_id) else {
        return fallback_x;
    };
    let Some(edges) = incoming.get(&node_id) else {
        return fallback_x;
    };

    let width = effective_node_width(graph, node_id, sizes);
    let mut by_port: Vec<(usize, f32)> = Vec::new();
    for edge in edges {
        let parent_unit = &units.units[units.node_to_unit[&edge.from]];
        let parent_tail = parent_unit.tail();
        let Some(parent) = graph.node(parent_tail) else {
            continue;
        };
        let Some(parent_pos) = positions.get(&parent_tail) else {
            continue;
        };
        let out_x = outlet_world_x(
            parent_pos.x,
            parent.size.0,
            edge.from_port,
            parent.outlets,
        );
        let in_off = port_x_offset(width, edge.to_port, node.inlets);
        by_port.push((edge.to_port, out_x - in_off));
    }

    if by_port.is_empty() {
        return fallback_x;
    }

    by_port.sort_by_key(|(port, _)| *port);
    by_port[0].1
}

/// When this unit feeds inlet 1 of a dual-inlet combiner and the inlet-0 branch is
/// already placed, lock this branch so both combiner inlets stay vertical with their wires.
fn branch_x_for_dual_inlet_partner(
    graph: &LayoutGraph,
    units: &UnitLayout,
    unit_ix: usize,
    node_ranks: &HashMap<LayoutNodeId, u32>,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    positions: &HashMap<LayoutNodeId, Point>,
    unit_x: &HashMap<usize, f32>,
    sizes: &mut HashMap<LayoutNodeId, (f32, f32)>,
) -> Option<f32> {
    let unit = &units.units[unit_ix];
    let tail = unit.tail();
    let tail_node = graph.node(tail)?;
    let rank = node_ranks.get(&tail).copied().unwrap_or(0);

    let edges = outgoing.get(&tail)?;
    let combine_edge = edges.iter().find(|edge| {
        units
            .units
            .get(units.node_to_unit[&edge.to])
            .is_some_and(|u| u.dual_inlet && edge.to_port >= 1)
    })?;
    let combine_id = combine_edge.to;
    let combine = graph.node(combine_id)?;

    let partner_edge = graph.edges().iter().find(|edge| {
        edge.to == combine_id
            && edge.to_port == 0
            && units.node_to_unit[&edge.from] != unit_ix
    })?;
    let partner_unit_ix = units.node_to_unit[&partner_edge.from];
    if node_ranks.get(&units.units[partner_unit_ix].tail()).copied().unwrap_or(0) != rank {
        return None;
    }

    let partner_tail = units.units[partner_unit_ix].tail();
    let partner = graph.node(partner_tail)?;
    let partner_pos = positions.get(&partner_tail)?;
    let partner_out_x = outlet_world_x(
        partner_pos.x,
        partner.size.0,
        partner_edge.from_port,
        partner.outlets,
    );

    let rank_x = unit_x.get(&unit_ix).copied().unwrap_or(partner_pos.x);
    let tail_out_x = outlet_world_x(
        rank_x,
        tail_node.size.0,
        combine_edge.from_port,
        tail_node.outlets,
    );

    let min_w = node_layout_width(graph, combine_id);
    let h = node_layout_height(graph, combine_id);
    let combine_w = dual_inlet_node_width(min_w, partner_out_x, tail_out_x);
    let combine_x = dual_inlet_node_x(combine_w, partner_out_x, combine.inlets);
    sizes.insert(combine_id, (combine_w, h));

    let target_in1_x = combine_x + port_x_offset(combine_w, 1, combine.inlets);
    let tail_out_off = port_x_offset(
        tail_node.size.0,
        combine_edge.from_port,
        tail_node.outlets,
    );
    Some(target_in1_x - tail_out_off)
}

fn dual_inlet_feeder_y(
    graph: &LayoutGraph,
    units: &UnitLayout,
    unit_ix: usize,
    outgoing: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    positions: &HashMap<LayoutNodeId, Point>,
    x: f32,
    y: f32,
    gap: f32,
) -> f32 {
    let unit = &units.units[unit_ix];
    let tail = unit.tail();
    let Some(edges) = outgoing.get(&tail) else {
        return y;
    };
    let Some(combine_edge) = edges.iter().find(|edge| {
        units
            .units
            .get(units.node_to_unit[&edge.to])
            .is_some_and(|u| u.dual_inlet && edge.to_port >= 1)
    }) else {
        return y;
    };
    let combine_id = combine_edge.to;

    let partner_edge = graph.edges().iter().find(|edge| {
        edge.to == combine_id && edge.to_port == 0 && units.node_to_unit[&edge.from] != unit_ix
    });
    let Some(partner_edge) = partner_edge else {
        return y;
    };

    let partner_unit_ix = units.node_to_unit[&partner_edge.from];
    let partner_tail = units.units[partner_unit_ix].tail();
    let Some(partner_pos) = positions.get(&partner_tail) else {
        return y;
    };

    let proposed = NodeRect {
        x,
        y,
        w: effective_node_width(graph, tail, None),
        h: effective_node_height(graph, tail, None),
    };
    let partner_rect = node_rect(graph, partner_tail, *partner_pos, None);

    if proposed.overlaps(partner_rect, gap) {
        partner_rect.bottom() + gap
    } else {
        y
    }
}

fn unit_feeds_dual_inlet(
    graph: &LayoutGraph,
    units: &UnitLayout,
    from_unit_ix: usize,
    to_unit_ix: usize,
) -> bool {
    if !units.units[to_unit_ix].dual_inlet {
        return false;
    }
    let combine_id = units.units[to_unit_ix].primary();
    let tail = units.units[from_unit_ix].tail();
    graph
        .edges()
        .iter()
        .any(|edge| edge.from == tail && edge.to == combine_id)
}

fn share_dual_inlet_combine(units: &UnitLayout, a_ix: usize, b_ix: usize) -> Option<LayoutNodeId> {
    let a = &units.units[a_ix];
    let b = &units.units[b_ix];
    if a.dual_inlet {
        return Some(a.primary());
    }
    if b.dual_inlet {
        return Some(b.primary());
    }
    None
}

fn co_feed_dual_inlet(
    graph: &LayoutGraph,
    units: &UnitLayout,
    a_ix: usize,
    b_ix: usize,
) -> bool {
    let Some(combine_id) = share_dual_inlet_combine(units, a_ix, b_ix) else {
        return false;
    };
    let a_tail = units.units[a_ix].tail();
    let b_tail = units.units[b_ix].tail();
    let a_feeds = graph
        .edges()
        .iter()
        .any(|e| e.from == a_tail && e.to == combine_id);
    let b_feeds = graph
        .edges()
        .iter()
        .any(|e| e.from == b_tail && e.to == combine_id);
    a_feeds && b_feeds
}

fn skip_gap_separation(
    graph: &LayoutGraph,
    units: &UnitLayout,
    prior_ix: usize,
    mobile_ix: usize,
) -> bool {
    if unit_feeds_dual_inlet(graph, units, prior_ix, mobile_ix)
        || unit_feeds_dual_inlet(graph, units, mobile_ix, prior_ix)
    {
        return true;
    }
    co_feed_dual_inlet(graph, units, prior_ix, mobile_ix)
}

fn port_aligned_head_x(
    graph: &LayoutGraph,
    units: &UnitLayout,
    positions: &HashMap<LayoutNodeId, Point>,
    unit: &crate::blocks::LayoutUnit,
    edges: &[&LayoutEdge],
) -> Option<f32> {
    let head = unit.head();
    let head_node = graph.node(head)?;
    let mut desired_x: Vec<f32> = Vec::new();
    for edge in edges {
        let parent_unit = &units.units[units.node_to_unit[&edge.from]];
        let parent_tail = parent_unit.tail();
        let parent = graph.node(parent_tail)?;
        let parent_pos = positions.get(&parent_tail)?;
        let parent_out_x = outlet_world_x(
            parent_pos.x,
            parent.size.0,
            edge.from_port,
            parent.outlets,
        );
        let child_in_off =
            crate::ports::port_x_offset(head_node.size.0, edge.to_port, head_node.inlets.max(1));
        desired_x.push(parent_out_x - child_in_off);
    }
    if desired_x.is_empty() {
        return None;
    }
    desired_x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(desired_x[desired_x.len() / 2])
}

/// True when another unit at the same rank shares a parent unit (parallel branches).
fn has_parallel_siblings(
    units: &UnitLayout,
    unit_ix: usize,
    incoming: &HashMap<LayoutNodeId, Vec<&LayoutEdge>>,
    node_ranks: &HashMap<LayoutNodeId, u32>,
) -> bool {
    let unit = &units.units[unit_ix];
    let head = unit.head();
    let rank = node_ranks.get(&head).copied().unwrap_or(0);
    let Some(edges) = incoming.get(&head) else {
        return false;
    };

    for edge in edges {
        let parent_unit_ix = units.node_to_unit[&edge.from];
        let siblings = units.units.iter().enumerate().filter(|(ix, u)| {
            *ix != unit_ix
                && node_ranks.get(&u.head()).copied().unwrap_or(0) == rank
                && incoming.get(&u.head()).is_some_and(|ins| {
                    ins.iter()
                        .any(|e| units.node_to_unit[&e.from] == parent_unit_ix)
                })
        });
        if siblings.count() > 0 {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, Debug)]
struct NodeRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl NodeRect {
    fn right(self) -> f32 {
        self.x + self.w
    }

    fn bottom(self) -> f32 {
        self.y + self.h
    }

    fn overlaps(self, other: Self, gap: f32) -> bool {
        self.overlaps_axis(other, gap, gap)
    }

    fn overlaps_axis(self, other: Self, gap_x: f32, gap_y: f32) -> bool {
        self.x < other.right() + gap_x
            && other.x < self.right() + gap_x
            && self.y < other.bottom() + gap_y
            && other.y < self.bottom() + gap_y
    }

    fn separated_by(self, other: Self, gap_x: f32, gap_y: f32) -> bool {
        self.right() + gap_x <= other.x
            || other.right() + gap_x <= self.x
            || self.bottom() + gap_y <= other.y
            || other.bottom() + gap_y <= self.y
    }
}

fn node_rect(
    graph: &LayoutGraph,
    id: LayoutNodeId,
    pos: Point,
    sizes: Option<&HashMap<LayoutNodeId, (f32, f32)>>,
) -> NodeRect {
    NodeRect {
        x: pos.x,
        y: pos.y,
        w: effective_node_width(graph, id, sizes),
        h: effective_node_height(graph, id, sizes),
    }
}

fn unit_bbox(
    graph: &LayoutGraph,
    unit: &crate::blocks::LayoutUnit,
    positions: &HashMap<LayoutNodeId, Point>,
    sizes: Option<&HashMap<LayoutNodeId, (f32, f32)>>,
) -> NodeRect {
    let mut rects: Vec<NodeRect> = unit
        .nodes
        .iter()
        .filter_map(|&id| positions.get(&id).map(|pos| node_rect(graph, id, *pos, sizes)))
        .collect();
    let first = rects.pop().expect("unit has nodes");
    rects.into_iter().fold(first, |acc, r| NodeRect {
        x: acc.x.min(r.x),
        y: acc.y.min(r.y),
        w: acc.right().max(r.right()) - acc.x.min(r.x),
        h: acc.bottom().max(r.bottom()) - acc.y.min(r.y),
    })
}

/// Push `mobile` away from `fixed` by at least the axis gaps (returns delta for mobile).
fn separation_push(mobile: NodeRect, fixed: NodeRect, gap_x: f32, gap_y: f32) -> (f32, f32) {
    if !mobile.overlaps_axis(fixed, gap_x, gap_y) {
        return (0.0, 0.0);
    }

    let overlap_x =
        (fixed.right().min(mobile.right()) - fixed.x.max(mobile.x)).max(0.0);
    let overlap_y = (fixed
        .bottom()
        .min(mobile.bottom())
        - fixed.y.max(mobile.y))
    .max(0.0);

    if overlap_x <= 0.0 || overlap_y <= 0.0 {
        return (0.0, 0.0);
    }

    let same_column = (fixed.x - mobile.x).abs() < gap_x;
    if same_column || overlap_y <= overlap_x {
        (0.0, fixed.bottom() + gap_y - mobile.y)
    } else {
        (fixed.right() + gap_x - mobile.x, 0.0)
    }
}

/// One-way gap pass: each unit only moves later in sort order (stable, no oscillation).
fn enforce_minimum_gaps(
    graph: &LayoutGraph,
    units: &UnitLayout,
    positions: &mut HashMap<LayoutNodeId, Point>,
    node_ranks: &HashMap<LayoutNodeId, u32>,
    config: &LayoutConfig,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) {
    let column_gap = config.column_gap();
    let row_gap = config.min_row_gap;
    let mut unit_order: Vec<usize> = (0..units.units.len()).collect();
    unit_order.sort_by_key(|&unit_ix| {
        let head = units.units[unit_ix].head();
        (node_ranks.get(&head).copied().unwrap_or(0), head)
    });

    for i in 1..unit_order.len() {
        let unit_ix = unit_order[i];
        let unit = &units.units[unit_ix];
        let max_iters = unit_order.len().saturating_mul(2).max(1);

        for _ in 0..max_iters {
            let mut dx = 0.0f32;
            let mut dy = 0.0f32;
            let mobile = unit_bbox(graph, unit, positions, Some(sizes));

            for &prior_ix in &unit_order[..i] {
                if skip_gap_separation(graph, units, prior_ix, unit_ix) {
                    continue;
                }
                let prior = &units.units[prior_ix];
                let fixed = unit_bbox(graph, prior, positions, Some(sizes));
                let shifted = NodeRect {
                    x: mobile.x + dx,
                    y: mobile.y + dy,
                    w: mobile.w,
                    h: mobile.h,
                };
                let (pdx, pdy) = separation_push(shifted, fixed, column_gap, row_gap);
                dx = dx.max(pdx);
                dy = dy.max(pdy);
            }

            if dx <= 0.0 && dy <= 0.0 {
                break;
            }
            translate_unit(unit, dx, dy, positions);
        }
    }
}

pub(crate) fn layout_respects_unit_gaps(
    graph: &LayoutGraph,
    positions: &HashMap<LayoutNodeId, Point>,
    config: &LayoutConfig,
    sizes: &HashMap<LayoutNodeId, (f32, f32)>,
) -> bool {
    let incoming = build_incoming(graph);
    let outgoing = build_outgoing(graph);
    let units = build_unit_layout(graph, &incoming, &outgoing);
    let gap_x = config.column_gap();
    let gap_y = config.min_row_gap;

    for i in 0..units.units.len() {
        for j in (i + 1)..units.units.len() {
            if skip_gap_separation(graph, &units, i, j) {
                continue;
            }
            let a = unit_bbox(graph, &units.units[i], positions, Some(sizes));
            let b = unit_bbox(graph, &units.units[j], positions, Some(sizes));
            if !a.separated_by(b, gap_x, gap_y) {
                return false;
            }
        }
    }
    true
}

fn snap_for_flow(point: Point, config: &LayoutConfig) -> Point {
    // Snap Y to grid; keep X exact so outlet/inlet pairs stay vertically aligned.
    Point {
        x: point.x,
        y: config.snap(point.y),
    }
}

fn build_unit_petgraph(
    graph: &LayoutGraph,
    units: &UnitLayout,
) -> (
    DiGraph<(), ()>,
    HashMap<NodeIndex, usize>,
    HashMap<usize, NodeIndex>,
) {
    let mut pet = DiGraph::new();
    let mut ix_to_unit = HashMap::new();
    let mut unit_to_ix = HashMap::new();

    for (unit_ix, _) in units.units.iter().enumerate() {
        let ix = pet.add_node(());
        ix_to_unit.insert(ix, unit_ix);
        unit_to_ix.insert(unit_ix, ix);
    }

    for edge in graph.edges() {
        let from_unit = units.node_to_unit[&edge.from];
        let to_unit = units.node_to_unit[&edge.to];
        if from_unit != to_unit {
            pet.add_edge(unit_to_ix[&from_unit], unit_to_ix[&to_unit], ());
        }
    }

    (pet, ix_to_unit, unit_to_ix)
}

fn assign_unit_ranks(
    pet: &DiGraph<(), ()>,
    ix_to_unit: &HashMap<NodeIndex, usize>,
    graph: &LayoutGraph,
    units: &UnitLayout,
    config: &LayoutConfig,
) -> Vec<u32> {
    let n = units.units.len();
    let mut ranks = vec![0u32; n];

    for ix in pet.node_indices() {
        let unit_ix = ix_to_unit[&ix];
        let unit = &units.units[unit_ix];
        let head = unit.head();
        let is_source = pet.edges_directed(ix, Direction::Incoming).next().is_none()
            || (config.pin_sources_left
                && graph
                    .node(head)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Source | NodeKind::DelayOut)));
        if is_source {
            ranks[unit_ix] = 0;
        }
    }

    let order = toposort(pet, None).unwrap_or_else(|_| pet.node_indices().collect());
    for ix in order {
        let unit_ix = ix_to_unit[&ix];
        let base = ranks[unit_ix];
        for edge in pet.edges_directed(ix, Direction::Outgoing) {
            let target_ix = ix_to_unit[&edge.target()];
            ranks[target_ix] = ranks[target_ix].max(base + 1);
        }
    }

    if config.pin_sinks_right {
        let max_rank = ranks.iter().copied().max().unwrap_or(0);
        for ix in pet.node_indices() {
            let unit_ix = ix_to_unit[&ix];
            let tail = units.units[unit_ix].tail();
            if graph
                .node(tail)
                .is_some_and(|n| matches!(n.kind, NodeKind::Sink | NodeKind::DelayIn))
            {
                ranks[unit_ix] = max_rank;
            }
        }
    }

    normalize_unit_ranks(&mut ranks);
    ranks
}

fn unit_ranks_to_node_ranks(units: &UnitLayout, unit_ranks: &[u32]) -> HashMap<LayoutNodeId, u32> {
    let mut ranks = HashMap::new();
    for (unit_ix, unit) in units.units.iter().enumerate() {
        let rank = unit_ranks.get(unit_ix).copied().unwrap_or(0);
        for &id in &unit.nodes {
            ranks.insert(id, rank);
        }
    }
    ranks
}

fn normalize_unit_ranks(ranks: &mut [u32]) {
    if ranks.is_empty() {
        return;
    }
    let min = ranks.iter().copied().min().unwrap_or(0);
    for rank in ranks.iter_mut() {
        *rank -= min;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LayoutGraph, LayoutNode};
    use crate::ports::{inlet_world_x, outlet_world_x};

    #[test]
    fn chain_ports_share_x() {
        let mut g = LayoutGraph::new();
        let a = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let b = g.add_node(LayoutNode::new(1, (56.0, 22.0), NodeKind::Param, 1, 1));
        let c = g.add_node(LayoutNode::new(2, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(a, 0, b, 0));
        g.add_edge(LayoutEdge::new(b, 0, c, 0));

        let result = layout(&g, &LayoutConfig::default());

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
    fn passthrough_chain_sorted_as_single_unit() {
        let mut g = LayoutGraph::new();
        let i = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p1 = g.add_node(LayoutNode::new(1, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p2 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let o = g.add_node(LayoutNode::new(3, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i, 0, p1, 0));
        g.add_edge(LayoutEdge::new(p1, 0, p2, 0));
        g.add_edge(LayoutEdge::new(p2, 0, o, 0));

        let incoming = build_incoming(&g);
        let outgoing = build_outgoing(&g);
        let units = build_unit_layout(&g, &incoming, &outgoing);
        let (unit_pet, unit_ix_to_id, _) = build_unit_petgraph(&g, &units);
        let config = LayoutConfig::default();
        let unit_ranks = assign_unit_ranks(&unit_pet, &unit_ix_to_id, &g, &units, &config);

        assert_eq!(units.units.len(), 3, "in, param chain block, out");
        assert_eq!(unit_ranks.len(), 3);
        assert_eq!(unit_ranks.iter().copied().max(), Some(2), "three columns, not four");

        let result = layout(&g, &config);
        let p1_pos = result.positions[&p1];
        let p2_pos = result.positions[&p2];
        assert!(
            (p1_pos.x - p2_pos.x).abs() < 0.01,
            "column nodes share the same port X"
        );
        assert!(
            p1_pos.y < p2_pos.y,
            "column nodes stack vertically top to bottom"
        );
    }

    #[test]
    fn parallel_blocks_in_separate_columns() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p1 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p2 = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p3 = g.add_node(LayoutNode::new(4, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p4 = g.add_node(LayoutNode::new(5, (56.0, 22.0), NodeKind::Param, 1, 1));
        let c = g.add_node(LayoutNode::new(6, (40.0, 22.0), NodeKind::Combine, 2, 1));
        let o = g.add_node(LayoutNode::new(7, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i0, 0, p1, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p3, 0));
        g.add_edge(LayoutEdge::new(p1, 0, p2, 0));
        g.add_edge(LayoutEdge::new(p3, 0, p4, 0));
        g.add_edge(LayoutEdge::new(p2, 0, c, 0));
        g.add_edge(LayoutEdge::new(p4, 0, c, 1));
        g.add_edge(LayoutEdge::new(c, 0, o, 0));

        let result = layout(&g, &LayoutConfig::default());
        let block_a_x = result.positions[&p1].x;
        let block_b_x = result.positions[&p3].x;
        assert!(
            (block_a_x - block_b_x).abs() > 1.0,
            "parallel branches should be in separate columns"
        );
        assert_straight_wires(&g, &result);
    }

    #[test]
    fn layout_is_stable_across_repeated_calls() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p1 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p2 = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p3 = g.add_node(LayoutNode::new(4, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p4 = g.add_node(LayoutNode::new(5, (56.0, 22.0), NodeKind::Param, 1, 1));
        let c = g.add_node(LayoutNode::new(6, (40.0, 22.0), NodeKind::Combine, 2, 1));
        let o = g.add_node(LayoutNode::new(7, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i0, 0, p1, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p3, 0));
        g.add_edge(LayoutEdge::new(p1, 0, p2, 0));
        g.add_edge(LayoutEdge::new(p3, 0, p4, 0));
        g.add_edge(LayoutEdge::new(p2, 0, c, 0));
        g.add_edge(LayoutEdge::new(p4, 0, c, 1));
        g.add_edge(LayoutEdge::new(c, 0, o, 0));

        let config = LayoutConfig::default();
        let first = layout(&g, &config);
        for _ in 0..20 {
            let next = layout(&g, &config);
            assert_eq!(first.positions, next.positions);
        }
    }

    #[test]
    fn passthrough_and_parallel_blocks_have_minimum_gap() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p1 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p2 = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p3 = g.add_node(LayoutNode::new(4, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p4 = g.add_node(LayoutNode::new(5, (56.0, 22.0), NodeKind::Param, 1, 1));
        let c = g.add_node(LayoutNode::new(6, (40.0, 22.0), NodeKind::Combine, 2, 1));
        let o = g.add_node(LayoutNode::new(7, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i0, 0, p1, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p3, 0));
        g.add_edge(LayoutEdge::new(p1, 0, p2, 0));
        g.add_edge(LayoutEdge::new(p3, 0, p4, 0));
        g.add_edge(LayoutEdge::new(p2, 0, c, 0));
        g.add_edge(LayoutEdge::new(p4, 0, c, 1));
        g.add_edge(LayoutEdge::new(c, 0, o, 0));

        let result = layout(&g, &LayoutConfig::default());
        assert_straight_wires(&g, &result);
    }

    #[test]
    fn combine_dual_inlet_aligns_with_each_parent() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p0 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p1 = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        let combine = g.add_node(LayoutNode::new(4, (40.0, 22.0), NodeKind::Combine, 2, 1));
        let o = g.add_node(LayoutNode::new(5, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i0, 0, p0, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p1, 0));
        g.add_edge(LayoutEdge::new(p0, 0, combine, 0));
        g.add_edge(LayoutEdge::new(p1, 0, combine, 1));
        g.add_edge(LayoutEdge::new(combine, 0, o, 0));

        let incoming = build_incoming(&g);
        let outgoing = build_outgoing(&g);
        let units = build_unit_layout(&g, &incoming, &outgoing);

        assert!(units.unit(combine).is_dual_inlet());
        assert!(!units.unit(combine).is_block());

        let result = layout(&g, &LayoutConfig::default());
        let combine_node = g.node(combine).unwrap();
        let combine_pos = result.positions[&combine];
        let combine_w = result
            .sizes
            .get(&combine)
            .map(|(w, _)| *w)
            .unwrap_or(combine_node.size.0);

        for edge in g.edges().iter().filter(|e| e.to == combine) {
            let from = g.node(edge.from).unwrap();
            let from_pos = result.positions[&edge.from];
            let from_w = effective_node_width(&g, edge.from, Some(&result.sizes));
            let out_x = outlet_world_x(from_pos.x, from_w, edge.from_port, from.outlets);
            let in_x = inlet_world_x(
                combine_pos.x,
                combine_w,
                edge.to_port,
                combine_node.inlets,
            );
            assert!(
                (out_x - in_x).abs() < 0.01,
                "combine inlet {} should align with parent {} outlet (out={out_x}, in={in_x})",
                edge.to_port,
                edge.from
            );
        }
    }

    #[test]
    fn combine_stretches_between_parent_outlets() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let p0 = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let p1 = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        let combine = g.add_node(LayoutNode::new(4, (48.0, 22.0), NodeKind::Combine, 2, 1));
        let o = g.add_node(LayoutNode::new(5, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i0, 0, p0, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p1, 0));
        g.add_edge(LayoutEdge::new(p0, 0, combine, 0));
        g.add_edge(LayoutEdge::new(p1, 0, combine, 1));
        g.add_edge(LayoutEdge::new(combine, 0, o, 0));

        let result = layout(&g, &LayoutConfig::default());
        let default_w = g.node(combine).unwrap().size.0;
        let stretched_w = result
            .sizes
            .get(&combine)
            .map(|(w, _)| *w)
            .expect("combine should be resized");
        assert!(
            stretched_w > default_w + 20.0,
            "combine should stretch wider than default ({stretched_w} vs {default_w})"
        );
    }

    #[test]
    fn in_feeds_combine_second_inlet_directly_above() {
        let mut g = LayoutGraph::new();
        let add = |g: &mut LayoutGraph, id, kind, inlets, outlets, hex| {
            let mut node = LayoutNode::new(id, (56.0, 28.0), kind, inlets, outlets);
            if let Some(h) = hex {
                node = node.with_delay_pair(h);
            }
            g.add_node(node)
        };

        let i2 = add(&mut g, 2, NodeKind::Source, 0, 1, None);
        let p3 = add(&mut g, 3, NodeKind::Param, 1, 1, None);
        let p4 = add(&mut g, 4, NodeKind::Param, 1, 1, None);
        let mul = add(&mut g, 5, NodeKind::Default, 1, 1, None);
        let o0 = add(&mut g, 6, NodeKind::Sink, 1, 0, None);
        let o1 = add(&mut g, 7, NodeKind::Sink, 1, 0, None);
        let c8 = add(&mut g, 8, NodeKind::Combine, 2, 1, None);
        let c9 = add(&mut g, 9, NodeKind::Combine, 2, 1, None);
        let c13 = add(&mut g, 13, NodeKind::Combine, 2, 1, None);
        let _send10 = add(&mut g, 10, NodeKind::Send, 1, 0, Some(0x6F));
        let recv11 = add(&mut g, 11, NodeKind::Receive, 0, 1, Some(0x6F));
        let recv12 = add(&mut g, 12, NodeKind::Receive, 0, 1, Some(0x6F));
        let _send14 = add(&mut g, 14, NodeKind::Send, 1, 0, Some(0xD3));
        let recv15 = add(&mut g, 15, NodeKind::Receive, 0, 1, Some(0xD3));
        let recv16 = add(&mut g, 16, NodeKind::Receive, 0, 1, Some(0xD3));

        g.add_edge(LayoutEdge::new(p3, 0, o0, 0));
        g.add_edge(LayoutEdge::new(p4, 0, c8, 0));
        g.add_edge(LayoutEdge::new(i2, 0, c8, 1));
        g.add_edge(LayoutEdge::new(c8, 0, mul, 0));
        g.add_edge(LayoutEdge::new(mul, 0, c13, 0));
        g.add_edge(LayoutEdge::new(recv11, 0, p3, 0));
        g.add_edge(LayoutEdge::new(recv12, 0, c9, 1));
        g.add_edge(LayoutEdge::new(c9, 0, o1, 0));
        g.add_edge(LayoutEdge::new(recv15, 0, p4, 0));
        g.add_edge(LayoutEdge::new(recv16, 0, c13, 1));
        g.add_edge(LayoutEdge::new(c13, 0, c9, 0));

        let config = LayoutConfig::default();
        let result = layout(&g, &config);
        let missing: Vec<_> = g
            .sorted_node_ids()
            .into_iter()
            .filter(|id| !result.positions.contains_key(id))
            .collect();
        assert!(missing.is_empty(), "nodes missing from layout: {missing:?}");
        assert_straight_wires(&g, &result);

        let i2_pos = result.positions[&i2];
        let c8_pos = result.positions[&c8];
        let c8_w = result
            .sizes
            .get(&c8)
            .map(|(w, _)| *w)
            .unwrap_or(g.node(c8).unwrap().size.0);
        let i2_w = effective_node_width(&g, i2, Some(&result.sizes));
        let i2_out = outlet_world_x(i2_pos.x, i2_w, 0, 1);
        let c8_in1 = inlet_world_x(c8_pos.x, c8_w, 1, 2);
        assert!(
            (i2_out - c8_in1).abs() < 0.01,
            "in should align with combine inlet 1 (out={i2_out}, in={c8_in1})"
        );
        assert!(
            i2_pos.y < c8_pos.y,
            "in should sit above the combine it feeds"
        );
        let expected_y =
            config.snap(c8_pos.y - config.row_gap() - node_layout_height(&g, i2));
        assert!(
            (i2_pos.y - expected_y).abs() < 0.01,
            "in should be one row above combine (got {}, expected {expected_y})",
            i2_pos.y
        );
    }

    #[test]
    fn editor_demo_patch_with_send_combine_fanout() {
        let mut g = LayoutGraph::new();
        let add = |g: &mut LayoutGraph, id, kind, inlets, outlets, hex| {
            let mut node = LayoutNode::new(id, (56.0, 28.0), kind, inlets, outlets);
            if let Some(h) = hex {
                node = node.with_delay_pair(h);
            }
            g.add_node(node)
        };

        let i0 = add(&mut g, 0, NodeKind::Source, 0, 1, None);
        let i1 = add(&mut g, 1, NodeKind::Source, 0, 1, None);
        let i2 = add(&mut g, 2, NodeKind::Source, 0, 1, None);
        let p0 = add(&mut g, 3, NodeKind::Param, 1, 1, None);
        let p1 = add(&mut g, 4, NodeKind::Param, 1, 1, None);
        let mul = add(&mut g, 5, NodeKind::Default, 1, 1, None);
        let o0 = add(&mut g, 6, NodeKind::Sink, 1, 0, None);
        let o1 = add(&mut g, 7, NodeKind::Sink, 1, 0, None);
        let c0 = add(&mut g, 8, NodeKind::Combine, 2, 1, None);
        let c1 = add(&mut g, 9, NodeKind::Combine, 2, 1, None);
        let send = add(&mut g, 10, NodeKind::Send, 1, 0, Some(0x08));
        let recv_a = add(&mut g, 11, NodeKind::Receive, 0, 1, Some(0x08));
        let recv_b = add(&mut g, 12, NodeKind::Receive, 0, 1, Some(0x08));

        g.add_edge(LayoutEdge::new(i0, 0, p0, 0));
        g.add_edge(LayoutEdge::new(p0, 0, o0, 0));
        g.add_edge(LayoutEdge::new(i1, 0, p1, 0));
        g.add_edge(LayoutEdge::new(p1, 0, send, 0));
        g.add_edge(LayoutEdge::new(i2, 0, c0, 1));
        g.add_edge(LayoutEdge::new(c0, 0, mul, 0));
        g.add_edge(LayoutEdge::new(mul, 0, c1, 0));
        g.add_edge(LayoutEdge::new(recv_a, 0, c0, 0));
        g.add_edge(LayoutEdge::new(recv_b, 0, c1, 1));
        g.add_edge(LayoutEdge::new(c1, 0, o1, 0));

        let result = layout(&g, &LayoutConfig::default());

        for id in g.sorted_node_ids() {
            assert!(
                result.positions.contains_key(&id),
                "node {id} should have a layout position"
            );
        }
        assert_straight_wires(&g, &result);
    }

    #[test]
    fn send_receive_fanout_branches_are_sorted() {
        let mut g = LayoutGraph::new();
        let src = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let send = g.add_node(
            LayoutNode::new(1, (56.0, 22.0), NodeKind::Send, 1, 0).with_delay_pair(0xAA),
        );
        let recv_a = g.add_node(
            LayoutNode::new(2, (56.0, 22.0), NodeKind::Receive, 0, 1).with_delay_pair(0xAA),
        );
        let recv_b = g.add_node(
            LayoutNode::new(3, (56.0, 22.0), NodeKind::Receive, 0, 1).with_delay_pair(0xAA),
        );
        let tgt_a = g.add_node(LayoutNode::new(4, (40.0, 22.0), NodeKind::Default, 1, 1));
        let tgt_b = g.add_node(LayoutNode::new(5, (40.0, 22.0), NodeKind::Default, 1, 1));
        let out_a = g.add_node(LayoutNode::new(6, (48.0, 22.0), NodeKind::Sink, 1, 0));
        let out_b = g.add_node(LayoutNode::new(7, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(src, 0, send, 0));
        g.add_edge(LayoutEdge::new(recv_a, 0, tgt_a, 0));
        g.add_edge(LayoutEdge::new(recv_b, 0, tgt_b, 0));
        g.add_edge(LayoutEdge::new(tgt_a, 0, out_a, 0));
        g.add_edge(LayoutEdge::new(tgt_b, 0, out_b, 0));

        let result = layout(&g, &LayoutConfig::default());
        assert_straight_wires(&g, &result);

        assert!(
            result.positions[&recv_a].y < result.positions[&tgt_a].y,
            "receive should sit above its target"
        );
        assert!(
            result.positions[&recv_b].y < result.positions[&tgt_b].y,
            "receive should sit above its target"
        );
        assert!(
            result.positions[&tgt_a].x < result.positions[&tgt_b].x,
            "fan-out branches should be sorted left-to-right by id"
        );
        assert!(
            result.positions[&send].y > result.positions[&src].y,
            "send should be below its source on the main spine"
        );
    }

    #[test]
    fn disconnected_chains_in_separate_columns() {
        let mut g = LayoutGraph::new();
        // Chain A: 0 -> 1 -> 2
        let a0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let a1 = g.add_node(LayoutNode::new(1, (56.0, 22.0), NodeKind::Param, 1, 1));
        let a2 = g.add_node(LayoutNode::new(2, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(a0, 0, a1, 0));
        g.add_edge(LayoutEdge::new(a1, 0, a2, 0));

        // Chain B: 10 -> 11 -> 12 (no edges to chain A)
        let b0 = g.add_node(LayoutNode::new(10, (48.0, 22.0), NodeKind::Source, 0, 1));
        let b1 = g.add_node(LayoutNode::new(11, (56.0, 22.0), NodeKind::Param, 1, 1));
        let b2 = g.add_node(LayoutNode::new(12, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(b0, 0, b1, 0));
        g.add_edge(LayoutEdge::new(b1, 0, b2, 0));

        let config = LayoutConfig::default();
        let result = layout(&g, &config);

        assert_straight_wires(&g, &result);

        let chain_a_x = result.positions[&a0].x;
        let chain_b_x = result.positions[&b0].x;
        assert!(
            chain_a_x < chain_b_x,
            "lower-id chain should be left of higher-id chain"
        );

        let gap = chain_b_x - (result.positions[&a2].x + effective_node_width(&g, a2, Some(&result.sizes)));
        assert!(
            gap >= config.column_gap() - 0.01,
            "disconnected chains should be separated by at least column_gap ({gap})"
        );
    }

    #[test]
    fn layout_has_no_overlapping_nodes() {
        let mut g = LayoutGraph::new();
        let i0 = g.add_node(LayoutNode::new(0, (48.0, 22.0), NodeKind::Source, 0, 1));
        let i1 = g.add_node(LayoutNode::new(1, (48.0, 22.0), NodeKind::Source, 0, 1));
        let b = g.add_node(LayoutNode::new(2, (56.0, 22.0), NodeKind::Param, 1, 1));
        let c = g.add_node(LayoutNode::new(3, (56.0, 22.0), NodeKind::Param, 1, 1));
        let combine = g.add_node(LayoutNode::new(4, (40.0, 22.0), NodeKind::Combine, 2, 1));
        let d = g.add_node(LayoutNode::new(5, (48.0, 22.0), NodeKind::Sink, 1, 0));
        g.add_edge(LayoutEdge::new(i0, 0, b, 0));
        g.add_edge(LayoutEdge::new(i1, 0, c, 0));
        g.add_edge(LayoutEdge::new(b, 0, combine, 0));
        g.add_edge(LayoutEdge::new(c, 0, combine, 1));
        g.add_edge(LayoutEdge::new(combine, 0, d, 0));

        let result = layout(&g, &LayoutConfig::default());
        assert_straight_wires(&g, &result);
    }
}

fn assert_straight_wires(graph: &LayoutGraph, result: &LayoutResult) {
    use crate::ports::inlet_world_x;

    for edge in graph.edges() {
        let from = graph.node(edge.from).unwrap();
        let to = graph.node(edge.to).unwrap();
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
        let out_x = outlet_world_x(from_pos.x, from_w, edge.from_port, from.outlets.max(1));
        let in_x = inlet_world_x(to_pos.x, to_w, edge.to_port, to.inlets.max(1));
        assert!(
            (out_x - in_x).abs() < 0.01,
            "edge {}→{} should be a straight vertical wire (out={out_x}, in={in_x})",
            edge.from,
            edge.to
        );
    }
}
