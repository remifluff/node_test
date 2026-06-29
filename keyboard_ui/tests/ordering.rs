use egui::{pos2, vec2, Rect};
use patch_graph::{
    layout::LayoutNavCell, layout_adapter::LayoutPreview, node::Node, object::PdObject, PatchGraph,
};

fn make_graph() -> (
    patch_graph::NodeId,
    patch_graph::NodeId,
    patch_graph::NodeId,
    PatchGraph,
) {
    let mut graph = PatchGraph::default();
    let a = graph.add_node(Node {
        object: PdObject::In,
        label: "in".into(),
        pos: pos2(0.0, 0.0),
        size: vec2(64.0, 32.0),
        box_id: None,
        screen_rect: Rect::NOTHING,
        inlet_t: vec![],
        outlet_t: vec![],
        inlet_positions: vec![],
        outlet_positions: vec![],
        selected: false,
    });
    let b = graph.add_node(Node {
        object: PdObject::Param,
        label: "param".into(),
        pos: pos2(0.0, 0.0),
        size: vec2(64.0, 32.0),
        box_id: None,
        screen_rect: Rect::NOTHING,
        inlet_t: vec![],
        outlet_t: vec![],
        inlet_positions: vec![],
        outlet_positions: vec![],
        selected: false,
    });
    let c = graph.add_node(Node {
        object: PdObject::Out,
        label: "out".into(),
        pos: pos2(0.0, 0.0),
        size: vec2(64.0, 32.0),
        box_id: None,
        screen_rect: Rect::NOTHING,
        inlet_t: vec![],
        outlet_t: vec![],
        inlet_positions: vec![],
        outlet_positions: vec![],
        selected: false,
    });
    graph.add_edge(
        a,
        b,
        patch_graph::node::EdgeData {
            from_port: 0,
            to_port: 0,
            selected: false,
        },
    );
    graph.add_edge(
        b,
        c,
        patch_graph::node::EdgeData {
            from_port: 0,
            to_port: 0,
            selected: false,
        },
    );
    (a, b, c, graph)
}

#[test]
fn keyboard_paint_order_follows_preview_rows() {
    let (a, b, c, graph) = make_graph();
    let preview = LayoutPreview {
        positions: Default::default(),
        sizes: Default::default(),
        nav: [
            (a.index(), LayoutNavCell { row: 1, slot: 1 }),
            (b.index(), LayoutNavCell { row: 0, slot: 0 }),
            (c.index(), LayoutNavCell { row: 1, slot: 0 }),
        ]
        .into_iter()
        .collect(),
        rows: vec![vec![b.index()], vec![a.index(), c.index()]],
    };

    let order = keyboard_ui::draw::node_order_for_paint(&graph, &preview);

    assert_eq!(order, vec![b, a, c]);
}
