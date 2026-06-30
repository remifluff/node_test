use egui::text::{LayoutJob, TextFormat};
use egui::{
    self, Color32, Context, CornerRadius, FontFamily, FontId, Margin, Stroke, Style, TextStyle,
    Visuals,
};

/// Kentucky Route Zero palette from `fragment_interlay` / `lop-node-editor`.
pub const INK: Color32 = Color32::from_rgb(18, 18, 20);
pub const PAPER: Color32 = Color32::from_rgb(222, 216, 198);
pub const PAPER_DIM: Color32 = Color32::from_rgb(120, 116, 104);
pub const INK_PANEL: Color32 = Color32::from_rgb(28, 28, 31);
pub const WIRE_HANDLE: Color32 = Color32::from_rgb(156, 92, 48);
pub const WIRE_HANDLE_HOVER: Color32 = Color32::from_rgb(196, 128, 72);

pub const FONT_PT: f32 = 10.0;
pub const BOX_H: f32 = 22.0;
pub const PORT_R: f32 = 3.5;
/// Visual scale vs fragment_interlay base (`PORT_R * 1.7`), plus 50% UI bump.
pub const PORT_SIZE_FACTOR: f32 = 1.7 * 1.2 * 1.5;
pub const GRID_STEP: f32 = 15.0;
pub const LABEL_INSET_X: f32 = 3.0;
pub const END_CAP_W: f32 = 3.0;
pub const PORT_EDGE_INSET: f32 = END_CAP_W + 3.0;
pub const CHAR_W: f32 = 6.0;
pub const LINE_W: f32 = 1.0;
pub const CABLE_STROKE: f32 = 1.15;

pub fn apply_interlay_visuals(ctx: &Context) {
    let mut style = Style::default();
    style.spacing.item_spacing = egui::vec2(2.0, 0.0);
    style.spacing.window_margin = Margin::same(0);
    style.spacing.button_padding = egui::vec2(4.0, 2.0);

    style.visuals = Visuals::dark();
    style.visuals.window_fill = INK;
    style.visuals.panel_fill = INK_PANEL;
    style.visuals.extreme_bg_color = INK;
    style.visuals.window_stroke = Stroke::new(LINE_W, PAPER);
    style.visuals.override_text_color = Some(PAPER);
    style.visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.active.corner_radius = CornerRadius::ZERO;
    style.visuals.widgets.open.corner_radius = CornerRadius::ZERO;
    style.visuals.selection.bg_fill = Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 60);
    style.visuals.selection.stroke = Stroke::new(LINE_W, PAPER);

    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(FONT_PT, FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(FONT_PT, FontFamily::Monospace),
    );

    ctx.set_global_style(style);
}

pub fn node_fill(selected: bool) -> Color32 {
    if selected { PAPER } else { INK }
}

pub fn node_frame(selected: bool, is_comment: bool) -> egui::Frame {
    if is_comment {
        return egui::Frame {
            fill: Color32::TRANSPARENT,
            stroke: Stroke::NONE,
            inner_margin: Margin::symmetric(2, 1),
            corner_radius: CornerRadius::ZERO,
            ..Default::default()
        };
    }

    egui::Frame {
        fill: node_fill(selected),
        stroke: Stroke::new(LINE_W, PAPER),
        inner_margin: Margin::symmetric(END_CAP_W as i8, 2),
        corner_radius: CornerRadius::ZERO,
        ..Default::default()
    }
}

pub fn node_edit_frame(is_comment: bool) -> egui::Frame {
    if is_comment {
        return egui::Frame {
            fill: PAPER,
            stroke: Stroke::NONE,
            inner_margin: Margin::symmetric(2, 1),
            corner_radius: CornerRadius::ZERO,
            ..Default::default()
        };
    }

    egui::Frame {
        fill: PAPER,
        stroke: Stroke::new(LINE_W, PAPER),
        inner_margin: Margin::symmetric(END_CAP_W as i8, 2),
        corner_radius: CornerRadius::ZERO,
        ..Default::default()
    }
}

pub fn port_size(zoom: f32) -> f32 {
    (PORT_R * PORT_SIZE_FACTOR * zoom).max(2.0)
}

pub fn default_port_ts(count: usize) -> Vec<f32> {
    if count == 0 {
        Vec::new()
    } else if count == 1 {
        vec![0.0]
    } else {
        (0..count)
            .map(|i| i as f32 / (count as f32 - 1.0))
            .collect()
    }
}

pub fn port_span(node_rect: egui::Rect, zoom: f32) -> f32 {
    (node_rect.width() - 2.0 * port_edge_inset(zoom)).max(0.0)
}

pub fn port_t_from_x(x: f32, node_rect: egui::Rect, zoom: f32) -> f32 {
    let inset = port_edge_inset(zoom);
    let span = port_span(node_rect, zoom);
    if span <= 0.0 {
        0.0
    } else {
        ((x - node_rect.left() - inset) / span).clamp(0.0, 1.0)
    }
}

/// Port on top/bottom edge at normalized position `t` ∈ [0, 1] along the inset span.
pub fn port_position_t(node_rect: egui::Rect, t: f32, is_outlet: bool, zoom: f32) -> egui::Pos2 {
    let inset = port_edge_inset(zoom);
    let span = port_span(node_rect, zoom);
    let x = node_rect.left() + inset + t.clamp(0.0, 1.0) * span;
    let y = if is_outlet {
        node_rect.bottom()
    } else {
        node_rect.top()
    };
    egui::pos2(x, y)
}

pub fn label_font(zoom: f32) -> FontId {
    FontId::new((FONT_PT * zoom).max(7.0), FontFamily::Monospace)
}

pub fn strip_brackets(name: &str) -> &str {
    name.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(name)
}

/// Operator token in `name_text`, remainder in `name_args`.
pub fn layout_job(name: &str, font: FontId, selected: bool) -> LayoutJob {
    let display = if name.is_empty() {
        "?"
    } else {
        strip_brackets(name)
    };
    let op_color = if selected { INK } else { PAPER };
    let args_color = PAPER_DIM;

    let mut job = LayoutJob::default();
    let fmt = |color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };

    let b = display.as_bytes();
    let mut i = 0usize;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < b.len() {
        let op_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        if op_start > 0 {
            job.append(&display[..op_start], 0.0, fmt(op_color));
        }
        job.append(&display[op_start..i], 0.0, fmt(op_color));
        if i < display.len() {
            job.append(&display[i..], 0.0, fmt(args_color));
        }
    } else if !display.is_empty() {
        job.append(display, 0.0, fmt(op_color));
    }
    job
}

pub fn paint_node_hover_highlight(painter: &egui::Painter, rect: egui::Rect, zoom: f32) {
    let pad = (5.0 * zoom).max(3.0);
    let highlight = rect.expand(pad);
    let fill = Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 28);
    let stroke = Stroke::new(
        1.0,
        Color32::from_rgba_premultiplied(PAPER.r(), PAPER.g(), PAPER.b(), 200),
    );
    painter.rect_filled(highlight, 0.0, fill);
    painter.rect_stroke(highlight, 0.0, stroke, egui::StrokeKind::Outside);
}

pub fn port_edge_inset(zoom: f32) -> f32 {
    PORT_EDGE_INSET * zoom
}

/// Port on top/bottom edge, inset from left/right like fragment_interlay.
pub fn port_position(node_rect: egui::Rect, index: usize, count: usize, is_outlet: bool, zoom: f32) -> egui::Pos2 {
    let t = if count <= 1 {
        0.0
    } else {
        index as f32 / (count as f32 - 1.0)
    };
    port_position_t(node_rect, t, is_outlet, zoom)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PortHighlight {
    None,
    Hovered,
    Connecting,
    ConnectTarget,
}

pub fn paint_port_square(
    painter: &egui::Painter,
    center: egui::Pos2,
    selected: bool,
    highlight: PortHighlight,
    zoom: f32,
) {
    let size = port_size(zoom);
    let fill = match highlight {
        PortHighlight::ConnectTarget | PortHighlight::Connecting => WIRE_HANDLE_HOVER,
        PortHighlight::Hovered => WIRE_HANDLE,
        PortHighlight::None if selected => INK,
        PortHighlight::None => PAPER,
    };
    painter.rect_filled(
        egui::Rect::from_center_size(center, egui::vec2(size, size)),
        0.0,
        fill,
    );
}

pub fn min_box_width(name: &str, inlets: usize) -> f32 {
    let label = strip_brackets(if name.is_empty() { "?" } else { name });
    let text_w = label.len() as f32 * CHAR_W;
    let label_w = END_CAP_W + LABEL_INSET_X + text_w + CHAR_W + END_CAP_W;
    let inlet_w = inlets.max(1) as f32 * GRID_STEP;
    label_w.max(inlet_w)
}
