use patch_graph::{NodeId, PatchGraph};

pub fn align_selection_vertical(_graph: &mut PatchGraph, _snap_to_grid: bool) -> bool {
    false
}

pub fn propagate_align_from_anchors(
    _graph: &mut PatchGraph,
    _anchors: &[NodeId],
    _snap_to_grid: bool,
    _bump: bool,
) -> bool {
    false
}
