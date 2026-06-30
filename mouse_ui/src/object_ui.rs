use egui::{self, pos2, Id, Key, RichText, Sense, Ui, Vec2};
use patch_graph::PdObject;

use crate::node_autocomplete::{show_operator_autocomplete, show_sized_text_edit};
use crate::operator_library::OperatorLibrary;
use crate::style::{label_font, layout_job, INK, LABEL_INSET_X, PAPER, PAPER_DIM};

#[derive(Default)]
pub struct NodeAreaBody {
    pub clicked_label: bool,
    pub commit_edit: bool,
    pub cancel_edit: bool,
}

pub fn show_display_ui(
    object: &PdObject,
    ui: &mut Ui,
    label: &str,
    selected: bool,
    zoom: f32,
) -> NodeAreaBody {
    let mut body = NodeAreaBody::default();

    if object.is_comment() {
        let label_response = ui.label(
            RichText::new(label).font(label_font(zoom)).color(PAPER_DIM),
        );
        if label_response.clicked() {
            body.clicked_label = true;
        }
        return body;
    }

    let font = label_font(zoom);
    let job = layout_job(label, font, selected);
    let galley = ui.painter().layout_job(job);
    let op_color = if selected { INK } else { PAPER };
    let (rect, label_response) =
        ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::click());
    ui.painter().galley(
        pos2(
            rect.min.x + LABEL_INSET_X * zoom,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        op_color,
    );
    if label_response.clicked() {
        body.clicked_label = true;
    }
    body
}

pub fn show_edit_ui(
    object: &PdObject,
    ui: &mut Ui,
    buffer: &mut String,
    edit_id: Id,
    zoom: f32,
    library: &OperatorLibrary,
) -> NodeAreaBody {
    let mut body = NodeAreaBody::default();
    let font = label_font(zoom);
    let inner_width = ui.available_width();
    let inner_height = ui.available_height();
    let skip_autocomplete =
        object.is_comment() || matches!(object, PdObject::Message { .. });

    let edit = if object.is_comment() || matches!(object, PdObject::Message { .. }) {
        egui::TextEdit::multiline(buffer)
            .font(font)
            .desired_width(inner_width)
            .desired_rows(1)
            .margin(egui::Margin::symmetric(LABEL_INSET_X as i8, 2))
            .text_color(INK)
            .background_color(PAPER)
            .frame(egui::Frame::NONE)
    } else {
        egui::TextEdit::singleline(buffer)
            .font(font)
            .desired_width(inner_width)
            .margin(egui::Margin::symmetric(LABEL_INSET_X as i8, 2))
            .text_color(INK)
            .background_color(PAPER)
            .frame(egui::Frame::NONE)
    };

    let output = show_sized_text_edit(
        ui,
        Vec2::new(inner_width, inner_height),
        edit.id(edit_id.with("edit")),
    );
    output.response.request_focus();

    let ac = show_operator_autocomplete(
        ui,
        edit_id,
        output.response.rect,
        buffer,
        output.cursor_range,
        library,
        skip_autocomplete,
    );

    if !object.is_comment()
        && !matches!(object, PdObject::Message { .. })
        && ui.input(|i| i.key_pressed(Key::Enter))
    {
        body.commit_edit = true;
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        if ac.dismiss_popup {
            ui.input_mut(|i| i.consume_key(Default::default(), Key::Escape));
        } else {
            body.cancel_edit = true;
        }
    }
    body
}
