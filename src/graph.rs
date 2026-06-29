use eframe::egui::{
    self, emath::TSTransform, pos2, vec2, Area, Color32, Context, CornerRadius, Id, Key, LayerId,
    Order, Pos2, Rect, RichText, Sense, Ui, Vec2,
};
use eframe::egui::{epaint::CubicBezierShape, epaint::Shape, pos2 as egui_pos2};
use eframe::egui::Stroke;
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableGraph;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::style::{
    self, default_port_ts, layout_job, label_font, paint_node_hover_highlight, paint_port_square,
    min_box_width, port_position_t, strip_brackets, PortHighlight, BOX_H, CABLE_STROKE, GRID_STEP,
    INK, LABEL_INSET_X, LINE_W, PAPER, PAPER_DIM, port_size, WIRE_HANDLE, WIRE_HANDLE_HOVER,
};

pub type NodeId = NodeIndex;
pub type EdgeId = EdgeIndex;
pub type PatchGraph = StableGraph<Node, EdgeData>;

const MARQUEE_MIN_AREA: f32 = 16.0;
const PATCH_BORDER_PAD: f32 = 100.0;
const MAX_UNDO_HISTORY: usize = 100;
/// Layout scale inside the patch scene (world coordinates). Camera zoom is applied separately.
const WORLD_ZOOM: f32 = 1.0;

#[derive(Clone, Debug)]
struct PatchSnapshot {
    graph: PatchGraph,
    delay_pairs: HashMap<NodeId, NodeId>,
    next_box_id: u64,
}

#[derive(Clone, Copy, Debug)]
struct MarqueeState {
    start: Pos2,
    current: Pos2,
    additive: bool,
    select_wires: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PdObject {
    OscTilde { freq: f32 },
    PlusTilde,
    MulTilde,
    DacTilde,
    Metro { ms: f32 },
    Random { max: i32 },
    FloatAtom { value: f32 },
    Message { text: String },
    Comment { text: String },
    In,
    Param,
    Out,
    DelayIn { id: Option<u8> },
    DelayOut { id: Option<u8> },
    Send { id: Option<u8> },
    Receive { id: Option<u8> },
    Combine,
}

impl PdObject {
    fn signal_label(kind: &str, id: Option<u8>) -> String {
        match id {
            Some(hex) => format!("{kind} #{hex:02X}"),
            None => kind.to_owned(),
        }
    }

    fn delay_label(kind: &str, id: Option<u8>) -> String {
        Self::signal_label(kind, id)
    }

    pub fn label(&self) -> String {
        match self {
            Self::OscTilde { freq } => format!("osc~ {freq}"),
            Self::PlusTilde => "+~".to_owned(),
            Self::MulTilde => "*~".to_owned(),
            Self::DacTilde => "dac~".to_owned(),
            Self::Metro { ms } => format!("metro {ms}"),
            Self::Random { max } => format!("random {max}"),
            Self::FloatAtom { value } => format!("{value:.3}"),
            Self::Message { text } => text.clone(),
            Self::Comment { text } => text.clone(),
            Self::In => "in".to_owned(),
            Self::Param => "param".to_owned(),
            Self::Out => "out".to_owned(),
            Self::DelayIn { id } => Self::delay_label("delay_in", *id),
            Self::DelayOut { id } => Self::delay_label("delay_out", *id),
            Self::Send { id } => Self::signal_label("send", *id),
            Self::Receive { id } => Self::signal_label("receive", *id),
            Self::Combine => "combine".to_owned(),
        }
    }

    pub fn bracketed_label(&self) -> String {
        match self {
            Self::Comment { text } => text.clone(),
            Self::Message { text } => format!("{text}"),
            Self::FloatAtom { .. } => self.label(),
            _ => format!("[{}]", self.label()),
        }
    }

    pub fn inlets(&self) -> usize {
        match self {
            Self::Comment { .. } | Self::Receive { .. } => 0,
            Self::In | Self::DelayIn { .. } => 0,
            Self::Combine => 2,
            Self::Send { .. } | Self::Out | Self::DelayOut { .. } => 1,
            _ => 1,
        }
    }

    pub fn outlets(&self) -> usize {
        match self {
            Self::Comment { .. } | Self::Send { .. } | Self::Out | Self::DacTilde => 0,
            Self::In | Self::Param | Self::DelayIn { .. } => 1,
            Self::Combine => 1,
            Self::Receive { .. } | Self::DelayOut { .. } => 1,
            _ => 1,
        }
    }

    pub fn is_comment(&self) -> bool {
        matches!(self, Self::Comment { .. })
    }

    pub fn is_send(&self) -> bool {
        matches!(self, Self::Send { .. })
    }

    pub fn is_receive(&self) -> bool {
        matches!(self, Self::Receive { .. })
    }

    pub fn signal_hex(&self) -> Option<u8> {
        match self {
            Self::DelayIn { id }
            | Self::DelayOut { id }
            | Self::Send { id }
            | Self::Receive { id } => *id,
            _ => None,
        }
    }

    pub fn is_number_box(&self) -> bool {
        matches!(self, Self::FloatAtom { .. })
    }

    pub fn edit_text(&self) -> String {
        match self {
            Self::Comment { text } | Self::Message { text } => text.clone(),
            Self::FloatAtom { value } => format!("{value}"),
            Self::OscTilde { freq } => format!("osc~ {freq}"),
            Self::Metro { ms } => format!("metro {ms}"),
            Self::Random { max } => format!("random {max}"),
            Self::PlusTilde => "+~".to_owned(),
            Self::MulTilde => "*~".to_owned(),
            Self::DacTilde => "dac~".to_owned(),
            Self::In => "in".to_owned(),
            Self::Param => "param".to_owned(),
            Self::Out => "out".to_owned(),
            Self::DelayIn { id } => Self::delay_label("delay_in", *id),
            Self::DelayOut { id } => Self::delay_label("delay_out", *id),
            Self::Send { id } => Self::signal_label("send", *id),
            Self::Receive { id } => Self::signal_label("receive", *id),
            Self::Combine => "combine".to_owned(),
        }
    }

    pub fn apply_edit_text(&mut self, text: &str) {
        if matches!(self, Self::Comment { .. }) {
            *self = Self::Comment {
                text: text.trim().to_owned(),
            };
            return;
        }
        *self = parse_pd_object_text(text);
    }

    /// Object label for `(node … :text …)` in `.lop` patch export.
    pub fn lop_text(&self, io_index: Option<usize>) -> String {
        match self {
            Self::In => format!("in {}", io_index.unwrap_or(1)),
            Self::Out => format!("out {}", io_index.unwrap_or(1)),
            Self::Param => format!("param {}", io_index.unwrap_or(1)),
            Self::PlusTilde => "+".to_owned(),
            Self::MulTilde => "*".to_owned(),
            Self::OscTilde { freq } => format!("osc~ {freq}"),
            Self::DacTilde => "dac~".to_owned(),
            Self::Metro { ms } => format!("metro {ms}"),
            Self::Random { max } => format!("random {max}"),
            Self::FloatAtom { value } => format!("{value}"),
            Self::Message { text } => text.clone(),
            Self::Comment { text } => text.clone(),
            Self::DelayIn { id } => Self::delay_label("delay_in", *id),
            Self::DelayOut { id } => Self::delay_label("delay_out", *id),
            Self::Send { id } => Self::signal_label("send", *id),
            Self::Receive { id } => Self::signal_label("receive", *id),
            Self::Combine => "combine".to_owned(),
        }
    }

    /// Optional `:bind` symbol for IO boxes in `.lop` patch export.
    pub fn lop_bind(&self, io_index: Option<usize>) -> Option<String> {
        match self {
            Self::In => Some(format!("_in_{}", io_index.unwrap_or(1))),
            Self::Out => Some(format!("_out_{}", io_index.unwrap_or(1))),
            Self::Param => Some(format!("_param_{}", io_index.unwrap_or(1))),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub object: PdObject,
    pub pos: Pos2,
    pub size: Vec2,
    /// Stable `obj-N` id for `.lop` patch export (matches fragment_interlay).
    pub box_id: Option<String>,
    pub screen_rect: Rect,
    pub inlet_t: Vec<f32>,
    pub outlet_t: Vec<f32>,
    pub inlet_positions: Vec<Pos2>,
    pub outlet_positions: Vec<Pos2>,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeData {
    pub from_port: usize,
    pub to_port: usize,
    pub selected: bool,
}

#[derive(Default)]
struct CanvasView {
    pan: Vec2,
    zoom: f32,
}

impl CanvasView {
    const MIN_ZOOM: f32 = 0.25;
    const MAX_ZOOM: f32 = 4.0;

    fn new() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
        }
    }

    fn world_to_screen(&self, origin: Pos2, world: Pos2) -> Pos2 {
        origin + self.pan + world.to_vec2() * self.zoom
    }

    fn screen_to_world(&self, origin: Pos2, screen: Pos2) -> Pos2 {
        Pos2::ZERO + (screen - origin - self.pan) / self.zoom
    }

    fn apply_pinch_zoom(&mut self, origin: Pos2, pointer: Pos2, zoom_delta: f32) {
        let factor = zoom_delta.clamp(Self::MIN_ZOOM / self.zoom, Self::MAX_ZOOM / self.zoom);
        if (factor - 1.0).abs() < f32::EPSILON {
            return;
        }
        let world_before = self.screen_to_world(origin, pointer);
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let world_after = self.screen_to_world(origin, pointer);
        self.pan += (world_after - world_before) * self.zoom;
    }

    fn apply_scroll_pan(&mut self, scroll_delta: Vec2) {
        if scroll_delta != Vec2::ZERO {
            self.pan += scroll_delta;
        }
    }

    /// Maps patch world coordinates to screen coordinates.
    fn canvas_transform(&self, origin: Pos2) -> TSTransform {
        TSTransform::from_translation(origin.to_vec2() + self.pan)
            * TSTransform::from_scaling(self.zoom)
    }
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

pub struct PdPatchEditor {
    graph: PatchGraph,
    view: CanvasView,
    layout_view: CanvasView,
    split_screen: bool,
    pending_wires: Vec<PendingWire>,
    wire_drag_active: bool,
    rewire_state: Option<WireRewireState>,
    context_menu: Option<(Pos2, Pos2)>,
    marquee: Option<MarqueeState>,
    editing_node: Option<NodeId>,
    edit_buffer: String,
    delay_pairs: HashMap<NodeId, NodeId>,
    layout_preview_cache: Option<(u64, crate::layout_adapter::LayoutPreview)>,
    debug_auto_combine: bool,
    debug_auto_send_receive: bool,
    debug_auto_delay: bool,
    patch_name: String,
    next_box_id: u64,
    alt_drag_duplicate: Option<AltDragDuplicate>,
    undo_stack: Vec<PatchSnapshot>,
    redo_stack: Vec<PatchSnapshot>,
    node_pointer_press: Option<NodePointerPress>,
}

impl PdPatchEditor {
    fn format_patch_lop(&self) -> String {
        crate::patch_export::export_patch(&self.graph, &self.patch_name)
    }

    pub fn demo_patch() -> Self {
        let mut editor = Self::default();

        let in0 = editor.add_object(PdObject::In, pos2(80.0, 80.0));
        let in1 = editor.add_object(PdObject::In, pos2(80.0, 180.0));
        let in2 = editor.add_object(PdObject::In, pos2(80.0, 280.0));

        let param0 = editor.add_object(PdObject::Param, pos2(200.0, 80.0));
        let param1 = editor.add_object(PdObject::Param, pos2(200.0, 180.0));

        let mul = editor.add_object(PdObject::MulTilde, pos2(320.0, 230.0));

        let out0 = editor.add_object(PdObject::Out, pos2(440.0, 80.0));
        let out1 = editor.add_object(PdObject::Out, pos2(440.0, 230.0));

        editor.connect_ports_unchecked(in0, 0, param0, 0);
        editor.connect_ports_unchecked(param0, 0, out0, 0);
        editor.connect_ports_unchecked(in1, 0, param1, 0);
        editor.connect_ports_unchecked(param1, 0, mul, 0);
        editor.connect_ports_unchecked(in2, 0, mul, 0);
        editor.connect_ports_unchecked(mul, 0, out1, 0);

        editor.clear_undo_history();
        editor
    }

    fn snapshot(&self) -> PatchSnapshot {
        PatchSnapshot {
            graph: self.graph.clone(),
            delay_pairs: self.delay_pairs.clone(),
            next_box_id: self.next_box_id,
        }
    }

    fn restore_snapshot(&mut self, snapshot: PatchSnapshot) {
        self.graph = snapshot.graph;
        self.delay_pairs = snapshot.delay_pairs;
        self.next_box_id = snapshot.next_box_id;
        self.invalidate_layout_preview();
        self.editing_node = None;
        self.edit_buffer.clear();
        self.pending_wires.clear();
        self.wire_drag_active = false;
        self.rewire_state = None;
        self.alt_drag_duplicate = None;
        self.context_menu = None;
        self.marquee = None;
        self.node_pointer_press = None;
    }

    fn clear_undo_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    fn record_undo(&mut self) {
        self.redo_stack.clear();
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.snapshot());
    }

    fn undo(&mut self) {
        let Some(snapshot) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
    }

    fn redo(&mut self) {
        let Some(snapshot) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
    }

    fn handle_undo_redo_input(&mut self, ui: &Ui) {
        if self.editing_node.is_some() {
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

    pub fn organize_layout(&mut self) {
        crate::layout_adapter::organize_patch(
            &mut self.graph,
            &patch_layout::LayoutConfig::default(),
        );
        self.invalidate_layout_preview();
    }

    fn invalidate_layout_preview(&mut self) {
        self.layout_preview_cache = None;
    }

    fn add_object(&mut self, object: PdObject, pos: Pos2) -> NodeId {
        let size = estimate_node_size(&object);
        let inlets = object.inlets();
        let outlets = object.outlets();
        let box_id = format!("obj-{}", self.next_box_id);
        self.next_box_id += 1;
        let id = self.graph.add_node(Node {
            object,
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
        self.invalidate_layout_preview();
        id
    }

    fn connect_ports(&mut self, from_node: NodeId, from_out: usize, to_node: NodeId, to_in: usize) {
        if self.rewire_state.is_none() {
            self.record_undo();
        }
        self.connect_ports_unchecked(from_node, from_out, to_node, to_in);
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

        if let Some((edge_id, existing_from, existing_out)) =
            self.find_edge_to_inlet(to_node, to_in)
        {
            if self.debug_auto_combine {
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
            if self.debug_auto_send_receive {
                if self.graph[existing_to].object.is_send() {
                    let Some(hex) = self.graph[existing_to].object.signal_hex() else {
                        return;
                    };
                    let receive = self.spawn_receive_near(to_node, to_in, hex);
                    self.force_connect_ports(receive, 0, to_node, to_in);
                    self.invalidate_layout_preview();
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
            self.invalidate_layout_preview();
            return;
        }

        if self.debug_auto_delay {
            self.connect_ports_through_delays(from_node, from_out, to_node, to_in);
            self.invalidate_layout_preview();
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
        self.invalidate_layout_preview();
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
        self.delay_pairs.insert(delay_out, delay_in);
        self.delay_pairs.insert(delay_in, delay_out);
    }

    fn handle_port_click(&mut self, pointer: Pos2, origin: Pos2) {
        self.stop_editing(true);

        if let Some(pending) = self.pending_wires.first().copied() {
            match pending.end {
                WireEndpoint::Outlet => {
                    if let Some((to_node, to_in)) = self.find_inlet_at(pointer, origin) {
                        self.connect_ports(pending.node, pending.port, to_node, to_in);
                        self.finish_rewire();
                        self.cancel_patching();
                        return;
                    }
                    if let Some((node_id, port)) = self.find_outlet_at(pointer, origin) {
                        self.pending_wires = vec![PendingWire {
                            node: node_id,
                            port,
                            end: WireEndpoint::Outlet,
                        }];
                        return;
                    }
                }
                WireEndpoint::Inlet => {
                    if let Some((from_node, from_out)) = self.find_outlet_at(pointer, origin) {
                        self.connect_ports(from_node, from_out, pending.node, pending.port);
                        self.finish_rewire();
                        self.cancel_patching();
                        return;
                    }
                    if let Some((node_id, port)) = self.find_inlet_at(pointer, origin) {
                        self.pending_wires = vec![PendingWire {
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

        if let Some((node_id, port)) = self.find_outlet_at(pointer, origin) {
            self.pending_wires = vec![PendingWire {
                node: node_id,
                port,
                end: WireEndpoint::Outlet,
            }];
        } else if let Some((node_id, port)) = self.find_inlet_at(pointer, origin) {
            self.pending_wires = vec![PendingWire {
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
            self.edit_buffer = node.object.edit_text();
            self.editing_node = Some(node_id);
        }
    }

    fn stop_editing(&mut self, commit: bool) {
        let Some(node_id) = self.editing_node.take() else {
            return;
        };

        if commit {
            if let Some(node) = self.graph.node_weight(node_id) {
                if self.edit_buffer != node.object.edit_text() {
                    self.record_undo();
                }
            }
            if let Some(node) = self.graph.node_weight_mut(node_id) {
                node.object.apply_edit_text(&self.edit_buffer);
                node.size = estimate_node_size(&node.object);
                self.sync_node_ports(node_id);
                self.invalidate_layout_preview();
            }
        }

        self.edit_buffer.clear();
    }

    fn remove_node(&mut self, id: NodeId) {
        if !self.graph.contains_node(id) {
            return;
        }

        if let Some(partner) = self.delay_pairs.remove(&id) {
            self.delay_pairs.remove(&partner);
            if partner != id && self.graph.contains_node(partner) {
                self.remove_node_internal(partner);
            }
        }

        self.remove_node_internal(id);
    }

    fn remove_node_internal(&mut self, id: NodeId) {
        if self.editing_node == Some(id) {
            self.editing_node = None;
            self.edit_buffer.clear();
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
        self.invalidate_layout_preview();
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
            self.invalidate_layout_preview();
        }
        self.cancel_patching();
    }

    fn apply_marquee_selection(&mut self, marquee: MarqueeState, origin: Pos2) {
        let marquee_rect = marquee_rect(marquee);
        if marquee.select_wires {
            if !marquee.additive {
                self.clear_wire_selection();
            }
            let selected_edges: Vec<EdgeId> = self
                .graph
                .edge_indices()
                .filter(|&edge_id| self.edge_intersects_rect(edge_id, marquee_rect, origin))
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

    fn socket_position_for(
        &self,
        node_id: NodeId,
        port: usize,
        is_outlet: bool,
        origin: Pos2,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Pos2 {
        let node = &self.graph[node_id];
        let rect = self.node_screen_rect_for(node_id, origin, view, preview);
        let t = if is_outlet {
            let count = node.object.outlets();
            if node.outlet_t.len() == count {
                node.outlet_t.get(port).copied()
            } else {
                default_port_ts(count).get(port).copied()
            }
        } else {
            let count = node.object.inlets();
            if node.inlet_t.len() == count {
                node.inlet_t.get(port).copied()
            } else {
                default_port_ts(count).get(port).copied()
            }
        }
        .unwrap_or(0.0);
        port_position_t(rect, t, is_outlet, view.zoom)
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

    fn edge_bezier_points_for(
        &self,
        edge_id: EdgeId,
        origin: Pos2,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Option<[Pos2; 4]> {
        let (from_id, to_id) = self.graph.edge_endpoints(edge_id)?;
        let edge = &self.graph[edge_id];
        let from = self.socket_position_for(from_id, edge.from_port, true, origin, view, preview);
        let to = self.socket_position_for(to_id, edge.to_port, false, origin, view, preview);
        Some(wire_bezier_points(from, true, to, true))
    }

    fn wire_hit_radius(&self) -> f32 {
        (CABLE_STROKE * WORLD_ZOOM * 2.5).max(8.0 / self.view.zoom)
    }

    fn find_edge_at(&self, pointer: Pos2, origin: Pos2) -> Option<EdgeId> {
        let pointer_world = self.view.screen_to_world(origin, pointer);
        let hit = self.wire_hit_radius();
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
        if self.rewire_state.is_some() {
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

    fn handle_center_for_group(&self, group: &WireHandleGroup) -> Option<Pos2> {
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
        let rect = node_world_rect(node);
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

    fn find_wire_handle_at(&self, pointer: Pos2, origin: Pos2) -> Option<WireHandleGroup> {
        let pointer_world = self.view.screen_to_world(origin, pointer);
        let hit = self.wire_handle_hit_radius();
        let mut best: Option<(WireHandleGroup, f32)> = None;

        for group in self.wire_handle_groups() {
            let Some(center) = self.handle_center_for_group(&group) else {
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

    fn pointer_on_wire_handle(&self, pointer: Pos2, origin: Pos2) -> bool {
        self.find_wire_handle_at(pointer, origin).is_some()
    }

    fn show_wire_handles(&mut self, ui: &mut Ui, transform: TSTransform) {
        if self.rewire_state.is_some() {
            return;
        }

        let ctx = ui.ctx();
        let groups = self.wire_handle_groups();

        for group in groups {
            let Some(center) = self.handle_center_for_group(&group) else {
                continue;
            };
            let combined = group.edge_ids.len() > 1;
            let end_tag = match group.end {
                WireEndpoint::Outlet => 0u8,
                WireEndpoint::Inlet => 1u8,
            };
            let response = show_wire_handle_widget(
                ctx,
                Id::new(("wire_handle", group.node.index(), group.port, end_tag)),
                center,
                transform,
                combined,
            );
            let pressed = response.is_pointer_button_down_on()
                && ui.input(|i| i.pointer.primary_pressed());
            if (response.drag_started() || pressed) && self.pending_wires.is_empty() {
                self.stop_editing(true);
                self.start_group_rewire_from_handle(&group);
            }
        }
    }

    fn finish_rewire(&mut self) {
        self.rewire_state = None;
    }

    fn edge_intersects_rect(&self, edge_id: EdgeId, rect: Rect, origin: Pos2) -> bool {
        let Some(points) = self.edge_bezier_points(edge_id) else {
            return false;
        };
        let transform = self.view.canvas_transform(origin);
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
            Self::remap_copied_signal_hex(&mut object, &hex_remap);
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
            let Some(partner_old) = self.delay_pairs.get(&old).copied() else {
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

        self.alt_drag_duplicate = Some(AltDragDuplicate {
            originals,
            copies: id_map.values().copied().collect(),
            drag_source: dragged,
        });
        self.stop_editing(true);
    }

    fn alt_drag_node_kind(&self, node_id: NodeId) -> AltDragNodeKind {
        let Some(state) = &self.alt_drag_duplicate else {
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
}

impl Default for PdPatchEditor {
    fn default() -> Self {
        Self {
            graph: PatchGraph::default(),
            view: CanvasView::new(),
            layout_view: CanvasView::new(),
            split_screen: false,
            pending_wires: Vec::new(),
            wire_drag_active: false,
            rewire_state: None,
            context_menu: None,
            marquee: None,
            editing_node: None,
            edit_buffer: String::new(),
            delay_pairs: HashMap::new(),
            layout_preview_cache: None,
            debug_auto_combine: false,
            debug_auto_send_receive: false,
            debug_auto_delay: true,
            patch_name: "patch".to_owned(),
            next_box_id: 1,
            alt_drag_duplicate: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            node_pointer_press: None,
        }
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
                        ui.checkbox(&mut self.debug_auto_combine, "Auto combine");
                        ui.checkbox(&mut self.debug_auto_send_receive, "Auto send/receive");
                        ui.checkbox(&mut self.debug_auto_delay, "Auto delay");
                    });
                    ui.checkbox(&mut self.split_screen, "Split view");
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
                if self.split_screen {
                    let preview = crate::layout_adapter::layout_preview_cached(
                        &self.graph,
                        &mut self.layout_preview_cache,
                    )
                    .clone();

                    egui::Panel::right("organized_layout_pane")
                        .resizable(true)
                        .min_size(200.0)
                        .default_size(ui.available_width() * 0.5)
                        .frame(egui::Frame {
                            fill: INK,
                            inner_margin: egui::Margin::ZERO,
                            ..Default::default()
                        })
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Sorted layout").strong());
                            });
                            ui.separator();
                            self.canvas_preview_ui(ui, &preview);
                        });

                    egui::CentralPanel::default()
                        .frame(egui::Frame {
                            fill: INK,
                            inner_margin: egui::Margin::ZERO,
                            ..Default::default()
                        })
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Raw patch").strong());
                            });
                            ui.separator();
                            self.canvas_ui(ui, true);
                        });
                } else {
                    self.canvas_ui(ui, true);
                }
            });

        self.object_menu_ui(ui.ctx());
    }
}

impl PdPatchEditor {
    fn canvas_ui(&mut self, ui: &mut Ui, editable: bool) {
        let canvas_rect = ui.max_rect();
        let origin = canvas_rect.min;

        let response = ui.allocate_rect(canvas_rect, Sense::click_and_drag());
        if editable {
            Self::handle_canvas_view_input(
                ui,
                canvas_rect,
                origin,
                &response,
                &mut self.view,
            );
            self.handle_canvas_editing(ui, canvas_rect, origin, &response);
        }

        let transform = self.view.canvas_transform(origin);
        let patch_paint_layer = LayerId::new(Order::Background, ui.id().with("patch_paint"));
        ui.ctx().set_transform_layer(patch_paint_layer, transform);
        let patch_painter = ui.ctx().layer_painter(patch_paint_layer);
        let world_clip = rect_in_world_space(transform, canvas_rect);
        draw_grid(&patch_painter, world_clip);

        let mut node_order: Vec<NodeId> = self.graph.node_indices().collect();
        node_order.sort_by_key(|id| self.graph[*id].object.is_comment());

        if editable {
            let clear_alt_drag = ui.input(|i| i.pointer.primary_released());
            if self.alt_drag_duplicate.is_some() {
                let drag_source = self.alt_drag_duplicate.as_ref().unwrap().drag_source;
                node_order.sort_by_key(|id| (*id == drag_source) as u8);
            }
            for node_id in node_order.clone() {
                self.show_pd_node(ui, node_id, origin, transform);
            }
            if clear_alt_drag {
                self.alt_drag_duplicate = None;
            }
            if ui.input(|i| i.pointer.primary_released()) {
                self.node_pointer_press = None;
            }
            self.show_all_ports(ui, origin, transform);
            self.draw_patch_border(&patch_painter);
            self.draw_wires_on_painter(&patch_painter, None);
            self.show_wire_handles(ui, transform);
            self.draw_pending_wire(&patch_painter, ui.ctx(), transform);
            self.draw_marquee(ui, canvas_rect);
            self.handle_patch_cable_input(ui, canvas_rect, origin);
            self.finish_marquee(ui, origin);

            if response.secondary_clicked() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    if self.is_background_pointer(canvas_rect, pointer, origin) {
                        let world = self.view.screen_to_world(origin, pointer);
                        self.context_menu = Some((pointer, world));
                    }
                }
            }
        }
    }

    fn canvas_preview_ui(&mut self, ui: &mut Ui, preview: &crate::layout_adapter::LayoutPreview) {
        let canvas_rect = ui.max_rect();
        let origin = canvas_rect.min;

        let response = ui.allocate_rect(canvas_rect, Sense::click_and_drag());
        Self::handle_canvas_view_input(
            ui,
            canvas_rect,
            origin,
            &response,
            &mut self.layout_view,
        );

        let painter = ui.painter_at(canvas_rect);
        draw_grid_screen(&painter, canvas_rect, &self.layout_view, origin);

        let mut node_order: Vec<NodeId> = self.graph.node_indices().collect();
        node_order.sort_by_key(|id| self.graph[*id].object.is_comment());

        for node_id in node_order {
            self.paint_node_readonly(ui, node_id, origin, &self.layout_view, Some(preview));
        }
        self.paint_ports_readonly(ui, &self.layout_view, Some(preview));
        self.draw_patch_border_for(ui, &self.layout_view, Some(preview));
        self.draw_wires(ui, canvas_rect, &self.layout_view, Some(preview));
    }

    fn handle_canvas_view_input(
        ui: &mut Ui,
        canvas_rect: Rect,
        origin: Pos2,
        response: &egui::Response,
        view: &mut CanvasView,
    ) {
        let pointer_over_canvas = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| canvas_rect.contains(p));

        if pointer_over_canvas {
            let pointer = ui.input(|i| i.pointer.hover_pos()).unwrap_or(origin);

            let zoom_delta = ui.input(|i| i.zoom_delta());
            if zoom_delta != 1.0 {
                view.apply_pinch_zoom(origin, pointer, zoom_delta);
            }

            let scroll = ui.input(|i| i.smooth_scroll_delta());
            view.apply_scroll_pan(scroll);
        }

        let panning = response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary)
                && ui.input(|i| i.modifiers.command));

        if panning {
            view.pan += response.drag_delta();
        }
    }

    fn world_pos_for_node(
        &self,
        node_id: NodeId,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Pos2 {
        if let Some(preview) = preview {
            if let Some(pos) = preview.positions.get(&node_id.index()) {
                return *pos;
            }
        }
        self.graph[node_id].pos
    }

    fn node_size_for_preview(
        &self,
        node_id: NodeId,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Vec2 {
        if let Some(preview) = preview {
            if let Some(size) = preview.sizes.get(&node_id.index()) {
                return *size;
            }
        }
        self.graph[node_id].size
    }

    fn node_screen_rect_for(
        &self,
        node_id: NodeId,
        origin: Pos2,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Rect {
        let world = self.world_pos_for_node(node_id, preview);
        let size = self.node_size_for_preview(node_id, preview);
        let screen_pos = view.world_to_screen(origin, world);
        Rect::from_min_size(screen_pos, size * view.zoom)
    }

    fn paint_node_readonly(
        &self,
        ui: &mut Ui,
        node_id: NodeId,
        origin: Pos2,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) {
        let node = &self.graph[node_id];
        let rect = self.node_screen_rect_for(node_id, origin, view, preview);
        let is_comment = node.object.is_comment();
        let label = node.object.bracketed_label();
        let zoom = view.zoom;
        let painter = ui.painter_at(ui.max_rect());

        if is_comment {
            let font = label_font(zoom);
            let job = layout_job(&label, font, false);
            let galley = painter.layout_job(job);
            painter.galley(
                pos2(rect.min.x, rect.center().y - galley.size().y * 0.5),
                galley,
                PAPER_DIM,
            );
            return;
        }

        let frame = style::node_frame(false, false);
        painter.add(frame.paint(rect));

        let font = label_font(zoom);
        let job = layout_job(&label, font, false);
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(
                rect.min.x + LABEL_INSET_X * zoom,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            PAPER,
        );
    }

    fn paint_ports_readonly(
        &self,
        ui: &mut Ui,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) {
        let painter = ui.painter_at(ui.max_rect());
        let zoom = view.zoom;
        let origin = ui.max_rect().min;

        for node_id in self.graph.node_indices() {
            let node = &self.graph[node_id];
            if node.object.is_comment() {
                continue;
            }

            let rect = self.node_screen_rect_for(node_id, origin, view, preview);
            let inlets = node.object.inlets();
            let outlets = node.object.outlets();
            if inlets == 0 && outlets == 0 {
                continue;
            }

            let inlet_ts = default_port_ts(inlets);
            let outlet_ts = default_port_ts(outlets);

            for i in 0..inlets {
                let center = port_position_t(rect, inlet_ts[i], false, zoom);
                paint_port_square(&painter, center, false, PortHighlight::None, zoom);
            }
            for i in 0..outlets {
                let center = port_position_t(rect, outlet_ts[i], true, zoom);
                paint_port_square(&painter, center, false, PortHighlight::None, zoom);
            }
        }
    }

    fn draw_patch_border_for(
        &self,
        ui: &mut Ui,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) {
        let origin = ui.max_rect().min;
        let mut bounds = Rect::NOTHING;
        let mut any = false;

        for node_id in self.graph.node_indices() {
            let rect = self.node_screen_rect_for(node_id, origin, view, preview);
            if !rect.is_positive() {
                continue;
            }
            bounds = if any { bounds.union(rect) } else { rect };
            any = true;
        }

        let Some(bounds) = any.then_some(bounds) else {
            return;
        };

        let painter = ui.painter_at(ui.max_rect());
        let border = bounds.expand(PATCH_BORDER_PAD * view.zoom);
        painter.rect_stroke(
            border,
            CornerRadius::ZERO,
            Stroke::new(LINE_W, PAPER_DIM),
            egui::StrokeKind::Outside,
        );
    }

    fn handle_canvas_editing(
        &mut self,
        ui: &mut Ui,
        canvas_rect: Rect,
        origin: Pos2,
        response: &egui::Response,
    ) {
        self.handle_undo_redo_input(ui);

        if ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            self.delete_selected();
        }

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            if self.editing_node.is_some() {
                self.stop_editing(false);
            } else {
                self.cancel_patching();
                self.context_menu = None;
            }
        }

        let box_selecting = response.dragged_by(egui::PointerButton::Primary)
            && !ui.input(|i| i.modifiers.command)
            && self.pending_wires.is_empty();

        if box_selecting {
            if response.drag_started() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    if !self.pointer_on_node(pointer) {
                        self.marquee = Some(MarqueeState {
                            start: pointer,
                            current: pointer,
                            additive: ui.input(|i| i.modifiers.shift),
                            select_wires: ui.input(|i| i.modifiers.alt),
                        });
                        self.cancel_patching();
                    }
                }
            }

            if let Some(marquee) = &mut self.marquee {
                if let Some(pointer) = response.interact_pointer_pos() {
                    marquee.current = pointer;
                }
            }
        }

        if response.double_clicked() && !response.secondary_clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if self.is_background_pointer(canvas_rect, pointer, origin) {
                    let world = self.view.screen_to_world(origin, pointer);
                    self.context_menu = Some((pointer, world));
                }
            }
        } else if response.clicked() && !response.secondary_clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if self.is_background_pointer(canvas_rect, pointer, origin) {
                    self.stop_editing(true);
                    self.clear_all_selection();
                    self.cancel_patching();
                }
            }
        }
    }

    fn is_background_pointer(&self, canvas_rect: Rect, pointer: Pos2, origin: Pos2) -> bool {
        canvas_rect.contains(pointer)
            && !self.pointer_on_node_or_port(pointer, origin)
            && self.find_edge_at(pointer, origin).is_none()
    }

    fn finish_marquee(&mut self, ui: &mut Ui, origin: Pos2) {
        if !ui.input(|i| i.pointer.primary_released()) {
            return;
        }

        let Some(marquee) = self.marquee.take() else {
            return;
        };

        let rect = marquee_rect(marquee);
        if rect.area() >= MARQUEE_MIN_AREA {
            self.apply_marquee_selection(marquee, origin);
        }
    }

    fn draw_marquee(&self, ui: &mut Ui, canvas_rect: Rect) {
        let Some(marquee) = self.marquee else {
            return;
        };

        let rect = marquee_rect(marquee);
        let painter = ui.painter_at(canvas_rect);
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 28),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(LINE_W, Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 200)),
            egui::StrokeKind::Outside,
        );
    }

    fn cancel_patching(&mut self) {
        if let Some(rewire) = self.rewire_state.take() {
            self.restore_rewired_edges(rewire);
        }
        self.pending_wires.clear();
        self.wire_drag_active = false;
    }

    fn restore_rewired_edges(&mut self, rewire: WireRewireState) {
        for edge in rewire.edges {
            self.force_connect_ports(edge.from, edge.from_port, edge.to, edge.to_port);
        }
        self.invalidate_layout_preview();
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

        self.rewire_state = Some(WireRewireState {
            edges: saved,
            dragged_end: group.end,
        });
        self.pending_wires = pending;
        self.invalidate_layout_preview();
        self.wire_drag_active = true;
    }

    fn pointer_on_node(&self, pointer: Pos2) -> bool {
        self.graph
            .node_weights()
            .any(|node| node.screen_rect.is_positive() && node.screen_rect.contains(pointer))
    }

    fn pointer_on_node_or_port(&self, pointer: Pos2, origin: Pos2) -> bool {
        self.pointer_on_node(pointer)
            || self.pointer_on_wire_handle(pointer, origin)
            || self.find_inlet_at(pointer, origin).is_some()
            || self.find_outlet_at(pointer, origin).is_some()
    }

    fn find_inlet_at(&self, pointer: Pos2, origin: Pos2) -> Option<(NodeId, usize)> {
        let pointer_world = self.view.screen_to_world(origin, pointer);
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

    fn find_outlet_at(&self, pointer: Pos2, origin: Pos2) -> Option<(NodeId, usize)> {
        let pointer_world = self.view.screen_to_world(origin, pointer);
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

    fn handle_patch_cable_input(&mut self, ui: &mut Ui, canvas_rect: Rect, origin: Pos2) {
        if !self.pending_wires.is_empty()
            && ui.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging())
        {
            self.wire_drag_active = true;
        }

        if ui.input(|i| i.pointer.primary_clicked()) {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                if canvas_rect.contains(pointer) {
                    if self.find_inlet_at(pointer, origin).is_some()
                        || self.find_outlet_at(pointer, origin).is_some()
                    {
                        self.handle_port_click(pointer, origin);
                    } else if let Some(edge_id) = self.find_edge_at(pointer, origin) {
                        let additive = ui.input(|i| i.modifiers.shift);
                        self.handle_wire_click(edge_id, additive);
                    } else if self.is_background_pointer(canvas_rect, pointer, origin) {
                        self.stop_editing(true);
                        self.cancel_patching();
                    }
                }
            }
        }

        if ui.input(|i| i.pointer.primary_released()) && !self.pending_wires.is_empty() {
            let dragging = ui.input(|i| i.pointer.is_decidedly_dragging()) || self.wire_drag_active;
            let mut connected = false;
            if dragging {
                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                    let rewire = self.rewire_state.clone();
                    if let Some(rewire) = rewire {
                        match rewire.dragged_end {
                            WireEndpoint::Inlet => {
                                if let Some((to_node, to_in)) = self.find_inlet_at(pointer, origin) {
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
                                if let Some((from_node, from_out)) = self.find_outlet_at(pointer, origin) {
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
                    } else if let Some(pending) = self.pending_wires.first().copied() {
                        match pending.end {
                            WireEndpoint::Outlet => {
                                if let Some((to_node, to_in)) = self.find_inlet_at(pointer, origin) {
                                    self.connect_ports(pending.node, pending.port, to_node, to_in);
                                    connected = true;
                                }
                            }
                            WireEndpoint::Inlet => {
                                if let Some((from_node, from_out)) = self.find_outlet_at(pointer, origin) {
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
                self.pending_wires.clear();
                self.wire_drag_active = false;
            } else if dragging || self.rewire_state.is_some() {
                self.cancel_patching();
            }
        }
    }

    fn show_pd_node(&mut self, ui: &mut Ui, node_id: NodeId, origin: Pos2, transform: TSTransform) {
        let (world_pos, is_comment, label, was_selected, world_size) = {
            let node = &self.graph[node_id];
            (
                node.pos,
                node.object.is_comment(),
                node.object.bracketed_label(),
                node.selected,
                node.size,
            )
        };

        let is_editing = self.editing_node == Some(node_id);

        let frame = if is_editing {
            style::node_edit_frame(is_comment)
        } else {
            style::node_frame(was_selected, is_comment)
        };

        let mut node_pos = self.graph[node_id].pos;
        let mut node_size = self.graph[node_id].size;
        let mut node_selected = was_selected;
        let mut screen_rect = Rect::NOTHING;

        let window_id = Id::new(("pd_node", node_id));

        let is_editing = self.editing_node == Some(node_id);

        if !is_comment
            && !is_editing
            && ui.input(|i| i.modifiers.alt)
            && self.alt_drag_duplicate.is_none()
            && self.pending_wires.is_empty()
        {
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                let prev_rect = self.graph[node_id].screen_rect;
                let on_body = prev_rect.is_positive() && prev_rect.contains(pointer);
                let on_port = self.find_inlet_at(pointer, origin).is_some()
                    || self.find_outlet_at(pointer, origin).is_some();
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
        let is_drag_source = self
            .alt_drag_duplicate
            .as_ref()
            .is_some_and(|state| state.drag_source == node_id);
        let pinned_pos = self
            .alt_drag_duplicate
            .as_ref()
            .and_then(|state| state.originals.get(&node_id).copied());

        let window_order = if is_alt_copy {
            Order::Foreground
        } else if is_comment {
            Order::Background
        } else {
            Order::Middle
        };

        let mut window = egui::containers::Window::new(&label)
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .scroll(false)
            .movable(!is_comment && !is_editing && !is_alt_original && !is_alt_copy)
            .constrain(false)
            .interactable(true)
            .fade_in(false)
            .order(window_order)
            .frame(frame)
            .current_pos(world_pos);

        if is_comment && !is_editing {
            window = window.auto_sized().resizable(false);
        } else if is_editing {
            window = window
                .resizable(false)
                .default_size(world_size)
                .min_size(world_size);
        } else {
            window = window
                .resizable(true)
                .min_size(vec2(48.0, BOX_H))
                .default_size(world_size);
        }

        set_widget_layer_transform(ui.ctx(), window_order, window_id, transform);

        let window_response = if is_editing {
            window.show(ui.ctx(), |ui| {
                node_edit_body(
                    ui,
                    window_id,
                    &mut self.edit_buffer,
                    is_comment,
                    world_size,
                    WORLD_ZOOM,
                )
            })
        } else {
            window.show(ui.ctx(), |ui| {
                node_window_body(ui, window_id, &label, is_comment, was_selected, WORLD_ZOOM)
            })
        };

        if is_editing {
            if let Some(inner) = &window_response {
                if let Some(body) = &inner.inner {
                    if body.commit_edit {
                        self.stop_editing(true);
                    } else if body.cancel_edit {
                        self.stop_editing(false);
                    }
                }
            }
        }

        if let Some(inner) = window_response {
            let rect = inner.response.rect;
            if is_editing {
                screen_rect = Rect::from_min_size(
                    self.view.world_to_screen(origin, node_pos),
                    world_size * self.view.zoom,
                );
            } else {
                screen_rect = rect;
            }

            if !is_editing && (inner.response.hovered() || inner.response.dragged()) {
                paint_node_hover_highlight(
                    &ui.painter_at(ui.max_rect()),
                    rect,
                    self.view.zoom,
                );
            }

            if !is_editing {
                let alt = ui.input(|i| i.modifiers.alt);
                let pointer = ui.input(|i| i.pointer.interact_pos());
                let hit_port = pointer.is_some_and(|p| {
                    self.find_inlet_at(p, origin).is_some() || self.find_outlet_at(p, origin).is_some()
                });
                let hit_wire_handle =
                    pointer.is_some_and(|p| self.pointer_on_wire_handle(p, origin));
                let hit_body = pointer.is_some_and(|p| rect.contains(p));

                if ui.input(|i| i.pointer.primary_pressed())
                    && hit_body
                    && !hit_port
                    && !hit_wire_handle
                    && self.pending_wires.is_empty()
                {
                    self.node_pointer_press = Some(NodePointerPress {
                        node: node_id,
                        was_selected,
                    });
                }

                let label_clicked = inner
                    .inner
                    .as_ref()
                    .is_some_and(|body| body.clicked_label);
                let pointer_clicked =
                    ui.input(|i| i.pointer.primary_clicked()) && hit_body;
                let node_clicked = (label_clicked || pointer_clicked)
                    && !hit_port
                    && !hit_wire_handle
                    && self.pending_wires.is_empty();

                if node_clicked {
                    let was_at_press = self
                        .node_pointer_press
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

                if inner.response.drag_started()
                    && !alt
                    && !is_comment
                    && self.pending_wires.is_empty()
                {
                    self.record_undo();
                }

                let drag_delta_world = inner.response.drag_delta();

                if is_alt_original {
                    if let Some(fixed_pos) = pinned_pos {
                        node_pos = fixed_pos;
                    }
                    if is_drag_source && ui.input(|i| i.pointer.primary_down()) {
                        let pointer_delta =
                            ui.input(|i| i.pointer.delta()) / self.view.zoom;
                        if pointer_delta.length_sq() > 0.0 {
                            self.move_selected_by(pointer_delta);
                        }
                    }
                } else if is_alt_copy {
                    node_pos = self.graph[node_id].pos;
                } else {
                    let group_drag = inner.response.dragged()
                        && was_selected
                        && self.selected_nodes().len() > 1;

                    if group_drag {
                        self.move_selected_by(drag_delta_world);
                        node_pos = self.graph[node_id].pos;
                    } else {
                        node_pos = self.view.screen_to_world(origin, rect.min);
                    }
                }

                if !is_comment {
                    node_size = rect.size() / self.view.zoom;
                }

                if inner.response.drag_started()
                    && !alt
                    && !ui.input(|i| i.modifiers.shift)
                    && !was_selected
                    && self.pending_wires.is_empty()
                {
                    let skip = ui.input(|i| i.pointer.interact_pos())
                        .is_some_and(|pointer| self.pointer_on_wire_handle(pointer, origin));
                    if !skip {
                        self.stop_editing(true);
                        self.clear_all_selection();
                        node_selected = true;
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
        origin: Pos2,
    ) -> Option<(NodeId, usize, WireEndpoint)> {
        if self.pending_wires.is_empty() {
            return None;
        }
        let pending = self.pending_wires[0];
        match pending.end {
            WireEndpoint::Outlet => self
                .find_inlet_at(pointer, origin)
                .map(|(node, port)| (node, port, WireEndpoint::Inlet)),
            WireEndpoint::Inlet => self
                .find_outlet_at(pointer, origin)
                .map(|(node, port)| (node, port, WireEndpoint::Outlet)),
        }
    }

    fn port_highlight(
        &self,
        node_id: NodeId,
        port: usize,
        end: WireEndpoint,
        pointer: Option<Pos2>,
        origin: Pos2,
    ) -> PortHighlight {
        if let Some(pending) = self.pending_wires.first() {
            if pending.node == node_id && pending.port == port && pending.end == end {
                return PortHighlight::Connecting;
            }
            if let Some(pointer) = pointer {
                if let Some((target_node, target_port, target_end)) =
                    self.connect_target_preview(pointer, origin)
                {
                    if target_node == node_id && target_port == port && target_end == end {
                        return PortHighlight::ConnectTarget;
                    }
                }
            }
        }
        PortHighlight::None
    }

    fn show_all_ports(&mut self, ui: &mut Ui, origin: Pos2, transform: TSTransform) {
        let ctx = ui.ctx();
        let node_ids: Vec<NodeId> = self.graph.node_indices().collect();
        let pointer = ui.input(|i| i.pointer.hover_pos());

        let mut drag_start: Option<PendingWire> = None;

        for node_id in node_ids {
            let (node_rect, selected, inlets, outlets, is_comment) = {
                let node = &self.graph[node_id];
                (
                    node_world_rect(node),
                    node.selected,
                    node.object.inlets(),
                    node.object.outlets(),
                    node.object.is_comment(),
                )
            };

            if is_comment || !node_rect.is_positive() || (inlets == 0 && outlets == 0) {
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
                let center = port_position_t(node_rect, inlet_ts[i], false, WORLD_ZOOM);
                let highlight = self.port_highlight(
                    node_id,
                    i,
                    WireEndpoint::Inlet,
                    pointer,
                    origin,
                );
                let response =
                    show_port_widget(ctx, port_id, center, selected, highlight, transform);
                inlet_positions[i] = center;
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
                let center = port_position_t(node_rect, outlet_ts[i], true, WORLD_ZOOM);
                let highlight = self.port_highlight(
                    node_id,
                    i,
                    WireEndpoint::Outlet,
                    pointer,
                    origin,
                );
                let response =
                    show_port_widget(ctx, port_id, center, selected, highlight, transform);
                outlet_positions[i] = center;
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
            self.pending_wires = vec![start];
            self.wire_drag_active = true;
        }
    }

    fn draw_wires_on_painter(
        &self,
        painter: &egui::Painter,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) {
        for edge_id in self.graph.edge_indices() {
            if self.graph[edge_id].selected {
                continue;
            }
            let points = if preview.is_some() {
                self.edge_bezier_points_for_preview(edge_id, preview)
            } else {
                self.edge_bezier_points(edge_id)
            };
            let Some(points) = points else {
                continue;
            };
            draw_bezier_wire(painter, points, false);
        }

        for edge_id in self.graph.edge_indices() {
            if !self.graph[edge_id].selected {
                continue;
            }
            let points = if preview.is_some() {
                self.edge_bezier_points_for_preview(edge_id, preview)
            } else {
                self.edge_bezier_points(edge_id)
            };
            let Some(points) = points else {
                continue;
            };
            draw_bezier_wire(painter, points, true);
        }
    }

    fn edge_bezier_points_for_preview(
        &self,
        edge_id: EdgeId,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Option<[Pos2; 4]> {
        let (from_id, to_id) = self.graph.edge_endpoints(edge_id)?;
        let edge = &self.graph[edge_id];
        let from = self.socket_position_for_preview(from_id, edge.from_port, true, preview);
        let to = self.socket_position_for_preview(to_id, edge.to_port, false, preview);
        Some(wire_bezier_points(from, true, to, true))
    }

    fn socket_position_for_preview(
        &self,
        node_id: NodeId,
        port: usize,
        is_outlet: bool,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) -> Pos2 {
        let node = &self.graph[node_id];
        let world = preview
            .and_then(|p| p.positions.get(&node_id.index()))
            .copied()
            .unwrap_or(node.pos);
        let size = preview
            .and_then(|p| p.sizes.get(&node_id.index()))
            .copied()
            .unwrap_or(node.size);
        let rect = Rect::from_min_size(world, size);
        let t = if is_outlet {
            let count = node.object.outlets();
            if node.outlet_t.len() == count {
                node.outlet_t.get(port).copied()
            } else {
                default_port_ts(count).get(port).copied()
            }
        } else {
            let count = node.object.inlets();
            if node.inlet_t.len() == count {
                node.inlet_t.get(port).copied()
            } else {
                default_port_ts(count).get(port).copied()
            }
        }
        .unwrap_or(0.0);
        port_position_t(rect, t, is_outlet, WORLD_ZOOM)
    }

    fn draw_wires(
        &self,
        ui: &mut Ui,
        canvas_rect: Rect,
        view: &CanvasView,
        preview: Option<&crate::layout_adapter::LayoutPreview>,
    ) {
        let transform = view.canvas_transform(canvas_rect.min);
        let patch_paint_layer = LayerId::new(Order::Background, Id::new("preview_wires"));
        ui.ctx().set_transform_layer(patch_paint_layer, transform);
        let painter = ui.ctx().layer_painter(patch_paint_layer);
        if preview.is_some() {
            for edge_id in self.graph.edge_indices() {
                let selected = self.graph[edge_id].selected;
                let points = self.edge_bezier_points_for_preview(edge_id, preview);
                let Some(points) = points else {
                    continue;
                };
                draw_bezier_wire(&painter, points, selected);
            }
        } else {
            self.draw_wires_on_painter(&painter, preview);
        }
    }

    fn draw_pending_wire(&self, painter: &egui::Painter, ctx: &Context, transform: TSTransform) {
        if self.pending_wires.is_empty() {
            return;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let Some(pointer) = pointer else {
            return;
        };
        let pointer_world = transform.inverse() * pointer;

        for pending in &self.pending_wires {
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

    fn object_menu_ui(&mut self, ctx: &Context) {
        let Some((screen, spawn_world)) = self.context_menu else {
            return;
        };

        Area::new(Id::new("pd_create_menu"))
            .order(Order::Foreground)
            .fixed_pos(screen)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ui.label(RichText::new("Put object").strong());
                    ui.separator();

                    let items: &[(&str, PdObject)] = &[
                        ("osc~", PdObject::OscTilde { freq: 440.0 }),
                        ("+~", PdObject::PlusTilde),
                        ("*~", PdObject::MulTilde),
                        ("dac~", PdObject::DacTilde),
                        ("in", PdObject::In),
                        ("param", PdObject::Param),
                        ("out", PdObject::Out),
                        ("delay_in", PdObject::DelayIn { id: None }),
                        ("delay_out", PdObject::DelayOut { id: None }),
                        ("combine", PdObject::Combine),
                        ("metro", PdObject::Metro { ms: 500.0 }),
                        ("random", PdObject::Random { max: 100 }),
                        ("float atom", PdObject::FloatAtom { value: 0.0 }),
                        (
                            "message",
                            PdObject::Message {
                                text: "bang".to_owned(),
                            },
                        ),
                        (
                            "comment",
                            PdObject::Comment {
                                text: "comment".to_owned(),
                            },
                        ),
                    ];

                    for (name, object) in items {
                        if ui.button(*name).clicked() {
                            self.record_undo();
                            self.add_object(object.clone(), spawn_world);
                            self.context_menu = None;
                        }
                    }

                    ui.separator();
                    if ui.button("Cancel").clicked() {
                        self.context_menu = None;
                    }
                });
            });
    }
}

struct NodeWindowBody {
    clicked_label: bool,
    commit_edit: bool,
    cancel_edit: bool,
}

fn node_edit_body(
    ui: &mut Ui,
    window_id: Id,
    buffer: &mut String,
    is_comment: bool,
    edit_size: Vec2,
    zoom: f32,
) -> NodeWindowBody {
    let mut body = NodeWindowBody {
        clicked_label: false,
        commit_edit: false,
        cancel_edit: false,
    };

    let font = label_font(zoom);

    ui.allocate_ui_with_layout(
        edit_size,
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_width(edit_size.x);
            ui.spacing_mut().item_spacing.y = 0.0;

            let edit = if is_comment {
                egui::TextEdit::multiline(buffer)
                    .font(font)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(LABEL_INSET_X as i8, 2))
            } else {
                egui::TextEdit::singleline(buffer)
                    .font(font)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(LABEL_INSET_X as i8, 2))
            };

            let response = ui.add(edit.id(window_id.with("edit")).text_color(INK));
            response.request_focus();

            if !is_comment && ui.input(|i| i.key_pressed(Key::Enter)) {
                body.commit_edit = true;
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                body.cancel_edit = true;
            }
        },
    );

    body
}

fn random_unused_delay_hex(used: &HashSet<u8>) -> u8 {
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

fn parse_delay_hex(token: Option<&str>) -> Option<u8> {
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

fn parse_pd_object_text(text: &str) -> PdObject {
    let stripped = strip_brackets(text.trim());
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

fn show_wire_handle_widget(
    ctx: &Context,
    id: Id,
    center: Pos2,
    transform: TSTransform,
    combined: bool,
) -> egui::Response {
    set_widget_layer_transform(ctx, Order::Foreground, id, transform);
    let size = port_size(WORLD_ZOOM) * if combined { 1.35 } else { 1.1 };
    let top_left = center - Vec2::splat(size * 0.5);

    Area::new(id)
        .fixed_pos(top_left)
        .order(Order::Foreground)
        .constrain(false)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(
                Vec2::splat(size),
                Sense::click_and_drag().union(Sense::hover()),
            );
            paint_wire_handle(ui.painter(), rect.center(), response.hovered(), WORLD_ZOOM);
            response
        })
        .inner
}

fn show_port_widget(
    ctx: &Context,
    id: Id,
    center: Pos2,
    selected: bool,
    highlight: PortHighlight,
    transform: TSTransform,
) -> egui::Response {
    set_widget_layer_transform(ctx, Order::Foreground, id, transform);
    let size = port_size(WORLD_ZOOM);
    let top_left = center - Vec2::splat(size * 0.5);

    Area::new(id)
        .fixed_pos(top_left)
        .order(Order::Foreground)
        .constrain(false)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(
                Vec2::splat(size),
                Sense::click_and_drag().union(Sense::hover()),
            );
            let highlight = if highlight != PortHighlight::None {
                highlight
            } else if response.hovered() {
                PortHighlight::Hovered
            } else {
                PortHighlight::None
            };
            paint_port_square(ui.painter(), rect.center(), selected, highlight, WORLD_ZOOM);
            response
        })
        .inner
}

fn node_window_body(
    ui: &mut Ui,
    window_id: Id,
    label: &str,
    is_comment: bool,
    selected: bool,
    zoom: f32,
) -> NodeWindowBody {
    let mut body = NodeWindowBody {
        clicked_label: false,
        commit_edit: false,
        cancel_edit: false,
    };

    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;

        if is_comment {
            let label_response = ui.label(
                egui::RichText::new(label).font(label_font(zoom)).color(PAPER_DIM),
            );
            if label_response.clicked() {
                body.clicked_label = true;
            }
        } else {
            let font = label_font(zoom);
            let job = layout_job(label, font, selected);
            let galley = ui.painter().layout_job(job);
            let op_color = if selected { INK } else { PAPER };
            let size = egui::vec2(
                ui.available_width().max(galley.size().x + LABEL_INSET_X * zoom * 2.0),
                (BOX_H * zoom).max(galley.size().y),
            );
            let (rect, label_response) = ui.allocate_exact_size(size, Sense::click());
            ui.painter().galley(
                egui::pos2(
                    rect.min.x + LABEL_INSET_X * zoom,
                    rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                op_color,
            );
            if label_response.clicked() {
                body.clicked_label = true;
            }
        }
    });

    let _ = window_id;
    body
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

fn set_widget_layer_transform(ctx: &Context, order: Order, id: Id, transform: TSTransform) {
    ctx.set_transform_layer(LayerId::new(order, id), transform);
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

fn node_draw_rect(node: &Node) -> Rect {
    node_world_rect(node)
}

fn estimate_node_size(object: &PdObject) -> Vec2 {
    let label = object.bracketed_label();
    let width = min_box_width(&label, object.inlets());
    let height = if object.is_comment() { BOX_H * 0.8 } else { BOX_H };
    vec2(width.max(48.0), height.max(BOX_H))
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

fn draw_grid_screen(painter: &egui::Painter, rect: Rect, view: &CanvasView, origin: Pos2) {
    let step = GRID_STEP * view.zoom;
    if step < 6.0 {
        return;
    }

    let start_x = ((origin.x + view.pan.x) % step) - step;
    let start_y = ((origin.y + view.pan.y) % step) - step;

    let mut x = start_x;
    while x < rect.right() {
        let mut y = start_y;
        while y < rect.bottom() {
            if rect.contains(egui_pos2(x, y)) {
                painter.circle_filled(egui_pos2(x, y), 0.75, PAPER_DIM);
            }
            y += step;
        }
        x += step;
    }
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
