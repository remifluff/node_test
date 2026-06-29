use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::visit::EdgeRef;

use crate::graph::{NodeId, PatchGraph};

pub fn find_cycle_nodes(graph: &PatchGraph, from_node: NodeId, to_node: NodeId) -> Vec<NodeId> {
    find_path_nodes(graph, to_node, from_node).unwrap_or_else(|| vec![from_node, to_node])
}

pub fn find_path_nodes(graph: &PatchGraph, start: NodeId, goal: NodeId) -> Option<Vec<NodeId>> {
    if start == goal {
        return Some(vec![start]);
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([start]);
    let mut parent = HashMap::new();
    visited.insert(start);

    while let Some(node) = queue.pop_front() {
        for edge in graph.edges(node) {
            let next = edge.target();
            if !visited.insert(next) {
                continue;
            }
            parent.insert(next, node);
            if next == goal {
                let mut path = vec![goal];
                let mut current = goal;
                while current != start {
                    current = parent[&current];
                    path.push(current);
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(next);
        }
    }

    None
}

pub fn cycle_vertical_bounds(graph: &PatchGraph, cycle_nodes: &[NodeId]) -> (f32, f32, f32) {
    let mut min_top = f32::INFINITY;
    let mut max_bottom = f32::NEG_INFINITY;
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;

    for &id in cycle_nodes {
        let node = &graph[id];
        min_top = min_top.min(node.pos.y);
        max_bottom = max_bottom.max(node.pos.y + node.size.y);
        min_x = min_x.min(node.pos.x);
        max_x = max_x.max(node.pos.x + node.size.x);
    }

    let center_x = (min_x + max_x) / 2.0;
    (min_top, max_bottom, center_x)
}
