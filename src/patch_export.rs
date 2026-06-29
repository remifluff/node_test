//! Export editor graph as fragment_interlay `(patch …)` s-expression notation.

use crate::graph::{EdgeData, Node, NodeId, PatchGraph, PdObject};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

const GRID: f64 = 15.0;

#[derive(Debug, Clone, PartialEq)]
pub struct PatchView {
    pub grid: f64,
    pub snap: bool,
    pub rect: [f64; 4],
}

impl Default for PatchView {
    fn default() -> Self {
        Self {
            grid: GRID,
            snap: false,
            rect: [0.0, 0.0, 640.0, 480.0],
        }
    }
}

/// Round patch layout numbers to two decimal places (matches fi-compiler `format_coord`).
pub fn format_coord(v: f64) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    if rounded.fract().abs() < 1e-9 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.2}")
    }
}

fn format_coord_pair(pair: [f64; 2]) -> String {
    format!("{} {}", format_coord(pair[0]), format_coord(pair[1]))
}

pub fn quote_lop_string(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn box_id_number(box_id: &str) -> Option<u64> {
    box_id.strip_prefix("obj-")?.parse().ok()
}

fn node_box_id(node: &Node, node_id: NodeId) -> String {
    node.box_id
        .clone()
        .unwrap_or_else(|| format!("obj-{}", node_id.index() + 1))
}

fn sorted_node_ids(graph: &PatchGraph) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = graph.node_indices().collect();
    ids.sort_by(|a, b| {
        let a_id = node_box_id(&graph[*a], *a);
        let b_id = node_box_id(&graph[*b], *b);
        match (box_id_number(&a_id), box_id_number(&b_id)) {
            (Some(a_n), Some(b_n)) => a_n.cmp(&b_n),
            _ => a_id.cmp(&b_id),
        }
    });
    ids
}

fn io_indices(graph: &PatchGraph, node_ids: &[NodeId]) -> HashMap<NodeId, usize> {
    let mut out = HashMap::new();
    let mut next_in = 1usize;
    let mut next_out = 1usize;
    let mut next_param = 1usize;

    for node_id in node_ids {
        let index = match &graph[*node_id].object {
            PdObject::In => {
                let i = next_in;
                next_in += 1;
                Some(i)
            }
            PdObject::Out => {
                let i = next_out;
                next_out += 1;
                Some(i)
            }
            PdObject::Param => {
                let i = next_param;
                next_param += 1;
                Some(i)
            }
            _ => None,
        };
        if let Some(i) = index {
            out.insert(*node_id, i);
        }
    }
    out
}

pub fn patch_view_from_graph(graph: &PatchGraph) -> PatchView {
    let mut view = PatchView::default();
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut any = false;

    for node_id in graph.node_indices() {
        let node = &graph[node_id];
        if node.object.is_comment() {
            continue;
        }
        any = true;
        min_x = min_x.min(f64::from(node.pos.x));
        min_y = min_y.min(f64::from(node.pos.y));
        max_x = max_x.max(f64::from(node.pos.x + node.size.x));
        max_y = max_y.max(f64::from(node.pos.y + node.size.y));
    }

    if any {
        let pad = 40.0;
        view.rect = [
            (min_x - pad).max(0.0),
            (min_y - pad).max(0.0),
            (max_x - min_x + pad * 2.0).max(200.0),
            (max_y - min_y + pad * 2.0).max(200.0),
        ];
    }

    view
}

fn format_view(view: &PatchView) -> String {
    format!(
        "(view :grid {} :snap {} :rect {} {} {} {})",
        format_coord(view.grid),
        if view.snap { "1" } else { "0" },
        format_coord(view.rect[0]),
        format_coord(view.rect[1]),
        format_coord(view.rect[2]),
        format_coord(view.rect[3]),
    )
}

fn format_node(node: &Node, node_id: NodeId, io_index: Option<usize>) -> String {
    let id = node_box_id(node, node_id);
    let text = node.object.lop_text(io_index);
    let at = [f64::from(node.pos.x), f64::from(node.pos.y)];
    let size = [f64::from(node.size.x), f64::from(node.size.y)];
    let mut line = format!(
        "(node {} :text {} :at {} :size {} :ports {} {}",
        id,
        quote_lop_string(&text),
        format_coord_pair(at),
        format_coord_pair(size),
        node.object.inlets(),
        node.object.outlets(),
    );
    if let Some(bind) = node.object.lop_bind(io_index) {
        line.push_str(" :bind ");
        line.push_str(&bind);
    }
    line.push(')');
    line
}

fn format_wire(
    wire_index: usize,
    from_id: &str,
    from_port: usize,
    to_id: &str,
    to_port: usize,
) -> String {
    format!(
        "(wire w{} :from ({} {}) :to ({} {}))",
        wire_index,
        from_id,
        from_port,
        to_id,
        to_port,
    )
}

/// Serialize the patch graph as a `(patch …)` block matching fragment_interlay `.lop` layout.
pub fn export_patch(graph: &PatchGraph, name: &str) -> String {
    let view = patch_view_from_graph(graph);
    let node_ids = sorted_node_ids(graph);
    let io_map = io_indices(graph, &node_ids);

    let mut out = String::from("(patch\n  ");
    out.push_str(name);
    out.push('\n');
    out.push_str("  ");
    out.push_str(&format_view(&view));
    out.push('\n');

    for node_id in &node_ids {
        let node = &graph[*node_id];
        if node.object.is_comment() {
            continue;
        }
        out.push_str("  ");
        out.push_str(&format_node(
            node,
            *node_id,
            io_map.get(node_id).copied(),
        ));
        out.push('\n');
    }

    let mut edge_ids: Vec<_> = graph.edge_indices().collect();
    edge_ids.sort_by_key(|id| id.index());

    for (wire_index, edge_id) in edge_ids.into_iter().enumerate() {
        let Some((from, to)) = graph.edge_endpoints(edge_id) else {
            continue;
        };
        let EdgeData {
            from_port,
            to_port,
            ..
        } = graph[edge_id].clone();
        out.push_str("  ");
        out.push_str(&format_wire(
            wire_index + 1,
            &node_box_id(&graph[from], from),
            from_port,
            &node_box_id(&graph[to], to),
            to_port,
        ));
        out.push('\n');
    }

    out.push(')');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::PdObject;
    use eframe::egui::pos2;

    #[test]
    fn format_coord_matches_interlay_rules() {
        assert_eq!(format_coord(22.0), "22");
        assert_eq!(format_coord(130.304), "130.30");
        assert_eq!(format_coord(-138.55), "-138.55");
    }

    #[test]
    fn export_emits_patch_structure() {
        let mut graph = PatchGraph::default();
        let in_id = graph.add_node(Node {
            object: PdObject::In,
            pos: pos2(100.0, 40.0),
            size: eframe::egui::vec2(30.0, 22.0),
            box_id: Some("obj-1".into()),
            screen_rect: eframe::egui::Rect::NOTHING,
            inlet_t: vec![],
            outlet_t: vec![0.5],
            inlet_positions: vec![],
            outlet_positions: vec![eframe::egui::Pos2::ZERO],
            selected: false,
        });
        let out_id = graph.add_node(Node {
            object: PdObject::Out,
            pos: pos2(100.0, 110.0),
            size: eframe::egui::vec2(30.0, 22.0),
            box_id: Some("obj-2".into()),
            screen_rect: eframe::egui::Rect::NOTHING,
            inlet_t: vec![0.5],
            outlet_t: vec![],
            inlet_positions: vec![eframe::egui::Pos2::ZERO],
            outlet_positions: vec![],
            selected: false,
        });
        graph.add_edge(
            in_id,
            out_id,
            EdgeData {
                from_port: 0,
                to_port: 0,
                selected: false,
            },
        );

        let text = export_patch(&graph, "circle");
        assert!(text.starts_with("(patch\n  circle\n"), "got:\n{text}");
        assert!(
            text.contains("(node obj-1 :text \"in 1\" :at 100 40 :size 30 22 :ports 0 1 :bind _in_1)"),
            "got:\n{text}"
        );
        assert!(text.contains(
            "(node obj-2 :text \"out 1\" :at 100 110 :size 30 22 :ports 1 0 :bind _out_1)"
        ));
        assert!(text.contains("(wire w1 :from (obj-1 0) :to (obj-2 0))"));
        assert!(text.ends_with(')'));
    }
}
