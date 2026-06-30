use eframe::egui::{self, RichText, Ui};
use mouse_ui::style::{INK, PAPER_DIM};
use mouse_ui::{PatchCanvas, OperatorLibrary};
use patch_graph::{export_patch, PatchGraph, PdObject};

pub struct PdPatchEditor {
    graph: PatchGraph,
    canvas: PatchCanvas,
    patch_name: String,
}

impl PdPatchEditor {
    fn format_patch_lop(&self) -> String {
        export_patch(&self.graph, &self.patch_name)
    }

    pub fn demo_patch() -> Self {
        let mut editor = Self {
            graph: PatchGraph::default(),
            canvas: PatchCanvas::default(),
            patch_name: "patch".to_owned(),
        };

        let in0 = editor.canvas.add_object(&mut editor.graph, PdObject::In, egui::pos2(80.0, 80.0));
        let in1 = editor.canvas.add_object(&mut editor.graph, PdObject::In, egui::pos2(80.0, 180.0));
        let in2 = editor.canvas.add_object(&mut editor.graph, PdObject::In, egui::pos2(80.0, 280.0));

        let param0 = editor.canvas.add_object(&mut editor.graph, PdObject::Param, egui::pos2(200.0, 80.0));
        let param1 = editor.canvas.add_object(&mut editor.graph, PdObject::Param, egui::pos2(200.0, 180.0));

        let mul = editor
            .canvas
            .add_object(&mut editor.graph, PdObject::MulTilde, egui::pos2(320.0, 230.0));

        let out0 = editor.canvas.add_object(&mut editor.graph, PdObject::Out, egui::pos2(440.0, 80.0));
        let out1 = editor.canvas.add_object(&mut editor.graph, PdObject::Out, egui::pos2(440.0, 230.0));

        editor
            .canvas
            .connect_ports_unchecked(&mut editor.graph, in0, 0, param0, 0);
        editor
            .canvas
            .connect_ports_unchecked(&mut editor.graph, param0, 0, out0, 0);
        editor
            .canvas
            .connect_ports_unchecked(&mut editor.graph, in1, 0, param1, 0);
        editor
            .canvas
            .connect_ports_unchecked(&mut editor.graph, param1, 0, mul, 0);
        editor
            .canvas
            .connect_ports_unchecked(&mut editor.graph, in2, 0, mul, 0);
        editor
            .canvas
            .connect_ports_unchecked(&mut editor.graph, mul, 0, out1, 0);

        editor.canvas.clear_undo_history();
        editor
    }
}

impl eframe::App for PdPatchEditor {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reset demo").clicked() {
                        *self = Self::demo_patch();
                    }
                    ui.menu_button("Wire debug", |ui| {
                        ui.checkbox(&mut self.canvas.debug_auto_combine, "Auto combine");
                        ui.checkbox(
                            &mut self.canvas.debug_auto_send_receive,
                            "Auto send/receive",
                        );
                        ui.checkbox(&mut self.canvas.debug_auto_delay, "Auto delay");
                    });
                    if ui.button("Sort").clicked() {
                        self.canvas.sort_layout(&mut self.graph);
                    }
                    if ui.button("Reload operators").clicked() {
                        self.canvas.operator_library = OperatorLibrary::load_preferred();
                    }
                    if let Some(path) = self.canvas.operator_library.source() {
                        ui.label(
                            RichText::new(format!("ops: {}", path.display()))
                                .small()
                                .color(PAPER_DIM),
                        );
                    }
                    if ui.button("Copy patch").clicked() {
                        ui.ctx().copy_text(self.format_patch_lop());
                    }
                });
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: INK,
                inner_margin: egui::Margin::ZERO,
                ..Default::default()
            })
            .show_inside(ui, |ui| {
                self.canvas.ui(ui, &mut self.graph, true);
            });
    }
}

#[cfg(test)]
mod shift_drag_tests {
    use super::*;
    use patch_graph::{EdgeData, NodeId, PdObject};
    use eframe::egui::{pos2, Rect, emath::TSTransform};

    struct TestEditor {
        graph: PatchGraph,
        canvas: PatchCanvas,
    }

    impl TestEditor {
        fn new() -> Self {
            Self {
                graph: PatchGraph::default(),
                canvas: PatchCanvas::default(),
            }
        }

        fn add_object(&mut self, object: PdObject, pos: eframe::egui::Pos2) -> NodeId {
            self.canvas.add_object(&mut self.graph, object, pos)
        }

        fn connect_ports_unchecked(
            &mut self,
            from_node: NodeId,
            from_out: usize,
            to_node: NodeId,
            to_in: usize,
        ) {
            self.canvas
                .connect_ports_unchecked(&mut self.graph, from_node, from_out, to_node, to_in);
        }

        fn connect_ports(
            &mut self,
            from_node: NodeId,
            from_out: usize,
            to_node: NodeId,
            to_in: usize,
        ) {
            self.canvas
                .connect_ports(&mut self.graph, from_node, from_out, to_node, to_in);
        }

        fn shift_drag_bridge_nodes(&mut self, node_ids: &[NodeId]) {
            self.canvas.shift_drag_bridge_nodes(&mut self.graph, node_ids);
        }

        fn insert_node_on_edge(&mut self, node_id: NodeId, edge_id: patch_graph::EdgeId) -> bool {
            self.canvas
                .insert_node_on_edge(&mut self.graph, node_id, edge_id)
        }

        fn try_shift_drag_insert_on_wire(
            &mut self,
            node_id: NodeId,
            pointer: eframe::egui::Pos2,
            node_world_rect: Rect,
            transform: TSTransform,
        ) {
            self.canvas.try_shift_drag_insert_on_wire(
                &mut self.graph,
                node_id,
                pointer,
                node_world_rect,
                transform,
            );
        }

        fn node_shift_drag_eligible(&mut self, node_id: NodeId) -> bool {
            self.canvas.node_shift_drag_eligible(&mut self.graph, node_id)
        }

        fn node_shift_drag_insert_eligible(&mut self, node_id: NodeId) -> bool {
            self.canvas
                .node_shift_drag_insert_eligible(&mut self.graph, node_id)
        }
    }

    fn chain_editor() -> TestEditor {
        let mut editor = TestEditor::new();
        let a = editor.add_object(PdObject::In, pos2(0.0, 0.0));
        let b = editor.add_object(PdObject::MulTilde, pos2(100.0, 0.0));
        let c = editor.add_object(PdObject::Out, pos2(200.0, 0.0));
        editor.connect_ports_unchecked(a, 0, b, 0);
        editor.connect_ports_unchecked(b, 0, c, 0);
        editor
    }

    fn edge_count(editor: &TestEditor) -> usize {
        editor.graph.edge_indices().count()
    }

    fn sole_edge(editor: &TestEditor) -> (NodeId, usize, NodeId, usize) {
        let edge_id = editor.graph.edge_indices().next().expect("edge");
        let (from, to) = editor.graph.edge_endpoints(edge_id).expect("endpoints");
        let edge = &editor.graph[edge_id];
        (from, edge.from_port, to, edge.to_port)
    }

    #[test]
    fn shift_drag_bridge_reconnects_neighbors() {
        let mut editor = chain_editor();
        let middle = editor
            .graph
            .node_indices()
            .find(|&id| matches!(editor.graph[id].object, PdObject::MulTilde))
            .expect("middle");

        editor.shift_drag_bridge_nodes(&[middle]);

        assert_eq!(edge_count(&editor), 1);
        let (from, _, to, _) = sole_edge(&editor);
        assert!(matches!(editor.graph[from].object, PdObject::In));
        assert!(matches!(editor.graph[to].object, PdObject::Out));
    }

    #[test]
    fn shift_drag_insert_splits_edge() {
        let mut editor = TestEditor::new();
        let a = editor.add_object(PdObject::In, pos2(0.0, 0.0));
        let c = editor.add_object(PdObject::Out, pos2(200.0, 0.0));
        let edge_id = editor.graph.add_edge(
            a,
            c,
            EdgeData {
                from_port: 0,
                to_port: 0,
                selected: false,
            },
        );
        let insert = editor.add_object(PdObject::MulTilde, pos2(100.0, 0.0));

        assert!(editor.insert_node_on_edge(insert, edge_id));
        assert_eq!(edge_count(&editor), 2);

        let mut found = false;
        for edge_id in editor.graph.edge_indices() {
            let (from, to) = editor.graph.edge_endpoints(edge_id).expect("endpoints");
            if from == insert {
                found = true;
                assert!(matches!(editor.graph[to].object, PdObject::Out));
            }
            if to == insert {
                assert!(matches!(editor.graph[from].object, PdObject::In));
            }
        }
        assert!(found);
    }

    #[test]
    fn shift_drag_insert_requires_eligibility_from_drag_start() {
        let mut editor = chain_editor();
        let middle = editor
            .graph
            .node_indices()
            .find(|&id| matches!(editor.graph[id].object, PdObject::MulTilde))
            .expect("middle");

        editor.shift_drag_bridge_nodes(&[middle]);
        let edge_id = editor.graph.edge_indices().next().expect("bridged edge");
        let (from, to) = editor.graph.edge_endpoints(edge_id).expect("endpoints");
        let from_out = editor.graph[from].outlet_positions[0];
        let to_in = editor.graph[to].inlet_positions[0];
        let wire_mid = from_out.lerp(to_in, 0.5);
        let node_rect = Rect::from_center_size(wire_mid, editor.graph[middle].size);
        let transform = TSTransform::IDENTITY;

        editor.try_shift_drag_insert_on_wire(middle, wire_mid, node_rect, transform);
        assert_eq!(edge_count(&editor), 1);

        assert!(editor.node_shift_drag_insert_eligible(middle));
        editor.canvas.mark_shift_drag_eligible(middle);
        editor.try_shift_drag_insert_on_wire(middle, wire_mid, node_rect, transform);
        assert_eq!(edge_count(&editor), 2);
    }

    #[test]
    fn shift_drag_insert_works_for_disconnected_splicable_node() {
        let mut editor = TestEditor::new();
        let a = editor.add_object(PdObject::In, pos2(0.0, 0.0));
        let c = editor.add_object(PdObject::Out, pos2(200.0, 0.0));
        let edge_id = editor.graph.add_edge(
            a,
            c,
            EdgeData {
                from_port: 0,
                to_port: 0,
                selected: false,
            },
        );
        let insert = editor.add_object(PdObject::MulTilde, pos2(100.0, 50.0));
        assert!(!editor.node_shift_drag_eligible(insert));
        assert!(editor.node_shift_drag_insert_eligible(insert));

        editor.canvas.mark_shift_drag_eligible(insert);
        assert!(editor.insert_node_on_edge(insert, edge_id));
        assert_eq!(edge_count(&editor), 2);
    }

    #[test]
    fn shift_drag_insert_rejects_comment() {
        let mut editor = TestEditor::new();
        let a = editor.add_object(PdObject::In, pos2(0.0, 0.0));
        let c = editor.add_object(PdObject::Out, pos2(200.0, 0.0));
        let edge_id = editor.graph.add_edge(
            a,
            c,
            EdgeData {
                from_port: 0,
                to_port: 0,
                selected: false,
            },
        );
        let comment = editor.add_object(PdObject::Comment { text: "x".into() }, pos2(100.0, 0.0));

        assert!(!editor.insert_node_on_edge(comment, edge_id));
        assert_eq!(edge_count(&editor), 1);
    }

    #[test]
    fn duplicate_port_pair_connection_is_ignored() {
        let mut editor = TestEditor::new();
        let a = editor.add_object(PdObject::In, pos2(0.0, 0.0));
        let b = editor.add_object(PdObject::Out, pos2(100.0, 0.0));
        editor.connect_ports_unchecked(a, 0, b, 0);

        editor.connect_ports(a, 0, b, 0);
        editor.connect_ports_unchecked(a, 0, b, 0);

        assert_eq!(edge_count(&editor), 1);
    }

    #[test]
    fn different_ports_on_same_nodes_can_still_connect() {
        let mut editor = TestEditor::new();
        let combine = editor.add_object(PdObject::Combine, pos2(100.0, 0.0));
        let a = editor.add_object(PdObject::In, pos2(0.0, -30.0));
        let b = editor.add_object(PdObject::In, pos2(0.0, 30.0));
        editor.connect_ports_unchecked(a, 0, combine, 0);
        editor.connect_ports_unchecked(b, 0, combine, 1);

        assert_eq!(edge_count(&editor), 2);
    }
}
