use eframe::egui::{
    emath::TSTransform, pos2, vec2, Context, Id, InnerResponse, LayerId, Order, PointerButton, Pos2,
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
