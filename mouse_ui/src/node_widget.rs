use egui::{self, Context, Id, Pos2, Rect, Sense, Ui, Vec2};

use patch_graph::NodeId;

use crate::layout::label_font;

#[derive(Clone, Copy, Debug, Default)]
pub struct NodeResponse {
    pub id: NodeId,
    pub index: usize,
    pub body: bool,
    pub clicked: bool,
    pub double_clicked: bool,
    pub drag_started: bool,
    pub dragged: bool,
    pub ports: bool,
}

impl NodeResponse {
    pub fn clicked(&self) -> bool {
        self.clicked
    }

    pub fn double_clicked(&self) -> bool {
        self.double_clicked
    }

    pub fn drag_started(&self) -> bool {
        self.drag_started
    }

    pub fn dragged(&self) -> bool {
        self.dragged
    }

    pub fn interact_pointer_pos(&self) -> Option<Pos2> {
        None
    }
}

#[derive(Debug)]
pub struct NodeWidget<'a> {
    pub id: NodeId,
    pub index: usize,
    pub screen_pos: Pos2,
    pub screen_size: Vec2,
    pub label: &'a str,
    pub selected: bool,
    pub editing: bool,
    pub inlet_pts: &'a [f32],
    pub outlet_pts: &'a [f32],
    pub zoom: f32,
    pub clip: Rect,
}

impl<'a> NodeWidget<'a> {
    pub fn show(self, ui: &mut Ui) -> NodeResponse {
        let mut resp = NodeResponse {
            id: self.id,
            index: self.index,
            ..Default::default()
        };

        if self.editing {
            return resp;
        }

        let font = label_font(self.zoom);
        let galley = ui.painter().layout_job(crate::style::layout_job(
            self.label,
            font,
            self.selected,
        ));
        let (_rect, label_response) =
            ui.allocate_exact_size(self.screen_size, Sense::click());
        ui.painter().galley(
            egui::pos2(
                self.screen_pos.x + crate::style::LABEL_INSET_X * self.zoom,
                self.screen_pos.y + self.screen_size.y * 0.5 - galley.size().y * 0.5,
            ),
            galley,
            if self.selected {
                crate::style::INK
            } else {
                crate::style::PAPER
            },
        );

        if label_response.clicked() {
            resp.clicked = true;
        }
        if label_response.double_clicked() {
            resp.double_clicked = true;
        }
        if label_response.drag_started() {
            resp.drag_started = true;
        }
        if label_response.dragged() {
            resp.dragged = true;
        }

        resp
    }
}

/// Show a draggable egui `Area` for node repositioning.
pub fn show_drag_area(ctx: &Context, id: Id, rect: Rect) -> egui::Response {
    egui::Area::new(id)
        .fixed_pos(rect.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.allocate_exact_size(rect.size(), Sense::drag())
        })
        .response
}

/// Screen-space drag delta from a drag response.
pub fn drag_response_screen_delta(resp: &egui::Response) -> Vec2 {
    resp.drag_delta()
}

/// Re-exported from `crate::style` for convenience.
pub use crate::style::layout_job;
