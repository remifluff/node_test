//! Navigation grid derived from final layout positions.

use std::collections::HashMap;

use crate::layout::layout_graph::{LayoutNavCell, LayoutNodeId, Point};

/// Build row/slot indices for keyboard navigation from laid-out positions.
pub fn build_nav_grid(
    positions: &HashMap<LayoutNodeId, Point>,
) -> (HashMap<LayoutNodeId, LayoutNavCell>, Vec<Vec<LayoutNodeId>>) {
    if positions.is_empty() {
        return (HashMap::new(), Vec::new());
    }

    let row_ys = extract_row_ys(positions);
    let mut rows: Vec<Vec<LayoutNodeId>> = vec![Vec::new(); row_ys.len()];
    let mut nav = HashMap::with_capacity(positions.len());

    for &id in positions.keys() {
        let pos = positions[&id];
        let row = row_index(pos.y, &row_ys);
        rows[row].push(id);
    }

    for (row_idx, row_nodes) in rows.iter_mut().enumerate() {
        row_nodes.sort_by(|&a, &b| {
            positions[&a]
                .x
                .partial_cmp(&positions[&b].x)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(&b))
        });
        for (slot, &id) in row_nodes.iter().enumerate() {
            nav.insert(
                id,
                LayoutNavCell {
                    row: row_idx as u32,
                    slot: slot as u32,
                },
            );
        }
    }

    (nav, rows)
}

fn extract_row_ys(positions: &HashMap<LayoutNodeId, Point>) -> Vec<f32> {
    let mut ys: Vec<f32> = positions.values().map(|p| p.y).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut row_ys: Vec<f32> = Vec::new();
    for y in ys {
        let is_new_row = match row_ys.last() {
            None => true,
            Some(&last) => (y - last).abs() > 0.5,
        };
        if is_new_row {
            row_ys.push(y);
        }
    }
    row_ys
}

fn row_index(y: f32, row_ys: &[f32]) -> usize {
    row_ys
        .iter()
        .position(|&row_y| (y - row_y).abs() <= 0.5)
        .unwrap_or(row_ys.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout_graph::{LayoutEdge, LayoutGraph, LayoutNode, NodeKind};

    #[test]
    fn parallel_inputs_share_a_row() {
        let mut g = LayoutGraph::new();
        let in0 = g.add_node(LayoutNode::new(0, (48.0, 20.0), NodeKind::Source, 0, 1));
        let in1 = g.add_node(LayoutNode::new(1, (48.0, 20.0), NodeKind::Source, 0, 1));
        let mul = g.add_node(LayoutNode::new(2, (40.0, 20.0), NodeKind::Default, 2, 1));
        g.add_edge(LayoutEdge::new(in0, 0, mul, 0));
        g.add_edge(LayoutEdge::new(in1, 0, mul, 1));

        let positions = HashMap::from([
            (0, Point { x: 0.0, y: 0.0 }),
            (1, Point { x: 60.0, y: 0.0 }),
            (2, Point { x: 30.0, y: 60.0 }),
        ]);

        let (nav, rows) = build_nav_grid(&positions);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec![0, 1]);
        assert_eq!(nav[&0].row, 0);
        assert_eq!(nav[&0].slot, 0);
        assert_eq!(nav[&1].slot, 1);
        assert_eq!(nav[&2].row, 1);
    }
}
