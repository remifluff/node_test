#[derive(Clone, Debug)]
pub struct Node {
    pub object: crate::object::PdObject,
    /// Exact text shown in the box; preserved verbatim while editing.
    pub label: String,
    pub pos: emath::Pos2,
    pub size: emath::Vec2,
    /// Stable `obj-N` id for `.lop` patch export (matches fragment_interlay).
    pub box_id: Option<String>,
    pub screen_rect: emath::Rect,
    pub inlet_t: Vec<f32>,
    pub outlet_t: Vec<f32>,
    pub inlet_positions: Vec<emath::Pos2>,
    pub outlet_positions: Vec<emath::Pos2>,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeData {
    pub from_port: usize,
    pub to_port: usize,
    pub selected: bool,
}
