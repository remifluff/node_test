use eframe::egui::{text::LayoutJob, FontId, Pos2, Rect, Vec2};

use patch_graph::NodeId;

pub struct CodeboxWidget<'a> {
    pub id: NodeId,
    pub index: usize,
    pub screen_pos: Pos2,
    pub screen_size: Vec2,
    pub source: &'a mut String,
    pub selected: bool,
    pub inlet_pts: &'a [f32],
    pub outlet_pts: &'a [f32],
    pub zoom: f32,
    pub clip: Rect,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CodeboxResponse {
    pub id: NodeId,
    pub index: usize,
    pub body: bool,
    pub clicked: bool,
    pub double_clicked: bool,
    pub changed: bool,
    pub editor_focused: bool,
}

impl CodeboxResponse {
    pub fn clicked(&self) -> bool {
        self.clicked
    }
    pub fn double_clicked(&self) -> bool {
        self.double_clicked
    }
    pub fn interact_pointer_pos(&self) -> Option<Pos2> {
        None
    }
    pub fn dragged(&self) -> bool {
        false
    }
    pub fn drag_started(&self) -> bool {
        false
    }
}

pub fn lop_expr_layout_job(_source: &str, _font: FontId, _text_color: impl Into<eframe::egui::Color32>) -> LayoutJob {
    LayoutJob::default()
}
