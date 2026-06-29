//! Sorted-layout rendering for the keyboard pane.

use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, Stroke, Ui, Vec2};
use patch_graph::{LayoutPreview, Node, NodeId, PatchGraph};

use mouse_ui::canvas::{scene_layer_id, scene_transform};
use mouse_ui::style::{
    self, default_port_ts, label_font, layout_job, paint_port_square, port_position_t,
    PortHighlight, GRID_STEP, INK, LABEL_INSET_X, LINE_W, PAPER, PAPER_DIM,
};

const PATCH_BORDER_PAD: f32 = 100.0;
const WORLD_ZOOM: f32 = 1.0;

pub fn draw_grid(painter: &egui::Painter, world_clip: Rect) {
    let step = GRID_STEP;
    let mut x = (world_clip.min.x / step).floor() * step;
    while x <= world_clip.max.x {
        if world_clip.min.x <= x && x <= world_clip.max.x {
            painter.line_segment(
                [pos2(x, world_clip.min.y), pos2(x, world_clip.max.y)],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 18)),
            );
        }
        x += step;
    }
    let mut y = (world_clip.min.y / step).floor() * step;
    while y <= world_clip.max.y {
        if world_clip.min.y <= y && y <= world_clip.max.y {
            painter.line_segment(
                [pos2(world_clip.min.x, y), pos2(world_clip.max.x, y)],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 18)),
            );
        }
        y += step;
    }
}

fn node_visible_in_scene(world_clip: Rect, world_rect: Rect) -> bool {
    world_rect.is_positive() && world_clip.intersects(world_rect)
}

fn world_pos_for_node(
    graph: &PatchGraph,
    node_id: NodeId,
    preview: &LayoutPreview,
) -> Pos2 {
    if let Some(pos) = preview.positions.get(&node_id.index()) {
        return *pos;
    }
    graph[node_id].pos
}

fn node_size_for_preview(
    graph: &PatchGraph,
    node_id: NodeId,
    preview: &LayoutPreview,
) -> Vec2 {
    if let Some(size) = preview.sizes.get(&node_id.index()) {
        return *size;
    }
    graph[node_id].size
}

fn node_world_rect_for(graph: &PatchGraph, node_id: NodeId, preview: &LayoutPreview) -> Rect {
    Rect::from_min_size(
        world_pos_for_node(graph, node_id, preview),
        node_size_for_preview(graph, node_id, preview),
    )
}

fn socket_position(node: &Node, rect: Rect, index: usize, is_outlet: bool) -> Pos2 {
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
    let ts = if is_outlet {
        &node.outlet_t
    } else {
        &node.inlet_t
    };
    let t = ts.get(index).copied().unwrap_or(0.0);
    port_position_t(rect, t, is_outlet, WORLD_ZOOM)
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
    [from, from + from_tangent * sag, to + to_tangent * sag, to]
}

fn draw_bezier_wire(painter: &egui::Painter, points: [Pos2; 4], selected: bool) {
    painter.add(egui::Shape::CubicBezier(egui::epaint::CubicBezierShape {
        points,
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: egui::epaint::PathStroke::new(
            if selected { 2.0 } else { 1.15 },
            if selected { PAPER } else { PAPER_DIM },
        ),
    }));
}

pub fn paint_scene(
    scene_ui: &mut Ui,
    graph: &PatchGraph,
    preview: &LayoutPreview,
    keyboard_focus: Option<NodeId>,
    parent_id: Id,
    parent_order: Order,
) {
    let ctx = scene_ui.ctx().clone();
    let scene_layer = scene_layer_id(parent_id, parent_order);
    let transform = scene_transform(&ctx, parent_id, parent_order);
    let world_clip = scene_ui.clip_rect();
    let mut painter = ctx.layer_painter(scene_layer);
    painter.set_clip_rect(world_clip);
    draw_grid(&painter, world_clip);

    let mut node_order: Vec<NodeId> = graph.node_indices().collect();
    node_order.sort_by_key(|id| graph[*id].object.is_comment());

    for node_id in node_order {
        paint_node(
            graph,
            &painter,
            world_clip,
            node_id,
            preview,
            transform.scaling,
            keyboard_focus == Some(node_id),
        );
    }
    paint_ports(graph, &painter, world_clip, preview, transform.scaling);
    draw_patch_border(graph, &painter, world_clip, preview);
    draw_wires(graph, &painter, preview);
}

fn paint_node(
    graph: &PatchGraph,
    painter: &egui::Painter,
    world_clip: Rect,
    node_id: NodeId,
    preview: &LayoutPreview,
    zoom: f32,
    keyboard_focus: bool,
) {
    let node = &graph[node_id];
    let rect = node_world_rect_for(graph, node_id, preview);
    if !node_visible_in_scene(world_clip, rect) {
        return;
    }
    let label = &node.label;

    if node.object.is_comment() {
        let font = label_font(zoom);
        let job = layout_job(label, font, false);
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(rect.min.x, rect.center().y - galley.size().y * 0.5),
            galley,
            PAPER_DIM,
        );
        return;
    }

    let frame = style::node_frame(keyboard_focus, false);
    painter.add(frame.paint(rect));

    let font = label_font(zoom);
    let job = layout_job(label, font, false);
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

fn paint_ports(
    graph: &PatchGraph,
    painter: &egui::Painter,
    world_clip: Rect,
    preview: &LayoutPreview,
    zoom: f32,
) {
    for node_id in graph.node_indices() {
        let node = &graph[node_id];
        if node.object.is_comment() {
            continue;
        }
        let rect = node_world_rect_for(graph, node_id, preview);
        if !node_visible_in_scene(world_clip, rect) {
            continue;
        }
        let inlets = node.object.inlets();
        let outlets = node.object.outlets();
        let inlet_ts = default_port_ts(inlets);
        let outlet_ts = default_port_ts(outlets);
        for i in 0..inlets {
            let center = port_position_t(rect, inlet_ts[i], false, WORLD_ZOOM);
            paint_port_square(painter, center, false, PortHighlight::None, zoom);
        }
        for i in 0..outlets {
            let center = port_position_t(rect, outlet_ts[i], true, WORLD_ZOOM);
            paint_port_square(painter, center, false, PortHighlight::None, zoom);
        }
    }
}

fn draw_patch_border(graph: &PatchGraph, painter: &egui::Painter, world_clip: Rect, preview: &LayoutPreview) {
    let mut bounds = Rect::NOTHING;
    let mut any = false;
    for node_id in graph.node_indices() {
        let rect = node_world_rect_for(graph, node_id, preview);
        if !rect.is_positive() {
            continue;
        }
        bounds = if any { bounds.union(rect) } else { rect };
        any = true;
    }
    let Some(bounds) = any.then_some(bounds) else {
        return;
    };
    let border = bounds.expand(PATCH_BORDER_PAD);
    if !world_clip.intersects(border) {
        return;
    }
    painter.rect_stroke(
        border,
        CornerRadius::ZERO,
        Stroke::new(LINE_W, PAPER_DIM),
        egui::StrokeKind::Outside,
    );
}

fn draw_wires(graph: &PatchGraph, painter: &egui::Painter, preview: &LayoutPreview) {
    for edge_id in graph.edge_indices() {
        let Some((from, to)) = graph.edge_endpoints(edge_id) else {
            continue;
        };
        let edge = &graph[edge_id];
        let from_rect = node_world_rect_for(graph, from, preview);
        let to_rect = node_world_rect_for(graph, to, preview);
        let from_pos = socket_position(&graph[from], from_rect, edge.from_port, true);
        let to_pos = socket_position(&graph[to], to_rect, edge.to_port, false);
        let points = wire_bezier_points(from_pos, true, to_pos, true);
        draw_bezier_wire(painter, points, edge.selected);
    }
}
