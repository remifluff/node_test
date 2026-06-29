//! Operator autocomplete popup while editing node labels.

use eframe::egui::{
    self, Align, Id, Key, Popup, RichText, ScrollArea, Style, TextEdit, Ui,
};
use eframe::egui::text::CCursorRange;

use crate::operator_library::{OperatorEntry, OperatorLibrary};
use crate::style::{INK, PAPER, PAPER_DIM};

const MAX_SUGGESTIONS: usize = 12;

pub struct AutocompleteAction {
    pub applied: bool,
    pub dismiss_popup: bool,
}

/// Show operator suggestions below a focused text field.
pub fn show_operator_autocomplete(
    ui: &mut Ui,
    edit_id: Id,
    anchor: eframe::egui::Rect,
    buffer: &mut String,
    cursor_range: Option<CCursorRange>,
    library: &OperatorLibrary,
    skip: bool,
) -> AutocompleteAction {
    let mut action = AutocompleteAction {
        applied: false,
        dismiss_popup: false,
    };

    if skip {
        return action;
    }

    let Some(cursor_range) = cursor_range else {
        return action;
    };

    let cursor = cursor_range.primary.index;
    let (token_start, token_end, prefix) = token_at_cursor(buffer, cursor);
    if prefix.is_empty() || buffer[..token_start].contains(char::is_whitespace) {
        return action;
    }

    let matches = library.match_prefix(prefix, MAX_SUGGESTIONS);
    if matches.is_empty() {
        return action;
    }

    let ac_id = edit_id.with("autocomplete");
    let mut selected = ui.data_mut(|d| d.get_temp::<usize>(ac_id).unwrap_or(0));
    if selected >= matches.len() {
        selected = 0;
    }

    let down = ui.input(|i| i.key_pressed(Key::ArrowDown));
    let up = ui.input(|i| i.key_pressed(Key::ArrowUp));
    let tab = ui.input(|i| i.key_pressed(Key::Tab));
    let esc = ui.input(|i| i.key_pressed(Key::Escape));

    if down {
        selected = (selected + 1) % matches.len();
        ui.input_mut(|i| i.consume_key(Default::default(), Key::ArrowDown));
    } else if up {
        selected = selected.checked_sub(1).unwrap_or(matches.len() - 1);
        ui.input_mut(|i| i.consume_key(Default::default(), Key::ArrowUp));
    }

    if tab {
        apply_completion(buffer, token_start, token_end, &matches[selected].name);
        action.applied = true;
        ui.input_mut(|i| i.consume_key(Default::default(), Key::Tab));
        return action;
    }

    if esc {
        action.dismiss_popup = true;
        ui.data_mut(|d| d.remove::<usize>(ac_id));
        return action;
    }

    ui.data_mut(|d| d.insert_temp(ac_id, selected));

    let popup_id = ac_id.with("popup");
    let mut click_apply = None;
    Popup::new(popup_id, ui.ctx().clone(), anchor, ui.layer_id())
        .kind(egui::PopupKind::Popup)
        .width(300.0)
        .gap(2.0)
        .layout(egui::Layout::top_down(Align::LEFT))
        .style(|style: &mut Style| {
            style.spacing.item_spacing.y = 0.0;
        })
        .show(|ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.label(
                    RichText::new(format!("{prefix}"))
                        .monospace()
                        .strong()
                        .color(INK),
                );
                ui.separator();
                ScrollArea::vertical()
                    .max_height(180.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (i, entry) in matches.iter().enumerate() {
                            if row(ui, entry, i == selected).clicked() {
                                click_apply = Some(entry.name.clone());
                            }
                        }
                    });
                ui.label(
                    RichText::new("↑↓ navigate · Tab accept")
                        .small()
                        .color(PAPER_DIM),
                );
            });
        });

    if let Some(name) = click_apply {
        apply_completion(buffer, token_start, token_end, &name);
        action.applied = true;
    }

    action
}

fn row(ui: &mut Ui, entry: &OperatorEntry, selected: bool) -> egui::Response {
    let name = RichText::new(&entry.name)
        .monospace()
        .color(if selected { INK } else { PAPER });
    let mut desc = RichText::new(format!("  {}", entry.description))
        .small()
        .color(PAPER_DIM);
    if selected {
        desc = desc.color(INK.linear_multiply(0.65));
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let name_resp = ui.selectable_label(selected, name);
        ui.label(desc);
        name_resp
    })
    .inner
}

fn token_at_cursor(text: &str, cursor: usize) -> (usize, usize, &str) {
    let cursor = cursor.min(text.len());
    let before = &text[..cursor];
    let after = &text[cursor..];
    let start = before
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let end = after
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| cursor + i)
        .unwrap_or(text.len());
    (start, end, &text[start..end])
}

fn apply_completion(buffer: &mut String, token_start: usize, token_end: usize, name: &str) {
    buffer.replace_range(token_start..token_end, name);
    let needs_space = token_end == buffer.len() || !buffer[token_end..].starts_with(' ');
    if needs_space {
        buffer.insert_str(token_start + name.len(), " ");
    }
}

/// Run a sized [`TextEdit`] and return its output.
pub fn show_sized_text_edit(
    ui: &mut Ui,
    size: eframe::egui::Vec2,
    edit: TextEdit<'_>,
) -> eframe::egui::text_edit::TextEditOutput {
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        edit.show(ui)
    })
    .inner
}
