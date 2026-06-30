use egui::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostRole {
    Anchor,
    Stub,
}

#[derive(Clone, Copy, Debug)]
pub struct Ghost {
    pub node: usize,
    pub col: i32,
    pub inlet: Option<usize>,
    pub role: GhostRole,
    pub rect: Rect,
}

#[derive(Clone, Debug)]
pub struct ColumnBox {
    pub col: i32,
    pub port_x: f32,
    pub bounds: Rect,
    pub ghosts: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct ColumnLayout {
    pub ghosts: Vec<Ghost>,
    pub boxes: Vec<ColumnBox>,
    pub port_x: std::collections::BTreeMap<i32, f32>,
}

impl ColumnLayout {
    pub fn build(_model: &patch_graph::PatchGraph) -> Self {
        Self::default()
    }

    pub fn apply_to(&self, _model: &mut patch_graph::PatchGraph, _snap: bool) -> bool {
        false
    }

    pub fn padded(&self, b: &ColumnBox, margin: f32) -> Rect {
        b.bounds.expand(margin)
    }
}
