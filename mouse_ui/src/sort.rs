use crate::layout::GRID_STEP;

pub const COLUMN_PITCH: f32 = GRID_STEP * 4.0;
pub const COLUMN_EXTRA_PADDING: f32 = 10.0;

pub fn sort_patch(graph: &mut patch_graph::PatchGraph, _snap: bool) -> bool {
    if graph.node_count() <= 1 {
        return false;
    }
    patch_graph::layout_adapter::organize_patch(
        graph,
        &patch_graph::layout::LayoutConfig::default(),
    );
    true
}
