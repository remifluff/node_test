use std::collections::{HashMap, HashSet, VecDeque};

use emath::Vec2;
use petgraph::visit::EdgeRef;

use crate::graph::{NodeId, PatchGraph};
use crate::node::Node;
use crate::object::PdObject;

pub fn random_unused_delay_hex(used: &HashSet<u8>) -> u8 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);

    for _ in 0..256 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let id = (seed >> 16) as u8;
        if !used.contains(&id) {
            return id;
        }
    }

    (0..=255)
        .find(|id| !used.contains(id))
        .unwrap_or(0)
}

pub fn parse_delay_hex(token: Option<&str>) -> Option<u8> {
    let token = token?;
    let hex_str = token.strip_prefix('#').unwrap_or(token);
    u8::from_str_radix(hex_str, 16).ok()
}

fn find_cycle_nodes(graph: &PatchGraph, from_node: NodeId, to_node: NodeId) -> Vec<NodeId> {
    find_path_nodes(graph, to_node, from_node).unwrap_or_else(|| vec![from_node, to_node])
}

fn find_path_nodes(graph: &PatchGraph, start: NodeId, goal: NodeId) -> Option<Vec<NodeId>> {
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

fn cycle_vertical_bounds(graph: &PatchGraph, cycle_nodes: &[NodeId]) -> (f32, f32, f32) {
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

pub fn parse_pd_object_text(text: &str) -> PdObject {
    let stripped = crate::sizing::strip_brackets(text.trim());
    if stripped.is_empty() {
        return PdObject::Message {
            text: String::new(),
        };
    }

    let mut parts = stripped.split_whitespace();
    let op = parts.next().unwrap_or("");
    match op {
        "osc~" => PdObject::OscTilde {
            freq: parts
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(440.0),
        },
        "+~" => PdObject::PlusTilde,
        "*~" => PdObject::MulTilde,
        "dac~" => PdObject::DacTilde,
        "in" => PdObject::In,
        "param" => PdObject::Param,
        "out" => PdObject::Out,
        "delay_in" => PdObject::DelayIn {
            id: parse_delay_hex(parts.next()),
        },
        "delay_out" => PdObject::DelayOut {
            id: parse_delay_hex(parts.next()),
        },
        "send" => PdObject::Send {
            id: parse_delay_hex(parts.next()),
        },
        "receive" => PdObject::Receive {
            id: parse_delay_hex(parts.next()),
        },
        "combine" => PdObject::Combine,
        "metro" => PdObject::Metro {
            ms: parts.next().and_then(|s| s.parse().ok()).unwrap_or(500.0),
        },
        "random" => PdObject::Random {
            max: parts.next().and_then(|s| s.parse().ok()).unwrap_or(100),
        },
        _ if op.parse::<f32>().is_ok() && parts.next().is_none() => PdObject::FloatAtom {
            value: op.parse().unwrap_or(0.0),
        },
        _ => PdObject::Message {
            text: stripped.to_owned(),
        },
    }
}

pub fn commit_node_label(node: &mut Node, text: &str) {
    node.label = text.to_owned();
    if node.object.is_comment() {
        node.object = PdObject::Comment {
            text: text.to_owned(),
        };
    } else {
        node.object = object_from_label(text);
    }
    node.size = estimate_text_box_size(&node.label, &node.object);
}

/// Parse box text into a typed object when it round-trips; otherwise keep all text as a message.
pub fn object_from_label(text: &str) -> PdObject {
    if text.is_empty() {
        return PdObject::Message {
            text: String::new(),
        };
    }

    let parsed = parse_pd_object_text(text);
    if object_text_matches_label(&parsed, text) {
        parsed
    } else {
        PdObject::Message {
            text: text.to_owned(),
        }
    }
}

pub fn object_text_matches_label(object: &PdObject, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return matches!(object, PdObject::Message { text } if text.is_empty());
    }
    object.edit_text().trim() == trimmed
        || object.bracketed_label().trim() == trimmed
        || crate::sizing::strip_brackets(&object.bracketed_label()).trim() == crate::sizing::strip_brackets(trimmed).trim()
}

pub fn estimate_node_size(object: &PdObject) -> emath::Vec2 {
    estimate_text_box_size(&object.bracketed_label(), object)
}

/// Size needed to display `text` in a node box (used while editing).
pub fn estimate_text_box_size(text: &str, object: &PdObject) -> emath::Vec2 {
    let label = if text.is_empty() { "?" } else { text };
    let width = crate::sizing::min_box_width(label, object.inlets());
    let height = if object.is_comment() {
        let lines = label.lines().count().max(1);
        crate::sizing::BOX_H * 0.8 * lines as f32
    } else {
        crate::sizing::BOX_H
    };
    emath::vec2(width.max(48.0), height.max(if object.is_comment() { crate::sizing::BOX_H * 0.8 } else { crate::sizing::BOX_H }))
}
