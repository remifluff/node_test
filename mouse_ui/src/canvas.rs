use eframe::egui::{
    emath::TSTransform, pos2, Context, Id, InnerResponse, LayerId, Order, PointerButton, Pos2,
    Rangef, Rect, Sense, Ui, UiBuilder, Vec2,
};

pub const SCENE_MAX_SIZE: f32 = 500_000.0;

/// Pan/zoom viewport state for [`Scene`].
#[derive(Clone, Copy, Debug)]
pub struct CanvasView {
    pub scene_rect: Rect,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasView {
    pub const MIN_ZOOM: f32 = 0.25;
    const MAX_ZOOM: f32 = 4.0;

    pub fn new() -> Self {
        Self {
            scene_rect: Rect::NOTHING,
        }
    }
}

/// Show the patch canvas scene without egui [`Scene`]'s built-in pan/zoom (we handle that
/// separately so scroll works over nodes and is not applied twice on the background).
pub fn show_patch_scene<R>(
    ui: &mut Ui,
    scene_rect: &mut Rect,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let zoom_range = Rangef::new(CanvasView::MIN_ZOOM, CanvasView::MAX_ZOOM);
    let max_inner_size = Vec2::splat(SCENE_MAX_SIZE);

    let (outer_rect, outer_response) =
        ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::click_and_drag());
    apply_canvas_navigation(ui, outer_rect, scene_rect);

    let mut to_global = fit_scene_to_view(outer_rect, *scene_rect, zoom_range);
    let scene_rect_was_good =
        to_global.is_valid() && scene_rect.is_finite() && scene_rect.size() != Vec2::ZERO;

    let scene_layer_id = LayerId::new(ui.layer_id().order, ui.id().with("scene_area"));
    ui.ctx().set_sublayer(ui.layer_id(), scene_layer_id);

    let mut local_ui = ui.new_child(
        UiBuilder::new()
            .layer_id(scene_layer_id)
            .max_rect(Rect::from_min_size(Pos2::ZERO, max_inner_size))
            .sense(Sense::empty()),
    );

    local_ui.set_clip_rect(to_global.inverse() * outer_rect);
    local_ui
        .ctx()
        .set_transform_layer(scene_layer_id, to_global);

    let inner = add_contents(&mut local_ui);

    if !scene_rect_was_good {
        let inner_rect = local_ui.min_rect();
        to_global = fit_scene_to_view(outer_rect, inner_rect, zoom_range);
        *scene_rect = to_global.inverse() * outer_rect;
    }

    InnerResponse {
        response: outer_response,
        inner,
    }
}

/// Match [`Scene`]'s fit transform (scene coords → screen).
pub fn fit_scene_to_view(view_rect: Rect, scene_rect: Rect, zoom_range: Rangef) -> TSTransform {
    let scale = view_rect.size() / scene_rect.size();
    let scale = scale.min_elem();
    let scale = zoom_range.clamp(scale);
    let center_in_global = view_rect.center().to_vec2();
    let center_scene = scene_rect.center().to_vec2();
    TSTransform::from_translation(center_in_global - scale * center_scene)
        * TSTransform::from_scaling(scale)
}

pub fn apply_canvas_navigation(ui: &Ui, view_rect: Rect, scene_rect: &mut Rect) {
    if !view_rect.is_positive() || !scene_rect.is_positive() {
        return;
    }

    let zoom_range = Rangef::new(CanvasView::MIN_ZOOM, CanvasView::MAX_ZOOM);
    let mut to_global = fit_scene_to_view(view_rect, *scene_rect, zoom_range);

    let Some(pointer) = ui.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    if !view_rect.contains(pointer) {
        return;
    }

    let mut changed = false;

    if ui.input(|i| i.pointer.button_down(PointerButton::Middle)) {
        let delta = ui.input(|i| i.pointer.delta());
        if delta.length_sq() > 0.0 {
            to_global.translation += to_global.scaling * delta;
            changed = true;
        }
    }

    let zoom_delta = ui.input(|i| i.zoom_delta());
    let pan_delta = ui.input(|i| i.smooth_scroll_delta());
    if zoom_delta != 1.0 || pan_delta != Vec2::ZERO {
        let pointer_in_scene = to_global.inverse() * pointer;

        if zoom_delta != 1.0 {
            let zd = zoom_delta.clamp(
                zoom_range.min / to_global.scaling,
                zoom_range.max / to_global.scaling,
            );
            to_global = to_global
                * TSTransform::from_translation(pointer_in_scene.to_vec2())
                * TSTransform::from_scaling(zd)
                * TSTransform::from_translation(-pointer_in_scene.to_vec2());
            to_global.scaling = zoom_range.clamp(to_global.scaling);
        }

        to_global = TSTransform::from_translation(pan_delta) * to_global;
        changed = true;
    }

    if changed {
        *scene_rect = to_global.inverse() * view_rect;
    }
}

pub fn scene_layer_id(parent: Id, order: Order) -> LayerId {
    LayerId::new(order, parent.with("scene_area"))
}

pub fn scene_transform(ctx: &Context, parent: Id, order: Order) -> TSTransform {
    ctx.layer_transform_to_global(scene_layer_id(parent, order))
        .unwrap_or(TSTransform::IDENTITY)
}

pub fn to_screen(p: Pos2, pan: Vec2, zoom: f32) -> Pos2 {
    pos2(p.x * zoom + pan.x, p.y * zoom + pan.y)
}

pub fn to_world(p: Pos2, pan: Vec2, zoom: f32) -> Pos2 {
    pos2((p.x - pan.x) / zoom, (p.y - pan.y) / zoom)
}

/// Drag state during node-editor interaction.
#[derive(Clone, Debug)]
pub enum Drag {
    Nodes {
        items: Vec<NodeDragItem>,
        extracted: bool,
    },
    Connect {
        origin: PortEnd,
        cursor: Pos2,
    },
    Select {
        start: Pos2,
        current: Pos2,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct NodeDragItem {
    pub id: patch_graph::NodeId,
    pub pos: Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortEnd {
    Outlet {
        node: patch_graph::NodeId,
        idx: usize,
    },
    Inlet {
        node: patch_graph::NodeId,
        idx: usize,
    },
}

/// Editor state: model + drag + undo + column offsets.
#[derive(Clone, Debug)]
pub struct PatchState {
    pub model: patch_graph::PatchGraph,
    pub drag: Option<Drag>,
    pub pending: Option<PortEnd>,
    pub col_offset: std::collections::BTreeMap<i32, f32>,
}

impl Default for PatchState {
    fn default() -> Self {
        Self {
            model: patch_graph::PatchGraph::default(),
            drag: None,
            pending: None,
            col_offset: std::collections::BTreeMap::new(),
        }
    }
}

/// Drag metrics for body-drag detection.
#[derive(Clone, Copy, Debug, Default)]
pub struct BodyDragProbe {
    pub global_dragged_id: Option<Id>,
    pub body_id: Option<Id>,
    pub motion: Vec2,
    pub dragged: bool,
    pub drag_active: bool,
    pub three_finger: bool,
}

/// Mouse/canvas debug info.
#[derive(Clone, Debug)]
pub struct MouseDebugInfo {
    pub screen_pos: Pos2,
    pub world_pos: Pos2,
    pub on_canvas: bool,
    pub primary_down: bool,
    pub secondary_down: bool,
    pub middle_down: bool,
    pub hover_target: String,
    pub click_target: String,
    pub drag_state: String,
    pub resize_drag: bool,
}

impl Default for MouseDebugInfo {
    fn default() -> Self {
        Self {
            screen_pos: Pos2::ZERO,
            world_pos: Pos2::ZERO,
            on_canvas: false,
            primary_down: false,
            secondary_down: false,
            middle_down: false,
            hover_target: String::new(),
            click_target: String::new(),
            drag_state: String::new(),
            resize_drag: false,
        }
    }
}

/// Main node-editor widget.
#[derive(Clone, Debug)]
pub struct NodeEditor {
    pub state: PatchState,
    pub drag_last_world: Option<Pos2>,
    pub focus_pending: bool,
    pan: Vec2,
    zoom: f32,
    zoom_min: f32,
    zoom_max: f32,
    fit_margin: f32,
    pending_fit: bool,
    mouse_debug: MouseDebugInfo,
}

impl Default for NodeEditor {
    fn default() -> Self {
        Self {
            state: PatchState::default(),
            drag_last_world: None,
            focus_pending: false,
            pan: Vec2::ZERO,
            zoom: 1.0,
            zoom_min: 0.25,
            zoom_max: 4.0,
            fit_margin: 100.0,
            pending_fit: false,
            mouse_debug: MouseDebugInfo::default(),
        }
    }
}

impl NodeEditor {
    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn set_view(&mut self, pan: Vec2, zoom: f32) {
        self.pan = pan;
        self.zoom = zoom.clamp(self.zoom_min, self.zoom_max);
    }

    pub fn set_zoom_range(&mut self, min: f32, max: f32) {
        self.zoom_min = min;
        self.zoom_max = max;
    }

    pub fn set_fit_margin(&mut self, margin: f32) {
        self.fit_margin = margin;
    }

    pub fn request_fit(&mut self) {
        self.pending_fit = true;
    }

    pub fn mouse_debug(&self) -> &MouseDebugInfo {
        &self.mouse_debug
    }

    pub fn clear_pending(&mut self) -> bool {
        let had = self.state.pending.is_some();
        self.state.pending = None;
        had
    }

    pub fn is_editing(&self) -> bool {
        self.focus_pending
    }

    pub fn handle_keyboard_clipboard(&mut self, _ctx: &Context) {}

    pub fn render_nodes(
        &mut self,
        _ctx: &Context,
        _grid: &crate::layout::Grid,
        _clip: Rect,
        _pan: Vec2,
        _zoom: f32,
        _flags: &crate::flags::Flags,
    ) {
    }

    pub fn show(&mut self, ui: &mut Ui, _flags: &crate::flags::Flags) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::click_and_drag());
        if self.pending_fit {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
            self.pending_fit = false;
        }
        self.mouse_debug.on_canvas = rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or(Pos2::ZERO)));
        response
    }
}
