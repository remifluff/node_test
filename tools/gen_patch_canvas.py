import re
from pathlib import Path

src = Path("src/app.rs").read_text()

const_start = src.index("const MARQUEE_MIN_AREA")
default_impl = src.index("impl Default for PdPatchEditor")
app_impl = src.index("impl eframe::App for PdPatchEditor")
canvas_frame = src.index("struct CanvasFrameState")
helpers_start = src.index("struct NodeAreaShowResult")
tests_start = src.index("#[cfg(test)]\nmod shift_drag_tests")
tail_helpers_start = src.index("fn show_wire_handle_widget")

part1 = src[const_start:default_impl]
part2 = src[canvas_frame:helpers_start]
helpers = src[helpers_start:tests_start] + src[tail_helpers_start:]
default_block = src[default_impl:app_impl]

snapshot_re = r'#\[derive\(Clone, Debug\)\]\nstruct PatchSnapshot \{.*?\n\}\n'
part1 = re.sub(snapshot_re, '', part1, count=1, flags=re.S)
part1 = re.sub(r'pub struct PdPatchEditor \{[^}]+\}\n\n', '', part1, count=1)
part1 = re.sub(r'    fn format_patch_lop\(&self\) -> String \{[^}]+\}\n\n', '', part1)
part1 = re.sub(r'    pub fn demo_patch\(\) -> Self \{.*?\n    \}\n\n', '', part1, flags=re.DOTALL)
part1 = part1.replace("impl PdPatchEditor {", "impl CanvasSession<'_> {")
part1 = part1.rstrip()
if part1.endswith("}"):
    part1 = part1[:-1].rstrip() + "\n"

part2 = re.sub(
    r"struct CanvasFrameState \{[^}]+\}\n\n",
    "",
    part2,
    count=1,
)
part2 = part2.replace("impl PdPatchEditor {\n", "", 1)

body = part1 + part2

canvas_fields = [
    "mouse_view", "pending_wires", "wire_drag_active", "rewire_state", "marquee",
    "editing_node", "edit_buffer", "edit_start_size", "delay_pairs",
    "debug_auto_combine", "debug_auto_send_receive", "debug_auto_delay",
    "next_box_id", "alt_drag_duplicate", "shift_drag_splice_preview", "shift_drag_eligible",
    "undo_stack", "redo_stack", "node_pointer_press", "operator_library",
]
for f in canvas_fields:
    body = re.sub(rf"\bself\.{f}\b", f"self.canvas.{f}", body)
    body = re.sub(rf"self\s*\n\s*\.{f}\b", f"self.canvas.{f}", body)

body = body.replace("Self::remap_copied_signal_hex", "CanvasSession::remap_copied_signal_hex")
body = body.replace("self.graph = snapshot.graph;", "*self.graph = snapshot.graph;")
body = re.sub(snapshot_re, '', body, count=1, flags=re.S)

patch_canvas_struct = """pub struct PatchCanvas {
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

"""

patch_canvas_impl = """impl PatchCanvas {
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

"""

default_block = default_block.replace("impl Default for PdPatchEditor", "impl Default for PatchCanvas")
default_block = default_block.replace("            graph: PatchGraph::default(),\n", "")
default_block = default_block.replace('            patch_name: "patch".to_owned(),\n', "")

header = """use egui::{
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

"""

insert_at = body.index("impl CanvasSession")
body = body[:insert_at] + patch_canvas_struct + body[insert_at:]

full = header + patch_canvas_impl + body + default_block + helpers
Path("mouse_ui/src/patch_canvas.rs").write_text(full)
print("lines", full.count("\n"))
