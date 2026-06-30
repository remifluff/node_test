use egui::{
    self, emath::TSTransform, pos2, vec2, Area, Color32, Context, CornerRadius, CursorIcon,
    Frame, Id, Key, LayerId, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use egui::{epaint::CubicBezierShape, epaint::Shape, pos2 as egui_pos2};
use patch_graph::{
    cycle_vertical_bounds, find_cycle_nodes, find_path_nodes,
    EdgeData, EdgeId, Node, NodeId, PatchGraph, PdObject,
};
use patch_graph::parse::{
    commit_node_label, estimate_node_size, estimate_text_box_size, random_unused_delay_hex,
};

use std::collections::{HashMap, HashSet};

use crate::canvas::{scene_layer_id, scene_transform, show_patch_scene, CanvasView};
use crate::object_ui::{self, NodeAreaBody};
use crate::style::{
    self, default_port_ts, paint_node_hover_highlight, paint_port_square,
    port_position_t, PortHighlight, BOX_H, CABLE_STROKE, GRID_STEP,
    LINE_W, PAPER, PAPER_DIM, port_size, WIRE_HANDLE, WIRE_HANDLE_HOVER,
};
use crate::operator_library::OperatorLibrary;
use crate::sort::sort_patch;

#[derive(Default)]
pub struct PatchCanvasResponse {}

#[derive(Clone, Debug)]
struct PatchSnapshot {
    graph: PatchGraph,
    delay_pairs: HashMap<NodeId, NodeId>,
    next_box_id: u64,
}

impl PatchCanvas {
    pub fn ui(&mut self, ui: &mut Ui, graph: &mut PatchGraph, editable: bool) -> PatchCanvasResponse {
        CanvasSession { canvas: self, graph }.mouse_ui(ui, editable);
        PatchCanvasResponse::default()
    }

    pub fn sort_layout(&mut self, graph: &mut PatchGraph) {
        CanvasSession { canvas: self, graph }.sort_layout();
    }

    pub fn add_object(&mut self, graph: &mut PatchGraph, object: PdObject, pos: Pos2) -> NodeId {
        CanvasSession { canvas: self, graph }.add_object(object, pos)
    }

    pub fn connect_ports_unchecked(
        &mut self,
        graph: &mut PatchGraph,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
    ) {
        CanvasSession { canvas: self, graph }.connect_ports_unchecked(from_node, from_out, to_node, to_in);
    }

    pub fn connect_ports(
        &mut self,
        graph: &mut PatchGraph,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
    ) {
        CanvasSession { canvas: self, graph }.connect_ports(from_node, from_out, to_node, to_in);
    }

    pub fn clear_undo_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn shift_drag_bridge_nodes(&mut self, graph: &mut PatchGraph, node_ids: &[NodeId]) {
        CanvasSession { canvas: self, graph }.shift_drag_bridge_nodes(node_ids);
    }

    pub fn insert_node_on_edge(&mut self, graph: &mut PatchGraph, node_id: NodeId, edge_id: EdgeId) -> bool {
        CanvasSession { canvas: self, graph }.insert_node_on_edge(node_id, edge_id)
    }

    pub fn try_shift_drag_insert_on_wire(
        &mut self,
        graph: &mut PatchGraph,
        node_id: NodeId,
        pointer: Pos2,
        node_world_rect: Rect,
        transform: TSTransform,
    ) {
        CanvasSession { canvas: self, graph }.try_shift_drag_insert_on_wire(node_id, pointer, node_world_rect, transform);
    }

    pub fn node_shift_drag_eligible(&mut self, graph: &mut PatchGraph, node_id: NodeId) -> bool {
        CanvasSession { canvas: self, graph }.node_shift_drag_eligible(node_id)
    }

    pub fn node_shift_drag_insert_eligible(&mut self, graph: &mut PatchGraph, node_id: NodeId) -> bool {
        CanvasSession { canvas: self, graph }.node_shift_drag_insert_eligible(node_id)
    }

    /// Test helper: mark a node eligible for shift-drag wire insertion.
    pub fn mark_shift_drag_eligible(&mut self, node_id: NodeId) {
        self.shift_drag_eligible.insert(node_id);
    }
}

const MARQUEE_MIN_AREA: f32 = 16.0;
const PATCH_BORDER_PAD: f32 = 100.0;
const MAX_UNDO_HISTORY: usize = 100;
const NODE_RESIZE_GRAB: f32 = 6.0;
const NODE_MIN_SIZE: Vec2 = Vec2::new(48.0, BOX_H);
/// Layout scale inside the patch scene (world coordinates). Camera zoom is applied separately.
const WORLD_ZOOM: f32 = 1.0;
const SCENE_MAX_SIZE: f32 = 500_000.0;


#[derive(Clone, Copy, Debug)]
struct MarqueeState {
    start: Pos2,
    current: Pos2,
    additive: bool,
    select_wires: bool,
}


#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum WireEndpoint {
    Inlet,
    Outlet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingWire {
    node: NodeId,
    port: usize,
    end: WireEndpoint,
}

/// Saved endpoints while dragging one end of existing patch cable(s).
#[derive(Clone, Debug)]
struct WireRewireState {
    edges: Vec<WireRewireEdge>,
    dragged_end: WireEndpoint,
}

#[derive(Clone, Copy, Debug)]
struct WireRewireEdge {
    from: NodeId,
    from_port: usize,
    to: NodeId,
    to_port: usize,
}

#[derive(Clone, Debug)]
struct WireHandleGroup {
    end: WireEndpoint,
    node: NodeId,
    port: usize,
    edge_ids: Vec<EdgeId>,
}

/// Original node positions pinned while Alt-dragging copies.
#[derive(Clone, Debug)]
struct AltDragDuplicate {
    originals: HashMap<NodeId, Pos2>,
    copies: HashSet<NodeId>,
    drag_source: NodeId,
}

/// Live preview while shift-dragging a node onto a patch cord.
#[derive(Clone, Copy, Debug)]
struct ShiftDragSplicePreview {
    node_id: NodeId,
    edge_id: EdgeId,
    node_rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AltDragNodeKind {
    None,
    Original,
    Copy,
}

#[derive(Clone, Copy, Debug)]
struct NodePointerPress {
    node: NodeId,
    was_selected: bool,
}

pub struct PatchCanvas {
    mouse_view: CanvasView,
    pending_wires: Vec<PendingWire>,
    wire_drag_active: bool,
    rewire_state: Option<WireRewireState>,
    marquee: Option<MarqueeState>,
    editing_node: Option<NodeId>,
    edit_buffer: String,
    edit_start_size: Option<Vec2>,
    delay_pairs: HashMap<NodeId, NodeId>,
    pub debug_auto_combine: bool,
    pub debug_auto_send_receive: bool,
    pub debug_auto_delay: bool,
    next_box_id: u64,
    alt_drag_duplicate: Option<AltDragDuplicate>,
    shift_drag_splice_preview: Option<ShiftDragSplicePreview>,
    pub(crate) shift_drag_eligible: HashSet<NodeId>,
    undo_stack: Vec<PatchSnapshot>,
    redo_stack: Vec<PatchSnapshot>,
    node_pointer_press: Option<NodePointerPress>,
    pub operator_library: OperatorLibrary,
}

struct CanvasSession<'a> {
    canvas: &'a mut PatchCanvas,
    graph: &'a mut PatchGraph,
}

struct CanvasFrameState {
    transform: TSTransform,
    canvas_rect: Rect,
    world_clip: Rect,
}

impl CanvasSession<'_> {
    fn snapshot(&self) -> PatchSnapshot {
        PatchSnapshot {
            graph: self.graph.clone(),
            delay_pairs: self.canvas.delay_pairs.clone(),
            next_box_id: self.canvas.next_box_id,
        }
    }

    fn restore_snapshot(&mut self, snapshot: PatchSnapshot) {
        *self.graph = snapshot.graph;
        self.canvas.delay_pairs = snapshot.delay_pairs;
        self.canvas.next_box_id = snapshot.next_box_id;
        self.canvas.editing_node = None;
        self.canvas.edit_buffer.clear();
        self.canvas.edit_start_size = None;
        self.canvas.pending_wires.clear();
        self.canvas.wire_drag_active = false;
        self.canvas.rewire_state = None;
        self.canvas.alt_drag_duplicate = None;
        self.canvas.shift_drag_splice_preview = None;
        self.canvas.shift_drag_eligible.clear();
        self.canvas.marquee = None;
        self.canvas.node_pointer_press = None;
    }

    fn clear_undo_history(&mut self) {
        self.canvas.undo_stack.clear();
        self.canvas.redo_stack.clear();
    }

    fn record_undo(&mut self) {
        self.canvas.redo_stack.clear();
        if self.canvas.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.canvas.undo_stack.remove(0);
        }
        self.canvas.undo_stack.push(self.snapshot());
    }

    fn undo(&mut self) {
        let Some(snapshot) = self.canvas.undo_stack.pop() else {
            return;
        };
        self.canvas.redo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
    }

    fn redo(&mut self) {
        let Some(snapshot) = self.canvas.redo_stack.pop() else {
            return;
        };
        self.canvas.undo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
    }

    fn handle_undo_redo_input(&mut self, ui: &Ui) {
        if self.canvas.editing_node.is_some() {
            return;
        }

        let (undo, redo) = ui.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::Z) && !i.modifiers.shift,
                i.modifiers.command && i.key_pressed(Key::Z) && i.modifiers.shift,
            )
        });

        if undo {
            self.undo();
        } else if redo {
            self.redo();
        }
    }

    fn canvas_keyboard_shortcuts_active(&self) -> bool {
        self.canvas.editing_node.is_none()
    }

    fn sort_layout(&mut self) {
        if self.graph.node_count() <= 1 {
            return;
        }
        self.record_undo();
        sort_patch(&mut self.graph, true);
    }

    fn spawn_object_at(&mut self, world_pos: Pos2) {
        self.record_undo();
        self.clear_all_selection();
        let id = self.add_object(
            PdObject::Message {
                text: String::new(),
            },
            world_pos,
        );
        self.graph[id].selected = true;
        self.start_editing(id);
    }

    fn add_object(&mut self, object: PdObject, pos: Pos2) -> NodeId {
        let label = object.bracketed_label();
        let size = estimate_text_box_size(&label, &object);
        let inlets = object.inlets();
        let outlets = object.outlets();
        let box_id = format!("obj-{}", self.canvas.next_box_id);
        self.canvas.next_box_id += 1;
        let id = self.graph.add_node(Node {
            object,
            label,
            pos,
            size,
            box_id: Some(box_id),
            screen_rect: Rect::NOTHING,
            inlet_t: default_port_ts(inlets),
            outlet_t: default_port_ts(outlets),
            inlet_positions: vec![Pos2::ZERO; inlets],
            outlet_positions: vec![Pos2::ZERO; outlets],
            selected: false,
        });
        id
    }

    fn connect_ports(&mut self, from_node: NodeId, from_out: usize, to_node: NodeId, to_in: usize) {
        if self
            .find_edge_between_ports(from_node, from_out, to_node, to_in)
            .is_some()
        {
            return;
        }
        if self.canvas.rewire_state.is_none() {
            self.record_undo();
        }
        self.connect_ports_unchecked(from_node, from_out, to_node, to_in);
    }

    fn find_edge_between_ports(
        &self,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
    ) -> Option<EdgeId> {
        for edge_id in self.graph.edge_indices() {
            let edge = &self.graph[edge_id];
            if edge.from_port != from_out || edge.to_port != to_in {
                continue;
            }
            let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            if from == from_node && to == to_node {
                return Some(edge_id);
            }
        }
        None
    }

    fn connect_ports_unchecked(
        &mut self,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
    ) {
        if from_node == to_node {
            return;
        }
        if self
            .find_edge_between_ports(from_node, from_out, to_node, to_in)
            .is_some()
        {
            return;
        }

        if let Some((edge_id, existing_from, existing_out)) =
            self.find_edge_to_inlet(to_node, to_in)
        {
            if self.canvas.debug_auto_combine {
                self.connect_ports_through_combine(
                    from_node,
                    from_out,
                    to_node,
                    to_in,
                    edge_id,
                    existing_from,
                    existing_out,
                );
                return;
            }
        }

        if let Some((edge_id, existing_to, existing_to_in)) =
            self.find_edge_from_outlet(from_node, from_out)
        {
            if self.canvas.debug_auto_send_receive {
                if self.graph[existing_to].object.is_send() {
                    let Some(hex) = self.graph[existing_to].object.signal_hex() else {
                        return;
                    };
                    let receive = self.spawn_receive_near(to_node, to_in, hex);
                    self.force_connect_ports(receive, 0, to_node, to_in);
                    return;
                }
                self.connect_ports_through_send_receive(
                    from_node,
                    from_out,
                    to_node,
                    to_in,
                    edge_id,
                    existing_to,
                    existing_to_in,
                );
                return;
            }
        }

        let edge_data = EdgeData {
            from_port: from_out,
            to_port: to_in,
            selected: false,
        };

        if find_path_nodes(&self.graph, to_node, from_node).is_none() {
            self.graph.add_edge(from_node, to_node, edge_data);
            return;
        }

        if self.canvas.debug_auto_delay {
            self.connect_ports_through_delays(from_node, from_out, to_node, to_in);
        }
    }

    fn find_edge_from_outlet(
        &self,
        from_node: NodeId,
        from_out: usize,
    ) -> Option<(EdgeId, NodeId, usize)> {
        for edge_id in self.graph.edge_indices() {
            let edge = &self.graph[edge_id];
            if edge.from_port != from_out {
                continue;
            }
            let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            if from == from_node {
                return Some((edge_id, to, edge.to_port));
            }
        }
        None
    }

    fn connect_ports_through_send_receive(
        &mut self,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
        existing_edge_id: EdgeId,
        existing_to: NodeId,
        existing_to_in: usize,
    ) {
        self.graph.remove_edge(existing_edge_id);

        let hex = self.random_signal_hex_id();
        let send = self.spawn_send_near(from_node, from_out, hex);
        let receive_existing = self.spawn_receive_near(existing_to, existing_to_in, hex);
        let receive_new = self.spawn_receive_near(to_node, to_in, hex);

        self.force_connect_ports(from_node, from_out, send, 0);
        self.force_connect_ports(receive_existing, 0, existing_to, existing_to_in);
        self.force_connect_ports(receive_new, 0, to_node, to_in);
    }

    fn spawn_send_near(&mut self, from_node: NodeId, from_out: usize, hex: u8) -> NodeId {
        let node = self.graph[from_node].clone();
        let send_size = estimate_node_size(&PdObject::Send { id: Some(hex) });
        let t = node.outlet_t.get(from_out).copied().unwrap_or(0.5);
        let rect = Rect::from_min_size(node.pos, node.size);
        let outlet_y = port_position_t(rect, t, true, 1.0).y;
        let pos = pos2(
            node.pos.x + node.size.x + GRID_STEP * 2.0,
            outlet_y - send_size.y / 2.0,
        );
        self.add_object(PdObject::Send { id: Some(hex) }, pos)
    }

    fn spawn_receive_near(&mut self, to_node: NodeId, to_in: usize, hex: u8) -> NodeId {
        let node = self.graph[to_node].clone();
        let receive_size = estimate_node_size(&PdObject::Receive { id: Some(hex) });
        let t = node.inlet_t.get(to_in).copied().unwrap_or(0.5);
        let rect = Rect::from_min_size(node.pos, node.size);
        let inlet_y = port_position_t(rect, t, false, 1.0).y;
        let pos = pos2(
            node.pos.x - receive_size.x - GRID_STEP * 2.0,
            inlet_y - receive_size.y / 2.0,
        );
        self.add_object(PdObject::Receive { id: Some(hex) }, pos)
    }

    fn find_edge_to_inlet(
        &self,
        to_node: NodeId,
        to_in: usize,
    ) -> Option<(EdgeId, NodeId, usize)> {
        for edge_id in self.graph.edge_indices() {
            let edge = &self.graph[edge_id];
            if edge.to_port != to_in {
                continue;
            }
            let Some((from_node, target)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            if target == to_node {
                return Some((edge_id, from_node, edge.from_port));
            }
        }
        None
    }

    fn connect_ports_through_combine(
        &mut self,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
        existing_edge_id: EdgeId,
        existing_from: NodeId,
        existing_out: usize,
    ) {
        self.graph.remove_edge(existing_edge_id);
        let combine = self.spawn_combine_near(to_node, to_in);
        self.connect_ports_unchecked(existing_from, existing_out, combine, 0);
        self.connect_ports_unchecked(from_node, from_out, combine, 1);
        self.connect_ports_unchecked(combine, 0, to_node, to_in);
    }

    fn spawn_combine_near(&mut self, to_node: NodeId, to_in: usize) -> NodeId {
        let node = self.graph[to_node].clone();
        let combine_size = estimate_node_size(&PdObject::Combine);
        let t = node.inlet_t.get(to_in).copied().unwrap_or(0.5);
        let rect = Rect::from_min_size(node.pos, node.size);
        let inlet_y = port_position_t(rect, t, false, 1.0).y;
        let pos = pos2(
            node.pos.x - combine_size.x - GRID_STEP * 2.0,
            inlet_y - combine_size.y / 2.0,
        );
        self.add_object(PdObject::Combine, pos)
    }

    fn connect_ports_through_delays(
        &mut self,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
    ) {
        let (delay_out, delay_in) = self.add_delay_pair_for_cycle(from_node, to_node);
        self.force_connect_ports(from_node, from_out, delay_out, 0);
        self.force_connect_ports(delay_in, 0, to_node, to_in);
    }

    fn force_connect_ports(
        &mut self,
        from_node: NodeId,
        from_out: usize,
        to_node: NodeId,
        to_in: usize,
    ) {
        if self
            .find_edge_between_ports(from_node, from_out, to_node, to_in)
            .is_some()
        {
            return;
        }
        self.graph
            .add_edge(from_node, to_node, EdgeData {
                from_port: from_out,
                to_port: to_in,
                selected: false,
            });
    }

    fn add_delay_pair_for_cycle(&mut self, from_node: NodeId, to_node: NodeId) -> (NodeId, NodeId) {
        let cycle_nodes = find_cycle_nodes(&self.graph, from_node, to_node);
        let (min_top, max_bottom, center_x) = cycle_vertical_bounds(&self.graph, &cycle_nodes);
        let pad = GRID_STEP * 2.0;

        let hex = self.random_signal_hex_id();
        let delay_out_object = PdObject::DelayOut { id: Some(hex) };
        let delay_in_object = PdObject::DelayIn { id: Some(hex) };

        let delay_out_size = estimate_node_size(&delay_out_object);
        let delay_in_size = estimate_node_size(&delay_in_object);

        let delay_out_pos = pos2(
            center_x - delay_out_size.x / 2.0,
            max_bottom + pad,
        );
        let delay_in_pos = pos2(
            center_x - delay_in_size.x / 2.0,
            min_top - pad - delay_in_size.y,
        );

        let delay_out = self.add_object(delay_out_object, delay_out_pos);
        let delay_in = self.add_object(delay_in_object, delay_in_pos);
        self.register_delay_pair(delay_out, delay_in);
        (delay_out, delay_in)
    }

    fn random_signal_hex_id(&self) -> u8 {
        let used = self.used_signal_hex_ids();
        random_unused_delay_hex(&used)
    }

    fn used_signal_hex_ids(&self) -> HashSet<u8> {
        let mut used = HashSet::new();
        for node_id in self.graph.node_indices() {
            if let Some(hex) = self.graph[node_id].object.signal_hex() {
                used.insert(hex);
            }
        }
        used
    }

    fn random_delay_hex_id(&self) -> u8 {
        self.random_signal_hex_id()
    }

    fn used_delay_hex_ids(&self) -> HashSet<u8> {
        self.used_signal_hex_ids()
    }

    fn register_delay_pair(&mut self, delay_out: NodeId, delay_in: NodeId) {
        self.canvas.delay_pairs.insert(delay_out, delay_in);
        self.canvas.delay_pairs.insert(delay_in, delay_out);
    }

    fn handle_port_click(&mut self, pointer: Pos2, transform: TSTransform) {
        self.stop_editing(true);

        if let Some(pending) = self.canvas.pending_wires.first().copied() {
            match pending.end {
                WireEndpoint::Outlet => {
                    if let Some((to_node, to_in)) = self.find_inlet_at(pointer, transform) {
                        self.connect_ports(pending.node, pending.port, to_node, to_in);
                        self.finish_rewire();
                        self.cancel_patching();
                        return;
                    }
                    if let Some((node_id, port)) = self.find_outlet_at(pointer, transform) {
                        self.canvas.pending_wires = vec![PendingWire {
                            node: node_id,
                            port,
                            end: WireEndpoint::Outlet,
                        }];
                        return;
                    }
                }
                WireEndpoint::Inlet => {
                    if let Some((from_node, from_out)) = self.find_outlet_at(pointer, transform) {
                        self.connect_ports(from_node, from_out, pending.node, pending.port);
                        self.finish_rewire();
                        self.cancel_patching();
                        return;
                    }
                    if let Some((node_id, port)) = self.find_inlet_at(pointer, transform) {
                        self.canvas.pending_wires = vec![PendingWire {
                            node: node_id,
                            port,
                            end: WireEndpoint::Inlet,
                        }];
                        return;
                    }
                }
            }
            return;
        }

        if let Some((node_id, port)) = self.find_outlet_at(pointer, transform) {
            self.canvas.pending_wires = vec![PendingWire {
                node: node_id,
                port,
                end: WireEndpoint::Outlet,
            }];
        } else if let Some((node_id, port)) = self.find_inlet_at(pointer, transform) {
            self.canvas.pending_wires = vec![PendingWire {
                node: node_id,
                port,
                end: WireEndpoint::Inlet,
            }];
        }
    }

    fn sync_node_ports(&mut self, node_id: NodeId) {
        let (inlets, outlets) = {
            let node = &self.graph[node_id];
            (node.object.inlets(), node.object.outlets())
        };

        if let Some(node) = self.graph.node_weight_mut(node_id) {
            if node.inlet_t.len() != inlets {
                node.inlet_t = default_port_ts(inlets);
            }
            if node.outlet_t.len() != outlets {
                node.outlet_t = default_port_ts(outlets);
            }
            node.inlet_positions.resize(inlets, Pos2::ZERO);
            node.outlet_positions.resize(outlets, Pos2::ZERO);
        }

        let invalid_edges: Vec<EdgeId> = self
            .graph
            .edge_indices()
            .filter(|&edge_id| {
                let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                    return false;
                };
                let edge = &self.graph[edge_id];
                (from == node_id && edge.from_port >= outlets)
                    || (to == node_id && edge.to_port >= inlets)
            })
            .collect();
        for edge_id in invalid_edges {
            self.graph.remove_edge(edge_id);
        }
    }

    fn start_editing(&mut self, node_id: NodeId) {
        self.stop_editing(true);
        if let Some(node) = self.graph.node_weight(node_id) {
            self.canvas.edit_buffer = node.label.clone();
            self.canvas.edit_start_size = Some(node.size);
            self.canvas.editing_node = Some(node_id);
        }
    }

    fn stop_editing(&mut self, commit: bool) {
        let Some(node_id) = self.canvas.editing_node.take() else {
            return;
        };
        let start_size = self.canvas.edit_start_size.take();

        if commit {
            if let Some(node) = self.graph.node_weight(node_id) {
                if self.canvas.edit_buffer != node.label {
                    self.record_undo();
                }
            }
            if let Some(node) = self.graph.node_weight_mut(node_id) {
                commit_node_label(node, &self.canvas.edit_buffer);
                self.sync_node_ports(node_id);
            }
        } else if let Some(size) = start_size {
            if let Some(node) = self.graph.node_weight_mut(node_id) {
                node.size = size;
            }
        }

        self.canvas.edit_buffer.clear();
    }

    fn remove_node(&mut self, id: NodeId) {
        if !self.graph.contains_node(id) {
            return;
        }

        if let Some(partner) = self.canvas.delay_pairs.remove(&id) {
            self.canvas.delay_pairs.remove(&partner);
            if partner != id && self.graph.contains_node(partner) {
                self.remove_node_internal(partner);
            }
        }

        self.remove_node_internal(id);
    }

    fn remove_node_internal(&mut self, id: NodeId) {
        if self.canvas.editing_node == Some(id) {
            self.canvas.editing_node = None;
            self.canvas.edit_buffer.clear();
            self.canvas.edit_start_size = None;
        }

        if let Some(hex) = self.graph[id].object.signal_hex() {
            if self.graph[id].object.is_send() {
                let receive_ids: Vec<NodeId> = self
                    .graph
                    .node_indices()
                    .filter(|&node_id| {
                        self.graph[node_id].object.is_receive()
                            && self.graph[node_id].object.signal_hex() == Some(hex)
                    })
                    .collect();
                for receive_id in receive_ids {
                    let _ = self.graph.remove_node(receive_id);
                }
            }
        }

        let _ = self.graph.remove_node(id);
    }

    fn clear_node_selection(&mut self) {
        let node_ids: Vec<NodeId> = self.graph.node_indices().collect();
        for node_id in node_ids {
            self.graph[node_id].selected = false;
        }
    }

    fn clear_wire_selection(&mut self) {
        let edge_ids: Vec<EdgeId> = self.graph.edge_indices().collect();
        for edge_id in edge_ids {
            self.graph[edge_id].selected = false;
        }
    }

    fn clear_all_selection(&mut self) {
        self.clear_node_selection();
        self.clear_wire_selection();
    }

    fn selected_nodes(&self) -> Vec<NodeId> {
        self.graph
            .node_indices()
            .filter(|&id| self.graph[id].selected)
            .collect()
    }

    fn delete_selected(&mut self) {
        if self.selected_nodes().is_empty()
            && !self
                .graph
                .edge_indices()
                .any(|id| self.graph[id].selected)
        {
            return;
        }
        self.record_undo();
        for id in self.selected_nodes() {
            self.remove_node(id);
        }
        let selected_edges: Vec<EdgeId> = self
            .graph
            .edge_indices()
            .filter(|&id| self.graph[id].selected)
            .collect();
        let had_edges = !selected_edges.is_empty();
        for edge_id in selected_edges {
            self.graph.remove_edge(edge_id);
        }
        if had_edges {
        }
        self.cancel_patching();
    }

    fn apply_marquee_selection(&mut self, marquee: MarqueeState, transform: TSTransform) {
        let marquee_rect = marquee_rect(marquee);
        if marquee.select_wires {
            if !marquee.additive {
                self.clear_wire_selection();
            }
            let selected_edges: Vec<EdgeId> = self
                .graph
                .edge_indices()
                .filter(|&edge_id| self.edge_intersects_rect(edge_id, marquee_rect, transform))
                .collect();
            for edge_id in selected_edges {
                self.graph[edge_id].selected = true;
            }
        } else {
            if !marquee.additive {
                self.clear_all_selection();
            }
            let node_ids: Vec<NodeId> = self.graph.node_indices().collect();
            for node_id in node_ids {
                let node = &mut self.graph[node_id];
                if node.screen_rect.is_positive() && node.screen_rect.intersects(marquee_rect) {
                    node.selected = true;
                }
            }
        }
    }

    fn edge_bezier_points(&self, edge_id: EdgeId) -> Option<[Pos2; 4]> {
        let (from_id, to_id) = self.graph.edge_endpoints(edge_id)?;
        let edge = &self.graph[edge_id];
        let from_node = self.graph.node_weight(from_id)?;
        let to_node = self.graph.node_weight(to_id)?;
        let from = socket_position(from_node, edge.from_port, true);
        let to = socket_position(to_node, edge.to_port, false);
        Some(wire_bezier_points(from, true, to, true))
    }

    fn wire_hit_radius(&self, zoom: f32) -> f32 {
        (CABLE_STROKE * WORLD_ZOOM * 2.5).max(8.0 / zoom)
    }

    fn find_edge_at(&self, pointer: Pos2, transform: TSTransform) -> Option<EdgeId> {
        let pointer_world = screen_to_world_pos(transform, pointer);
        let hit = self.wire_hit_radius(transform.scaling);
        let mut best: Option<(EdgeId, f32)> = None;

        for edge_id in self.graph.edge_indices() {
            let Some(points) = self.edge_bezier_points(edge_id) else {
                continue;
            };
            let dist = distance_to_cubic_bezier(pointer_world, points);
            if dist <= hit && best.is_none_or(|(_, d)| dist < d) {
                best = Some((edge_id, dist));
            }
        }

        best.map(|(id, _)| id)
    }

    fn wire_handle_hit_radius(&self) -> f32 {
        port_size(WORLD_ZOOM) * 1.35
    }

    fn edges_showing_handles(&self) -> Vec<EdgeId> {
        if self.canvas.rewire_state.is_some() {
            return Vec::new();
        }
        self.graph
            .edge_indices()
            .filter(|&edge_id| self.graph[edge_id].selected)
            .collect()
    }

    fn wire_handle_groups(&self) -> Vec<WireHandleGroup> {
        let mut map: HashMap<(NodeId, usize, WireEndpoint), Vec<EdgeId>> = HashMap::new();

        for edge_id in self.edges_showing_handles() {
            let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            let edge = &self.graph[edge_id];
            map.entry((from, edge.from_port, WireEndpoint::Outlet))
                .or_default()
                .push(edge_id);
            map.entry((to, edge.to_port, WireEndpoint::Inlet))
                .or_default()
                .push(edge_id);
        }

        map.into_iter()
            .map(|((node, port, end), mut edge_ids)| {
                edge_ids.sort_by_key(|id| id.index());
                edge_ids.dedup();
                WireHandleGroup {
                    end,
                    node,
                    port,
                    edge_ids,
                }
            })
            .collect()
    }

    fn handle_center_for_group(&self, group: &WireHandleGroup, transform: TSTransform) -> Option<Pos2> {
        if group.edge_ids.len() == 1 {
            let edge_id = group.edge_ids[0];
            let points = self.edge_bezier_points(edge_id)?;
            let [outlet, inlet] = wire_handle_positions(points, WORLD_ZOOM);
            return Some(match group.end {
                WireEndpoint::Outlet => outlet,
                WireEndpoint::Inlet => inlet,
            });
        }

        if !self.graph.contains_node(group.node) {
            return None;
        }
        let node = &self.graph[group.node];
        let rect = node_layout_world_rect(node, transform);
        if !rect.is_positive() {
            return None;
        }

        let is_outlet = group.end == WireEndpoint::Outlet;
        let t = if is_outlet {
            node.outlet_t.get(group.port).copied()
        } else {
            node.inlet_t.get(group.port).copied()
        }
        .unwrap_or(0.5);
        let socket = port_position_t(rect, t, is_outlet, WORLD_ZOOM);
        let offset = port_size(WORLD_ZOOM) * 2.4;
        let nudge = if is_outlet {
            vec2(0.0, offset)
        } else {
            vec2(0.0, -offset)
        };
        Some(socket + nudge)
    }

    fn find_wire_handle_at(&self, pointer: Pos2, transform: TSTransform) -> Option<WireHandleGroup> {
        let pointer_world = screen_to_world_pos(transform, pointer);
        let hit = self.wire_handle_hit_radius();
        let mut best: Option<(WireHandleGroup, f32)> = None;

        for group in self.wire_handle_groups() {
            let Some(center) = self.handle_center_for_group(&group, transform) else {
                continue;
            };
            let dist = pointer_world.distance(center);
            if dist <= hit {
                if best.as_ref().is_none_or(|(_, d)| dist < *d) {
                    best = Some((group, dist));
                }
            }
        }

        best.map(|(group, _)| group)
    }

    fn pointer_on_wire_handle(&self, pointer: Pos2, transform: TSTransform) -> bool {
        self.find_wire_handle_at(pointer, transform).is_some()
    }

    fn show_wire_handles(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        transform: TSTransform,
    ) {
        if self.canvas.rewire_state.is_some() {
            return;
        }

        let ctx = ui.ctx();
        let zoom = transform.scaling;
        let groups = self.wire_handle_groups();

        for group in groups {
            let Some(world_center) = self.handle_center_for_group(&group, transform) else {
                continue;
            };
            let combined = group.edge_ids.len() > 1;
            let handle_size = port_size(zoom) * if combined { 1.35 } else { 1.1 };
            let screen_center = world_to_screen_pos(transform, world_center);
            let screen_rect =
                Rect::from_center_size(screen_center, Vec2::splat(handle_size));
            if !node_visible_on_canvas(canvas_rect, screen_rect) {
                continue;
            }
            let end_tag = match group.end {
                WireEndpoint::Outlet => 0u8,
                WireEndpoint::Inlet => 1u8,
            };
            let response = show_wire_handle_widget(
                ctx,
                Id::new(("wire_handle", group.node.index(), group.port, end_tag)),
                screen_center,
                combined,
                canvas_rect,
                zoom,
            );
            let pressed = response.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.primary_pressed());
            if (response.drag_started() || pressed) && self.canvas.pending_wires.is_empty() {
                self.stop_editing(true);
                self.start_group_rewire_from_handle(&group);
            }
        }
    }

    fn finish_rewire(&mut self) {
        self.canvas.rewire_state = None;
    }

    fn edge_intersects_rect(&self, edge_id: EdgeId, rect: Rect, transform: TSTransform) -> bool {
        let Some(points) = self.edge_bezier_points(edge_id) else {
            return false;
        };
        let screen_points = points.map(|p| transform * p);
        bezier_intersects_rect(screen_points, rect)
    }

    fn handle_wire_click(&mut self, edge_id: EdgeId, additive: bool) {
        self.stop_editing(true);

        if additive {
            if let Some(edge) = self.graph.edge_weight_mut(edge_id) {
                edge.selected = !edge.selected;
            }
            return;
        }

        self.clear_all_selection();
        if let Some(edge) = self.graph.edge_weight_mut(edge_id) {
            edge.selected = true;
        }
    }

    fn move_selected_by(&mut self, delta_world: Vec2) {
        if delta_world.length_sq() == 0.0 {
            return;
        }
        let node_ids: Vec<NodeId> = self.graph.node_indices().collect();
        for node_id in node_ids {
            let node = &mut self.graph[node_id];
            if node.selected {
                node.pos += delta_world;
            }
        }
    }

    fn collect_node_edges(
        &self,
        node_id: NodeId,
    ) -> (
        Vec<(EdgeId, NodeId, usize, usize)>,
        Vec<(EdgeId, usize, NodeId, usize)>,
    ) {
        let mut incoming = Vec::new();
        let mut outgoing = Vec::new();

        for edge_id in self.graph.edge_indices() {
            let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            let edge = &self.graph[edge_id];
            if to == node_id {
                incoming.push((edge_id, from, edge.from_port, edge.to_port));
            }
            if from == node_id {
                outgoing.push((edge_id, edge.from_port, to, edge.to_port));
            }
        }

        (incoming, outgoing)
    }

    fn node_shift_drag_eligible(&self, node_id: NodeId) -> bool {
        if !self.graph.contains_node(node_id) {
            return false;
        }
        let (incoming, outgoing) = self.collect_node_edges(node_id);
        incoming.len() == 1 && outgoing.len() == 1
    }

    fn node_shift_drag_insert_eligible(&self, node_id: NodeId) -> bool {
        if !self.graph.contains_node(node_id) {
            return false;
        }
        let node = &self.graph[node_id];
        !node.object.is_comment()
            && node.object.inlets() > 0
            && node.object.outlets() > 0
    }

    /// Max-style shift-drag: disconnect selected node(s) and bridge upstream to downstream.
    fn shift_drag_bridge_nodes(&mut self, node_ids: &[NodeId]) {
        if node_ids.is_empty() {
            return;
        }

        let mut edges_to_remove = HashSet::new();
        let mut bridges = Vec::new();

        for &node_id in node_ids {
            if !self.node_shift_drag_eligible(node_id) {
                continue;
            }

            let (incoming, outgoing) = self.collect_node_edges(node_id);
            let (_, from, from_port, _) = incoming[0];
            let (_, _, to, to_port) = outgoing[0];
            bridges.push((from, from_port, to, to_port));

            for (edge_id, _, _, _) in &incoming {
                edges_to_remove.insert(*edge_id);
            }
            for (edge_id, _, _, _) in &outgoing {
                edges_to_remove.insert(*edge_id);
            }
        }

        if edges_to_remove.is_empty() {
            return;
        }

        for edge_id in edges_to_remove {
            self.graph.remove_edge(edge_id);
        }
        for (from, from_port, to, to_port) in bridges {
            self.force_connect_ports(from, from_port, to, to_port);
        }
    }

    /// Max-style shift-drag: insert `node_id` into `edge_id` (from → node → to).
    fn can_insert_node_on_edge(&self, node_id: NodeId, edge_id: EdgeId) -> bool {
        if !self.graph.contains_node(node_id) {
            return false;
        }

        let node = &self.graph[node_id];
        if node.object.is_comment() || node.object.inlets() == 0 || node.object.outlets() == 0 {
            return false;
        }

        let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
            return false;
        };
        from != node_id && to != node_id
    }

    fn insert_node_on_edge(&mut self, node_id: NodeId, edge_id: EdgeId) -> bool {
        if !self.can_insert_node_on_edge(node_id, edge_id) {
            return false;
        }

        let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
            return false;
        };

        let edge = self.graph[edge_id].clone();
        self.graph.remove_edge(edge_id);
        self.force_connect_ports(from, edge.from_port, node_id, 0);
        self.force_connect_ports(node_id, 0, to, edge.to_port);
        true
    }

    fn find_splice_edge_for_node(
        &self,
        node_id: NodeId,
        pointer: Pos2,
        node_world_rect: Rect,
        transform: TSTransform,
    ) -> Option<EdgeId> {
        if !self.graph.contains_node(node_id) {
            return None;
        }
        let node = &self.graph[node_id];
        if node.object.is_comment() || node.object.inlets() == 0 || node.object.outlets() == 0 {
            return None;
        }

        if let Some(edge_id) = self.find_edge_at(pointer, transform) {
            if self.can_insert_node_on_edge(node_id, edge_id) {
                return Some(edge_id);
            }
        }

        let hit = self.wire_hit_radius(transform.scaling);
        let probe = node_world_rect.expand(hit);
        let center = node_world_rect.center();
        let mut best: Option<(EdgeId, f32)> = None;

        for edge_id in self.graph.edge_indices() {
            if !self.can_insert_node_on_edge(node_id, edge_id) {
                continue;
            }
            let Some(points) = self.edge_bezier_points(edge_id) else {
                continue;
            };
            let intersects = bezier_intersects_rect(points, probe);
            let dist = distance_to_cubic_bezier(center, points);
            if !intersects && dist > hit {
                continue;
            }
            if best.is_none_or(|(_, d)| dist < d) {
                best = Some((edge_id, dist));
            }
        }

        best.map(|(edge_id, _)| edge_id)
    }

    fn try_shift_drag_insert_on_wire(
        &mut self,
        node_id: NodeId,
        pointer: Pos2,
        node_world_rect: Rect,
        transform: TSTransform,
    ) {
        if !self.canvas.shift_drag_eligible.contains(&node_id) {
            return;
        }
        let Some(edge_id) =
            self.find_splice_edge_for_node(node_id, pointer, node_world_rect, transform)
        else {
            return;
        };
        self.insert_node_on_edge(node_id, edge_id);
    }

    fn update_shift_drag_splice_preview(
        &mut self,
        node_id: NodeId,
        pointer: Pos2,
        node_world_rect: Rect,
        transform: TSTransform,
    ) {
        if !self.canvas.shift_drag_eligible.contains(&node_id) {
            return;
        }
        self.canvas.shift_drag_splice_preview = self
            .find_splice_edge_for_node(node_id, pointer, node_world_rect, transform)
            .map(|edge_id| ShiftDragSplicePreview {
                node_id,
                edge_id,
                node_rect: node_world_rect,
            });
    }

    fn splice_preview_segments(
        &self,
        preview: ShiftDragSplicePreview,
    ) -> Option<([Pos2; 4], [Pos2; 4])> {
        let ShiftDragSplicePreview {
            node_id,
            edge_id,
            node_rect,
        } = preview;
        let (from, to) = self.graph.edge_endpoints(edge_id)?;
        let edge = &self.graph[edge_id];
        if !self.graph.contains_node(node_id) {
            return None;
        }

        let from_node = self.graph.node_weight(from)?;
        let to_node = self.graph.node_weight(to)?;
        let from_out = socket_position(from_node, edge.from_port, true);
        let to_in = socket_position(to_node, edge.to_port, false);

        let node = self.graph.node_weight(node_id)?;
        let inlet_t = node.inlet_t.first().copied().unwrap_or(0.0);
        let outlet_t = node.outlet_t.first().copied().unwrap_or(0.0);
        let node_in = port_position_t(node_rect, inlet_t, false, WORLD_ZOOM);
        let node_out = port_position_t(node_rect, outlet_t, true, WORLD_ZOOM);

        Some((
            wire_bezier_points(from_out, true, node_in, true),
            wire_bezier_points(node_out, true, to_in, true),
        ))
    }

    fn draw_shift_drag_splice_preview(&self, painter: &egui::Painter) {
        let Some(preview) = self.canvas.shift_drag_splice_preview else {
            return;
        };
        let Some((upstream, downstream)) = self.splice_preview_segments(preview) else {
            return;
        };
        draw_bezier_wire_colored(painter, upstream, WIRE_HANDLE_HOVER);
        draw_bezier_wire_colored(painter, downstream, WIRE_HANDLE_HOVER);
    }

    fn nodes_to_duplicate(&self, dragged: NodeId) -> Vec<NodeId> {
        let selected = self.selected_nodes();
        if self.graph[dragged].selected && selected.len() > 1 {
            selected
        } else {
            vec![dragged]
        }
    }

    fn remap_copied_signal_hex(object: &mut PdObject, hex_remap: &HashMap<u8, u8>) {
        let remap = |id: &mut Option<u8>| {
            if let Some(old) = *id {
                if let Some(&new) = hex_remap.get(&old) {
                    *id = Some(new);
                }
            }
        };
        match object {
            PdObject::DelayIn { id }
            | PdObject::DelayOut { id }
            | PdObject::Send { id }
            | PdObject::Receive { id } => remap(id),
            _ => {}
        }
    }

    fn duplicate_nodes(&mut self, ids: &[NodeId]) -> HashMap<NodeId, NodeId> {
        let id_set: HashSet<NodeId> = ids.iter().copied().collect();
        let mut hex_remap = HashMap::new();
        for &id in ids {
            if let Some(old_hex) = self.graph[id].object.signal_hex() {
                hex_remap
                    .entry(old_hex)
                    .or_insert_with(|| self.random_signal_hex_id());
            }
        }

        let mut id_map = HashMap::new();
        for &id in ids {
            let source = self.graph[id].clone();
            let mut object = source.object.clone();
            CanvasSession::remap_copied_signal_hex(&mut object, &hex_remap);
            let new_id = self.add_object(object, source.pos);
            if let Some(new_node) = self.graph.node_weight_mut(new_id) {
                new_node.size = source.size;
                new_node.inlet_t = source.inlet_t.clone();
                new_node.outlet_t = source.outlet_t.clone();
            }
            id_map.insert(id, new_id);
        }

        let edge_ids: Vec<EdgeId> = self.graph.edge_indices().collect();
        for edge_id in edge_ids {
            let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            if !id_set.contains(&from) || !id_set.contains(&to) {
                continue;
            }
            let edge = &self.graph[edge_id];
            let Some(&new_from) = id_map.get(&from) else {
                continue;
            };
            let Some(&new_to) = id_map.get(&to) else {
                continue;
            };
            self.force_connect_ports(new_from, edge.from_port, new_to, edge.to_port);
        }

        let mut registered_pairs = HashSet::new();
        for &old in ids {
            let Some(partner_old) = self.canvas.delay_pairs.get(&old).copied() else {
                continue;
            };
            let Some(&new) = id_map.get(&old) else {
                continue;
            };
            let Some(&partner_new) = id_map.get(&partner_old) else {
                continue;
            };
            let key = (new.index().min(partner_new.index()), new.index().max(partner_new.index()));
            if !registered_pairs.insert(key) {
                continue;
            }
            if matches!(self.graph[new].object, PdObject::DelayOut { .. }) {
                self.register_delay_pair(new, partner_new);
            } else {
                self.register_delay_pair(partner_new, new);
            }
        }

        id_map
    }

    fn begin_alt_drag_duplicate(&mut self, dragged: NodeId) {
        self.record_undo();
        let ids = self.nodes_to_duplicate(dragged);
        let originals: HashMap<_, _> = ids
            .iter()
            .map(|&id| (id, self.graph[id].pos))
            .collect();
        let id_map = self.duplicate_nodes(&ids);

        for &id in &ids {
            self.graph[id].selected = false;
        }
        for new_id in id_map.values() {
            self.graph[*new_id].selected = true;
        }

        self.canvas.alt_drag_duplicate = Some(AltDragDuplicate {
            originals,
            copies: id_map.values().copied().collect(),
            drag_source: dragged,
        });
        self.stop_editing(true);
    }

    fn alt_drag_node_kind(&self, node_id: NodeId) -> AltDragNodeKind {
        let Some(state) = &self.canvas.alt_drag_duplicate else {
            return AltDragNodeKind::None;
        };
        if state.originals.contains_key(&node_id) {
            AltDragNodeKind::Original
        } else if state.copies.contains(&node_id) {
            AltDragNodeKind::Copy
        } else {
            AltDragNodeKind::None
        }
    }

    fn nodes_world_bounds(&self) -> Option<Rect> {
        let mut bounds = Rect::NOTHING;
        let mut any = false;

        for node_id in self.graph.node_indices() {
            let node = &self.graph[node_id];
            let rect = node_world_rect(node);
            if !rect.is_positive() {
                continue;
            }
            bounds = if any { bounds.union(rect) } else { rect };
            any = true;
        }

        any.then_some(bounds)
    }

    fn default_scene_view_rect(&self) -> Rect {
        self.nodes_world_bounds()
            .map(|bounds| bounds.expand(PATCH_BORDER_PAD * 2.0))
            .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)))
    }

    fn ensure_scene_rect(&self, view: &mut CanvasView) {
        if !view.scene_rect.is_positive() {
            view.scene_rect = self.default_scene_view_rect();
        }
    }

    fn draw_patch_border(&self, painter: &egui::Painter) {
        let Some(bounds) = self.nodes_world_bounds() else {
            return;
        };

        let border = bounds.expand(PATCH_BORDER_PAD);
        painter.rect_stroke(
            border,
            CornerRadius::ZERO,
            Stroke::new(LINE_W, PAPER_DIM),
            egui::StrokeKind::Outside,
        );
    }
    fn mouse_ui(&mut self, ui: &mut Ui, editable: bool) {
        let mut scene_rect = self.canvas.mouse_view.scene_rect;
        if !scene_rect.is_positive() {
            scene_rect = self.default_scene_view_rect();
        }

        let parent_id = ui.id();
        let parent_order = ui.layer_id().order;

        let mut frame = CanvasFrameState {
            transform: TSTransform::IDENTITY,
            canvas_rect: Rect::NOTHING,
            world_clip: Rect::NOTHING,
        };

        let scene_response = show_patch_scene(ui, &mut scene_rect, |scene_ui| {
            let ctx = scene_ui.ctx().clone();
            let scene_layer = scene_layer_id(parent_id, parent_order);
            let transform = scene_transform(&ctx, parent_id, parent_order);
            let world_clip = scene_ui.clip_rect();
            let canvas_rect = transform.mul_rect(world_clip);
            frame = CanvasFrameState {
                transform,
                canvas_rect,
                world_clip,
            };

            let mut painter = ctx.layer_painter(scene_layer);
            painter.set_clip_rect(world_clip);
            draw_grid(&painter, world_clip);

            if !editable {
                return;
            }

            let clear_alt_drag = scene_ui.input(|i| i.pointer.primary_released());

            let mut node_order: Vec<NodeId> = self.graph.node_indices().collect();
            node_order.sort_by_key(|id| self.graph[*id].object.is_comment());
            if self.canvas.alt_drag_duplicate.is_some() {
                let drag_source = self.canvas.alt_drag_duplicate.as_ref().unwrap().drag_source;
                node_order.sort_by_key(|id| (*id == drag_source) as u8);
            }
            for node_id in node_order.clone() {
                self.show_pd_node(
                    scene_ui,
                    node_id,
                    canvas_rect,
                    transform,
                    &painter,
                );
            }
            if clear_alt_drag {
                self.canvas.alt_drag_duplicate = None;
            }
            if scene_ui.input(|i| i.pointer.primary_released()) {
                self.canvas.node_pointer_press = None;
                self.canvas.shift_drag_eligible.clear();
                self.canvas.shift_drag_splice_preview = None;
            }
            self.show_all_ports(scene_ui, canvas_rect, transform);
            self.draw_patch_border(&painter);
            self.draw_wires_on_painter(&painter);
            self.draw_shift_drag_splice_preview(&painter);
            self.show_wire_handles(scene_ui, canvas_rect, transform);
            self.draw_pending_wire(&painter, &ctx, transform);
            self.draw_marquee(&painter, transform);
        });

        self.canvas.mouse_view.scene_rect = scene_rect;

        if editable && frame.canvas_rect.is_positive() {
            let CanvasFrameState {
                transform,
                canvas_rect,
                ..
            } = frame;
            let response = &scene_response.response;
            self.handle_canvas_editing(ui, canvas_rect, transform, response);
            self.handle_patch_cable_input(ui, canvas_rect, transform);
            self.finish_marquee(ui, transform);
        }
    }

    fn handle_canvas_editing(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        transform: TSTransform,
        response: &egui::Response,
    ) {
        self.handle_undo_redo_input(ui);

        if self.canvas_keyboard_shortcuts_active() {
            if ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
                self.delete_selected();
            }

            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.cancel_patching();
            }
        }

        let box_selecting = response.dragged_by(egui::PointerButton::Primary)
            && !ui.input(|i| i.modifiers.command)
            && self.canvas.pending_wires.is_empty();

        if box_selecting {
            if response.drag_started() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    if !self.pointer_on_node(pointer) {
                        self.canvas.marquee = Some(MarqueeState {
                            start: pointer,
                            current: pointer,
                            additive: ui.input(|i| i.modifiers.shift),
                            select_wires: ui.input(|i| i.modifiers.alt),
                        });
                        self.cancel_patching();
                    }
                }
            }

            if let Some(marquee) = &mut self.canvas.marquee {
                if let Some(pointer) = response.interact_pointer_pos() {
                    marquee.current = pointer;
                }
            }
        }

        if response.double_clicked() && !response.secondary_clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if self.is_background_pointer(canvas_rect, pointer, transform) {
                    let world = screen_to_world_pos(transform, pointer);
                    self.spawn_object_at(world);
                }
            }
        } else if response.clicked() && !response.secondary_clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if self.is_background_pointer(canvas_rect, pointer, transform) {
                    self.stop_editing(true);
                    self.clear_all_selection();
                    self.cancel_patching();
                }
            }
        }
    }

    fn is_background_pointer(&self, canvas_rect: Rect, pointer: Pos2, transform: TSTransform) -> bool {
        canvas_rect.contains(pointer)
            && !self.pointer_on_node_or_port(pointer, transform)
            && self.find_edge_at(pointer, transform).is_none()
    }

    fn finish_marquee(&mut self, ui: &mut Ui, transform: TSTransform) {
        if !ui.input(|i| i.pointer.primary_released()) {
            return;
        }

        let Some(marquee) = self.canvas.marquee.take() else {
            return;
        };

        let rect = marquee_rect(marquee);
        if rect.area() >= MARQUEE_MIN_AREA {
            self.apply_marquee_selection(marquee, transform);
        }
    }

    fn draw_marquee(&self, painter: &egui::Painter, transform: TSTransform) {
        let Some(marquee) = self.canvas.marquee else {
            return;
        };

        let world_rect = rect_in_world_space(transform, marquee_rect(marquee));
        painter.rect_filled(
            world_rect,
            CornerRadius::ZERO,
            Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 28),
        );
        painter.rect_stroke(
            world_rect,
            CornerRadius::ZERO,
            Stroke::new(LINE_W, Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 200)),
            egui::StrokeKind::Outside,
        );
    }

    fn cancel_patching(&mut self) {
        if let Some(rewire) = self.canvas.rewire_state.take() {
            self.restore_rewired_edges(rewire);
        }
        self.canvas.pending_wires.clear();
        self.canvas.wire_drag_active = false;
    }

    fn restore_rewired_edges(&mut self, rewire: WireRewireState) {
        for edge in rewire.edges {
            self.force_connect_ports(edge.from, edge.from_port, edge.to, edge.to_port);
        }
    }

    fn start_group_rewire_from_handle(&mut self, group: &WireHandleGroup) {
        if group.edge_ids.is_empty() {
            return;
        }

        self.record_undo();
        let mut saved = Vec::new();
        let mut pending = Vec::new();

        for &edge_id in &group.edge_ids {
            let Some((from, to)) = self.graph.edge_endpoints(edge_id) else {
                continue;
            };
            let edge = self.graph[edge_id].clone();
            saved.push(WireRewireEdge {
                from,
                from_port: edge.from_port,
                to,
                to_port: edge.to_port,
            });
            self.graph.remove_edge(edge_id);
            pending.push(match group.end {
                WireEndpoint::Inlet => PendingWire {
                    node: from,
                    port: edge.from_port,
                    end: WireEndpoint::Outlet,
                },
                WireEndpoint::Outlet => PendingWire {
                    node: to,
                    port: edge.to_port,
                    end: WireEndpoint::Inlet,
                },
            });
        }

        if saved.is_empty() {
            return;
        }

        self.canvas.rewire_state = Some(WireRewireState {
            edges: saved,
            dragged_end: group.end,
        });
        self.canvas.pending_wires = pending;
        self.canvas.wire_drag_active = true;
    }

    fn pointer_on_node(&self, pointer: Pos2) -> bool {
        self.graph
            .node_weights()
            .any(|node| node.screen_rect.is_positive() && node.screen_rect.contains(pointer))
    }

    fn pointer_on_node_or_port(&self, pointer: Pos2, transform: TSTransform) -> bool {
        self.pointer_on_node(pointer)
            || self.pointer_on_wire_handle(pointer, transform)
            || self.find_inlet_at(pointer, transform).is_some()
            || self.find_outlet_at(pointer, transform).is_some()
    }

    fn find_inlet_at(&self, pointer: Pos2, transform: TSTransform) -> Option<(NodeId, usize)> {
        let pointer_world = screen_to_world_pos(transform, pointer);
        let hit = port_size(WORLD_ZOOM) * 1.5;
        let mut best: Option<(NodeId, usize, f32)> = None;

        for node_id in self.graph.node_indices() {
            let node = &self.graph[node_id];
            for (index, inlet_pos) in node.inlet_positions.iter().enumerate() {
                let dist = pointer_world.distance(*inlet_pos);
                if dist <= hit {
                    if best.is_none_or(|(_, _, d)| dist < d) {
                        best = Some((node_id, index, dist));
                    }
                }
            }
        }

        best.map(|(id, index, _)| (id, index))
    }

    fn find_outlet_at(&self, pointer: Pos2, transform: TSTransform) -> Option<(NodeId, usize)> {
        let pointer_world = screen_to_world_pos(transform, pointer);
        let hit = port_size(WORLD_ZOOM) * 1.5;
        let mut best: Option<(NodeId, usize, f32)> = None;

        for node_id in self.graph.node_indices() {
            let node = &self.graph[node_id];
            for (index, outlet_pos) in node.outlet_positions.iter().enumerate() {
                let dist = pointer_world.distance(*outlet_pos);
                if dist <= hit {
                    if best.is_none_or(|(_, _, d)| dist < d) {
                        best = Some((node_id, index, dist));
                    }
                }
            }
        }

        best.map(|(id, index, _)| (id, index))
    }

    fn handle_patch_cable_input(&mut self, ui: &mut Ui, canvas_rect: Rect, transform: TSTransform) {
        if !self.canvas.pending_wires.is_empty()
            && ui.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging())
        {
            self.canvas.wire_drag_active = true;
        }

        if ui.input(|i| i.pointer.primary_clicked()) {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                if canvas_rect.contains(pointer) {
                    if self.find_inlet_at(pointer, transform).is_some()
                        || self.find_outlet_at(pointer, transform).is_some()
                    {
                        self.handle_port_click(pointer, transform);
                    } else if let Some(edge_id) = self.find_edge_at(pointer, transform) {
                        let additive = ui.input(|i| i.modifiers.shift);
                        self.handle_wire_click(edge_id, additive);
                    } else if self.is_background_pointer(canvas_rect, pointer, transform) {
                        self.stop_editing(true);
                        self.cancel_patching();
                    }
                }
            }
        }

        if ui.input(|i| i.pointer.primary_released()) && !self.canvas.pending_wires.is_empty() {
            let dragging = ui.input(|i| i.pointer.is_decidedly_dragging()) || self.canvas.wire_drag_active;
            let mut connected = false;
            if dragging {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let rewire = self.canvas.rewire_state.clone();
                    if let Some(rewire) = rewire {
                        match rewire.dragged_end {
                            WireEndpoint::Inlet => {
                                if let Some((to_node, to_in)) = self.find_inlet_at(pointer, transform) {
                                    for edge in rewire.edges {
                                        self.connect_ports(
                                            edge.from,
                                            edge.from_port,
                                            to_node,
                                            to_in,
                                        );
                                    }
                                    connected = true;
                                }
                            }
                            WireEndpoint::Outlet => {
                                if let Some((from_node, from_out)) = self.find_outlet_at(pointer, transform) {
                                    for edge in rewire.edges {
                                        self.connect_ports(
                                            from_node,
                                            from_out,
                                            edge.to,
                                            edge.to_port,
                                        );
                                    }
                                    connected = true;
                                }
                            }
                        }
                    } else if let Some(pending) = self.canvas.pending_wires.first().copied() {
                        match pending.end {
                            WireEndpoint::Outlet => {
                                if let Some((to_node, to_in)) = self.find_inlet_at(pointer, transform) {
                                    self.connect_ports(pending.node, pending.port, to_node, to_in);
                                    connected = true;
                                }
                            }
                            WireEndpoint::Inlet => {
                                if let Some((from_node, from_out)) = self.find_outlet_at(pointer, transform) {
                                    self.connect_ports(from_node, from_out, pending.node, pending.port);
                                    connected = true;
                                }
                            }
                        }
                    }
                }
            }
            if connected {
                self.finish_rewire();
                self.canvas.pending_wires.clear();
                self.canvas.wire_drag_active = false;
            } else if dragging || self.canvas.rewire_state.is_some() {
                self.cancel_patching();
            }
        }
    }

    fn show_pd_node(
        &mut self,
        ui: &mut Ui,
        node_id: NodeId,
        canvas_rect: Rect,
        transform: TSTransform,
        patch_painter: &egui::Painter,
    ) {
        let zoom = transform.scaling;
        let (world_pos, is_comment, was_selected, world_size) = {
            let node = &self.graph[node_id];
            (
                node.pos,
                node.object.is_comment(),
                node.selected,
                node.size,
            )
        };

        let is_editing = self.canvas.editing_node == Some(node_id);
        let screen_pos = world_to_screen_pos(transform, world_pos);
        let screen_size = world_size * zoom;
        let screen_bounds = Rect::from_min_size(screen_pos, screen_size);
        let interacting =
            was_selected && ui.input(|i| i.pointer.primary_down());
        if !is_editing
            && !interacting
            && !node_visible_on_canvas(canvas_rect, screen_bounds)
        {
            if let Some(node) = self.graph.node_weight_mut(node_id) {
                node.screen_rect = screen_bounds;
            }
            return;
        }

        let frame = if is_editing {
            style::node_edit_frame(is_comment)
        } else {
            style::node_frame(was_selected, is_comment)
        };

        let mut node_pos = self.graph[node_id].pos;
        let mut node_size = self.graph[node_id].size;
        let mut node_selected = was_selected;

        let area_id = Id::new(("pd_node", node_id));

        if !is_comment
            && !is_editing
            && ui.input(|i| i.modifiers.alt)
            && self.canvas.alt_drag_duplicate.is_none()
            && self.canvas.pending_wires.is_empty()
        {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                let prev_rect = self.graph[node_id].screen_rect;
                let on_body = prev_rect.is_positive() && prev_rect.contains(pointer);
                let on_port = self.find_inlet_at(pointer, transform).is_some()
                    || self.find_outlet_at(pointer, transform).is_some();
                if on_body
                    && !on_port
                    && ui.input(|i| i.pointer.is_decidedly_dragging())
                {
                    self.begin_alt_drag_duplicate(node_id);
                }
            }
        }

        let alt_drag_kind = self.alt_drag_node_kind(node_id);
        let is_alt_original = alt_drag_kind == AltDragNodeKind::Original;
        let is_alt_copy = alt_drag_kind == AltDragNodeKind::Copy;
        let is_drag_source = self.canvas.alt_drag_duplicate
            .as_ref()
            .is_some_and(|state| state.drag_source == node_id);
        let pinned_pos = self.canvas.alt_drag_duplicate
            .as_ref()
            .and_then(|state| state.originals.get(&node_id).copied());

        let area_order = if is_alt_copy {
            Order::Foreground
        } else if is_comment {
            Order::Background
        } else {
            Order::Middle
        };

        let shift = ui.input(|i| i.modifiers.shift);
        let movable = !is_comment && (!is_editing || shift) && !is_alt_original && !is_alt_copy;
        let resizable = !is_comment && !is_editing && !is_alt_original && !is_alt_copy;

        let object = self.graph[node_id].object.clone();
        let node_label = self.graph[node_id].label.clone();
        let library = &self.canvas.operator_library;
        let area_result = if is_editing {
            let mut edit_buffer = std::mem::take(&mut self.canvas.edit_buffer);
            let edit_size = world_size.max(estimate_text_box_size(&edit_buffer, &object));
            let screen_edit_size = edit_size * zoom;
            let result = show_node_area(
                ui.ctx(),
                area_id,
                screen_pos,
                screen_edit_size,
                &frame,
                area_order,
                canvas_rect,
                zoom,
                movable,
                false,
                |ui| object_ui::show_edit_ui(&object, ui, &mut edit_buffer, area_id, zoom, library),
            );
            self.canvas.edit_buffer = edit_buffer;
            node_size = world_size.max(estimate_text_box_size(&self.canvas.edit_buffer, &object));
            result
        } else {
            show_node_area(
                ui.ctx(),
                area_id,
                screen_pos,
                screen_size,
                &frame,
                area_order,
                canvas_rect,
                zoom,
                movable,
                resizable && was_selected,
                |ui| object_ui::show_display_ui(&object, ui, &node_label, was_selected, zoom),
            )
        };

        if is_editing {
            if area_result.body.commit_edit {
                self.stop_editing(true);
            } else if area_result.body.cancel_edit {
                self.stop_editing(false);
            }
        }

        let screen_rect = area_result.rect;
        if screen_rect.is_positive() {
            let world_rect = Rect::from_min_size(
                screen_to_world_pos(transform, screen_rect.min),
                screen_rect.size() / zoom,
            );

            if !is_editing
                && (area_result.body_response.hovered() || area_result.body_response.dragged())
            {
                paint_node_hover_highlight(patch_painter, world_rect, WORLD_ZOOM);
            }

            let alt = ui.input(|i| i.modifiers.alt);
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let hit_port = pointer.is_some_and(|p| {
                self.find_inlet_at(p, transform).is_some() || self.find_outlet_at(p, transform).is_some()
            });
            let hit_wire_handle =
                pointer.is_some_and(|p| self.pointer_on_wire_handle(p, transform));
            let hit_body = area_result.body_response.contains_pointer();

            if !is_editing {
                if ui.input(|i| i.pointer.primary_pressed())
                    && hit_body
                    && !hit_port
                    && !hit_wire_handle
                    && self.canvas.pending_wires.is_empty()
                {
                    self.canvas.node_pointer_press = Some(NodePointerPress {
                        node: node_id,
                        was_selected,
                    });
                }

                let label_clicked = area_result.body.clicked_label;
                let pointer_clicked =
                    ui.input(|i| i.pointer.primary_clicked()) && hit_body;
                let node_clicked = (label_clicked || pointer_clicked)
                    && !hit_port
                    && !hit_wire_handle
                    && self.canvas.pending_wires.is_empty();

                if node_clicked {
                    let was_at_press = self.canvas.node_pointer_press
                        .filter(|press| press.node == node_id)
                        .map(|press| press.was_selected)
                        .unwrap_or(was_selected);
                    let additive = ui.input(|i| i.modifiers.shift);
                    if additive {
                        node_selected = !node_selected;
                    } else if was_at_press && !alt {
                        self.start_editing(node_id);
                        node_selected = true;
                    } else if !alt {
                        self.stop_editing(true);
                        self.clear_all_selection();
                        node_selected = true;
                    }
                }

                if area_result.body_response.drag_started()
                    && !alt
                    && !shift
                    && !was_selected
                    && self.canvas.pending_wires.is_empty()
                {
                    let skip = ui.input(|i| i.pointer.interact_pos())
                        .is_some_and(|pointer| self.pointer_on_wire_handle(pointer, transform));
                    if !skip {
                        self.stop_editing(true);
                        self.clear_all_selection();
                        node_selected = true;
                    }
                }
            }

            if !is_comment && self.canvas.pending_wires.is_empty() {
                if area_result.body_response.drag_started() && !alt {
                    if is_editing {
                        self.stop_editing(true);
                    }
                    self.record_undo();
                    if shift {
                        let nodes = self.nodes_to_duplicate(node_id);
                        for &id in &nodes {
                            if self.node_shift_drag_insert_eligible(id) {
                                self.canvas.shift_drag_eligible.insert(id);
                            }
                        }
                        self.shift_drag_bridge_nodes(&nodes);
                    }
                }

                if area_result.body_response.drag_stopped()
                    && shift
                    && !alt
                {
                    if let Some(pointer) = pointer {
                        self.try_shift_drag_insert_on_wire(node_id, pointer, world_rect, transform);
                    }
                    self.canvas.shift_drag_splice_preview = None;
                }

                let drag_delta_world = area_result.body_response.drag_delta() / zoom;

                if is_alt_original {
                    if let Some(fixed_pos) = pinned_pos {
                        node_pos = fixed_pos;
                    }
                    if is_drag_source && ui.input(|i| i.pointer.primary_down()) {
                        let pointer_delta =
                            ui.input(|i| i.pointer.delta()) / transform.scaling;
                        if pointer_delta.length_sq() > 0.0 {
                            self.move_selected_by(pointer_delta);
                        }
                    }
                } else if is_alt_copy {
                    node_pos = self.graph[node_id].pos;
                } else if area_result.body_response.dragged() {
                    let group_drag = was_selected && self.selected_nodes().len() > 1;

                    if group_drag {
                        self.move_selected_by(drag_delta_world);
                        node_pos = self.graph[node_id].pos;
                    } else {
                        node_pos += drag_delta_world;
                    }
                }

                if area_result.resize_delta.length_sq() > 0.0 && !is_editing {
                    node_size += area_result.resize_delta / zoom;
                    node_size = node_size.max(NODE_MIN_SIZE);
                }

                if area_result.body_response.dragged() && shift && !alt {
                    if let Some(pointer) = pointer {
                        self.update_shift_drag_splice_preview(
                            node_id,
                            pointer,
                            world_rect,
                            transform,
                        );
                    }
                }
            }
        }

        if let Some(node) = self.graph.node_weight_mut(node_id) {
            node.pos = node_pos;
            node.size = node_size;
            node.selected = match alt_drag_kind {
                AltDragNodeKind::Original => false,
                AltDragNodeKind::Copy => true,
                AltDragNodeKind::None => node_selected,
            };
            node.screen_rect = screen_rect;
        }
    }

    fn connect_target_preview(
        &self,
        pointer: Pos2,
        transform: TSTransform,
    ) -> Option<(NodeId, usize, WireEndpoint)> {
        if self.canvas.pending_wires.is_empty() {
            return None;
        }
        let pending = self.canvas.pending_wires[0];
        match pending.end {
            WireEndpoint::Outlet => self
                .find_inlet_at(pointer, transform)
                .map(|(node, port)| (node, port, WireEndpoint::Inlet)),
            WireEndpoint::Inlet => self
                .find_outlet_at(pointer, transform)
                .map(|(node, port)| (node, port, WireEndpoint::Outlet)),
        }
    }

    fn port_highlight(
        &self,
        node_id: NodeId,
        port: usize,
        end: WireEndpoint,
        hovered: bool,
        pointer: Option<Pos2>,
        transform: TSTransform,
    ) -> PortHighlight {
        if let Some(preview) = self.canvas.shift_drag_splice_preview {
            if preview.node_id == node_id && port == 0 {
                return PortHighlight::ConnectTarget;
            }
        }
        if let Some(pending) = self.canvas.pending_wires.first() {
            if pending.node == node_id && pending.port == port && pending.end == end {
                return PortHighlight::Connecting;
            }
            if let Some(pointer) = pointer {
                if let Some((target_node, target_port, target_end)) =
                    self.connect_target_preview(pointer, transform)
                {
                    if target_node == node_id && target_port == port && target_end == end {
                        return PortHighlight::ConnectTarget;
                    }
                }
            }
        }
        if hovered {
            PortHighlight::Hovered
        } else {
            PortHighlight::None
        }
    }

    fn show_all_ports(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        transform: TSTransform,
    ) {
        let ctx = ui.ctx();
        let zoom = transform.scaling;
        let node_ids: Vec<NodeId> = self.graph.node_indices().collect();
        let pointer = ui.input(|i| i.pointer.hover_pos());

        let mut drag_start: Option<PendingWire> = None;

        for node_id in node_ids {
            let (node_rect, selected, inlets, outlets, is_comment) = {
                let node = &self.graph[node_id];
                (
                    node_layout_world_rect(node, transform),
                    node.selected,
                    node.object.inlets(),
                    node.object.outlets(),
                    node.object.is_comment(),
                )
            };

            if is_comment || !node_rect.is_positive() || (inlets == 0 && outlets == 0) {
                continue;
            }

            let screen_node = world_rect_to_screen(transform, node_rect);
            if !node_visible_on_canvas(canvas_rect, screen_node) {
                continue;
            }

            let inlet_ts = if self.graph[node_id].inlet_t.len() != inlets {
                default_port_ts(inlets)
            } else {
                self.graph[node_id].inlet_t.clone()
            };
            let outlet_ts = if self.graph[node_id].outlet_t.len() != outlets {
                default_port_ts(outlets)
            } else {
                self.graph[node_id].outlet_t.clone()
            };

            let mut inlet_positions = vec![Pos2::ZERO; inlets];
            let mut outlet_positions = vec![Pos2::ZERO; outlets];

            for i in 0..inlets {
                let port_id = Id::new(("port_in", node_id, i));
                let world_center = port_position_t(node_rect, inlet_ts[i], false, WORLD_ZOOM);
                let screen_center = world_to_screen_pos(transform, world_center);
                let highlight = self.port_highlight(
                    node_id,
                    i,
                    WireEndpoint::Inlet,
                    false,
                    pointer,
                    transform,
                );
                let response =
                    show_port_widget(ctx, port_id, screen_center, selected, highlight, canvas_rect, zoom);
                inlet_positions[i] = world_center;
                if response.drag_started() {
                    drag_start = Some(PendingWire {
                        node: node_id,
                        port: i,
                        end: WireEndpoint::Inlet,
                    });
                }
            }

            for i in 0..outlets {
                let port_id = Id::new(("port_out", node_id, i));
                let world_center = port_position_t(node_rect, outlet_ts[i], true, WORLD_ZOOM);
                let screen_center = world_to_screen_pos(transform, world_center);
                let highlight = self.port_highlight(
                    node_id,
                    i,
                    WireEndpoint::Outlet,
                    false,
                    pointer,
                    transform,
                );
                let response =
                    show_port_widget(ctx, port_id, screen_center, selected, highlight, canvas_rect, zoom);
                outlet_positions[i] = world_center;
                if response.drag_started() {
                    drag_start = Some(PendingWire {
                        node: node_id,
                        port: i,
                        end: WireEndpoint::Outlet,
                    });
                }
            }

            if let Some(node) = self.graph.node_weight_mut(node_id) {
                node.inlet_t = inlet_ts;
                node.outlet_t = outlet_ts;
                node.inlet_positions = inlet_positions;
                node.outlet_positions = outlet_positions;
            }
        }

        if let Some(start) = drag_start {
            self.canvas.pending_wires = vec![start];
            self.canvas.wire_drag_active = true;
        }
    }

    fn draw_wires_on_painter(&self, painter: &egui::Painter) {
        let omit_edge = self.canvas.shift_drag_splice_preview.map(|p| p.edge_id);
        for edge_id in self.graph.edge_indices() {
            if omit_edge == Some(edge_id) {
                continue;
            }
            if self.graph[edge_id].selected {
                continue;
            }
            let Some(points) = self.edge_bezier_points(edge_id) else {
                continue;
            };
            draw_bezier_wire(painter, points, false);
        }

        for edge_id in self.graph.edge_indices() {
            if omit_edge == Some(edge_id) {
                continue;
            }
            if !self.graph[edge_id].selected {
                continue;
            }
            let Some(points) = self.edge_bezier_points(edge_id) else {
                continue;
            };
            draw_bezier_wire(painter, points, true);
        }
    }

    fn draw_pending_wire(&self, painter: &egui::Painter, ctx: &Context, transform: TSTransform) {
        if self.canvas.pending_wires.is_empty() {
            return;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let Some(pointer) = pointer else {
            return;
        };
        let pointer_world = transform.inverse() * pointer;

        for pending in &self.canvas.pending_wires {
            let Some(node) = self.graph.node_weight(pending.node) else {
                continue;
            };
            let (from, from_is_outlet, to_is_inlet) = match pending.end {
                WireEndpoint::Outlet => (
                    socket_position(node, pending.port, true),
                    true,
                    true,
                ),
                WireEndpoint::Inlet => (
                    socket_position(node, pending.port, false),
                    false,
                    false,
                ),
            };
            let points = wire_bezier_points(from, from_is_outlet, pointer_world, to_is_inlet);
            draw_bezier_wire(painter, points, false);
        }
    }
}

impl Default for PatchCanvas {
    fn default() -> Self {
        Self {
            mouse_view: CanvasView::new(),
            pending_wires: Vec::new(),
            wire_drag_active: false,
            rewire_state: None,
            marquee: None,
            editing_node: None,
            edit_buffer: String::new(),
            edit_start_size: None,
            delay_pairs: HashMap::new(),
            debug_auto_combine: false,
            debug_auto_send_receive: false,
            debug_auto_delay: true,
            next_box_id: 1,
            alt_drag_duplicate: None,
            shift_drag_splice_preview: None,
            shift_drag_eligible: HashSet::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            node_pointer_press: None,
            operator_library: OperatorLibrary::load_preferred(),
            
        }
    }
}

struct NodeAreaShowResult {
    rect: Rect,
    body_response: egui::Response,
    resize_delta: Vec2,
    body: NodeAreaBody,
}

fn show_node_area(
    ctx: &Context,
    id: Id,
    screen_pos: Pos2,
    screen_size: Vec2,
    frame: &Frame,
    order: Order,
    canvas_clip: Rect,
    zoom: f32,
    movable: bool,
    resizable: bool,
    content: impl FnOnce(&mut Ui) -> NodeAreaBody,
) -> NodeAreaShowResult {
    let mut area_rect = Rect::NOTHING;
    let mut body_response = None;
    let mut resize_delta = Vec2::ZERO;
    let mut body = NodeAreaBody::default();
    let resize_grab = NODE_RESIZE_GRAB * zoom;

    Area::new(id)
        .fixed_pos(screen_pos)
        .order(order)
        .constrain(false)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(canvas_clip);
            let sense = if movable {
                Sense::click_and_drag()
            } else {
                Sense::click()
            };
            let (rect, response) = ui.allocate_exact_size(screen_size, sense);
            area_rect = rect;
            body_response = Some(response);

            let content_rect = rect - frame.total_margin();
            ui.painter().add(frame.paint(content_rect));
            let inner = content_rect;

            if resizable {
                let grab = Rect::from_min_size(
                    rect.right_bottom() - vec2(resize_grab, resize_grab),
                    vec2(resize_grab, resize_grab),
                );
                let resize_resp = ui.interact(grab, id.with("resize"), Sense::drag());
                if resize_resp.dragged() {
                    resize_delta = resize_resp.drag_delta();
                }
                if resize_resp.hovered() || resize_resp.dragged() {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
                }
            }

            body = ui
                .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                    ui.set_clip_rect(inner.intersect(canvas_clip));
                    content(ui)
                })
                .inner;
        });

    NodeAreaShowResult {
        rect: area_rect,
        body_response: body_response.expect("node area allocates a body response"),
        resize_delta,
        body,
    }
}


fn wire_handle_positions(points: [Pos2; 4], zoom: f32) -> [Pos2; 2] {
    let offset = port_size(zoom) * 2.4;
    let dir = points[3] - points[0];
    let axis = if dir.length_sq() > f32::EPSILON {
        dir.normalized()
    } else {
        vec2(0.0, 1.0)
    };
    [points[0] + axis * offset, points[3] - axis * offset]
}

fn wire_bezier_points(from: Pos2, from_is_outlet: bool, to: Pos2, to_is_inlet: bool) -> [Pos2; 4] {
    if (from.x - to.x).abs() < 0.5 {
        return [from, from, to, to];
    }
    let sag = ((to - from).length() * 0.35).clamp(10.0, 72.0);
    let from_tangent = if from_is_outlet {
        vec2(0.0, 1.0)
    } else {
        vec2(0.0, -1.0)
    };
    let to_tangent = if to_is_inlet {
        vec2(0.0, -1.0)
    } else {
        vec2(0.0, 1.0)
    };
    let cp1 = from + from_tangent * sag;
    let cp2 = to + to_tangent * sag;
    [from, cp1, cp2, to]
}

fn cubic_bezier_point(points: [Pos2; 4], t: f32) -> Pos2 {
    let t1 = 1.0 - t;
    let a = points[0].to_vec2() * (t1 * t1 * t1);
    let b = points[1].to_vec2() * (3.0 * t1 * t1 * t);
    let c = points[2].to_vec2() * (3.0 * t1 * t * t);
    let d = points[3].to_vec2() * (t * t * t);
    pos2(0.0, 0.0) + a + b + c + d
}

fn distance_point_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

fn distance_to_cubic_bezier(point: Pos2, points: [Pos2; 4]) -> f32 {
    const SAMPLES: usize = 32;
    let mut min_dist = point.distance(points[0]);
    let mut prev = points[0];
    for i in 1..=SAMPLES {
        let t = i as f32 / SAMPLES as f32;
        let sample = cubic_bezier_point(points, t);
        min_dist = min_dist.min(distance_point_to_segment(point, prev, sample));
        prev = sample;
    }
    min_dist
}

fn bezier_intersects_rect(points: [Pos2; 4], rect: Rect) -> bool {
    const SAMPLES: usize = 32;
    let mut prev = points[0];
    if rect.contains(prev) {
        return true;
    }
    for i in 1..=SAMPLES {
        let t = i as f32 / SAMPLES as f32;
        let sample = cubic_bezier_point(points, t);
        if rect.contains(sample) || segment_intersects_rect(prev, sample, rect) {
            return true;
        }
        prev = sample;
    }
    false
}

fn segment_intersects_rect(a: Pos2, b: Pos2, rect: Rect) -> bool {
    if rect.contains(a) || rect.contains(b) {
        return true;
    }
    let edges = [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ];
    for (p1, p2) in edges {
        if segments_intersect(a, b, p1, p2) {
            return true;
        }
    }
    false
}

fn segments_intersect(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    fn cross(a: Pos2, b: Pos2, c: Pos2) -> f32 {
        (b - a).x * (c - a).y - (b - a).y * (c - a).x
    }

    let d1 = cross(a, b, c);
    let d2 = cross(a, b, d);
    let d3 = cross(c, d, a);
    let d4 = cross(c, d, b);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    false
}

fn draw_bezier_wire(painter: &egui::Painter, points: [Pos2; 4], selected: bool) {
    let stroke = Stroke::new(
        if selected { CABLE_STROKE * 1.75 } else { CABLE_STROKE },
        PAPER,
    );

    painter.add(Shape::CubicBezier(CubicBezierShape {
        points,
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: stroke.into(),
    }));
}

fn draw_bezier_wire_colored(painter: &egui::Painter, points: [Pos2; 4], color: Color32) {
    let stroke = Stroke::new(CABLE_STROKE * 1.5, color);

    painter.add(Shape::CubicBezier(CubicBezierShape {
        points,
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: stroke.into(),
    }));
}

fn paint_wire_handle(painter: &egui::Painter, center: Pos2, hovered: bool, zoom: f32) {
    let size = port_size(zoom) * 1.1;
    let half = size * 0.5;
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    let fill = if hovered { WIRE_HANDLE_HOVER } else { WIRE_HANDLE };
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(LINE_W * 1.25, PAPER),
        egui::StrokeKind::Middle,
    );
    painter.circle_filled(center, half * 0.22, PAPER);
}

fn marquee_rect(marquee: MarqueeState) -> Rect {
    Rect::from_min_max(marquee.start.min(marquee.current), marquee.start.max(marquee.current))
}

fn draw_grid(painter: &egui::Painter, world_clip: Rect) {
    let step = GRID_STEP;
    if step < 1.0 {
        return;
    }

    let x0 = (world_clip.left() / step).floor() * step;
    let y0 = (world_clip.top() / step).floor() * step;

    let mut x = x0;
    while x <= world_clip.right() {
        let mut y = y0;
        while y <= world_clip.bottom() {
            if world_clip.contains(egui_pos2(x, y)) {
                painter.circle_filled(egui_pos2(x, y), 0.75, PAPER_DIM);
            }
            y += step;
        }
        x += step;
    }
}

fn show_wire_handle_widget(
    ctx: &Context,
    id: Id,
    screen_center: Pos2,
    combined: bool,
    canvas_clip: Rect,
    zoom: f32,
) -> egui::Response {
    let size = port_size(zoom) * if combined { 1.35 } else { 1.1 };
    let top_left = screen_center - Vec2::splat(size * 0.5);

    Area::new(id)
        .fixed_pos(top_left)
        .order(Order::Foreground)
        .constrain(false)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(canvas_clip);
            let (rect, response) = ui.allocate_exact_size(
                Vec2::splat(size),
                Sense::click_and_drag().union(Sense::hover()),
            );
            paint_wire_handle(ui.painter(), rect.center(), response.hovered(), zoom);
            response
        })
        .inner
}

fn show_port_widget(
    ctx: &Context,
    id: Id,
    screen_center: Pos2,
    selected: bool,
    highlight: PortHighlight,
    canvas_clip: Rect,
    zoom: f32,
) -> egui::Response {
    let size = port_size(zoom);
    let top_left = screen_center - Vec2::splat(size * 0.5);

    Area::new(id)
        .fixed_pos(top_left)
        .order(Order::Foreground)
        .constrain(false)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(canvas_clip);
            let (rect, response) = ui.allocate_exact_size(
                Vec2::splat(size),
                Sense::click_and_drag().union(Sense::hover()),
            );
            paint_port_square(ui.painter(), rect.center(), selected, highlight, zoom);
            response
        })
        .inner
}

fn node_draw_rect(node: &Node) -> Rect {
    Rect::from_min_size(node.pos, node.size)
}

fn socket_position(node: &Node, index: usize, is_outlet: bool) -> Pos2 {
    let positions = if is_outlet {
        &node.outlet_positions
    } else {
        &node.inlet_positions
    };

    if let Some(pos) = positions.get(index) {
        if pos.x.is_finite() && pos.y.is_finite() {
            return *pos;
        }
    }

    let rect = node_draw_rect(node);
    let ts = if is_outlet { &node.outlet_t } else { &node.inlet_t };
    let t = ts.get(index).copied().unwrap_or(0.0);
    port_position_t(rect, t, is_outlet, WORLD_ZOOM)
}

fn node_world_rect(node: &Node) -> Rect {
    Rect::from_min_size(node.pos, node.size)
}

fn node_layout_world_rect(node: &Node, transform: TSTransform) -> Rect {
    if node.screen_rect.is_positive() {
        rect_in_world_space(transform, node.screen_rect)
    } else {
        node_world_rect(node)
    }
}

fn world_rect_to_screen(transform: TSTransform, world: Rect) -> Rect {
    Rect::from_min_size(
        transform * world.min,
        world.size() * transform.scaling,
    )
}

fn screen_to_world_pos(transform: TSTransform, screen: Pos2) -> Pos2 {
    transform.inverse() * screen
}

fn world_to_screen_pos(transform: TSTransform, world: Pos2) -> Pos2 {
    transform * world
}

fn node_visible_on_canvas(canvas: Rect, item: Rect) -> bool {
    item.is_positive() && canvas.intersects(item)
}

fn node_visible_in_scene(world_clip: Rect, world_rect: Rect) -> bool {
    if !world_rect.is_positive() {
        return false;
    }
    world_clip.intersects(world_rect)
}

fn rect_in_world_space(transform: TSTransform, screen_rect: Rect) -> Rect {
    let inv = transform.inverse();
    Rect::from_points(&[
        inv * screen_rect.left_top(),
        inv * screen_rect.right_top(),
        inv * screen_rect.right_bottom(),
        inv * screen_rect.left_bottom(),
    ])
}
