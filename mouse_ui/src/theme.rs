use std::sync::OnceLock;

use egui::Color32;

use crate::layout::LINE_W;

static THEME: OnceLock<EditorTheme> = OnceLock::new();

#[derive(Clone, Debug, PartialEq)]
pub struct EditorTheme {
    pub ink: Color32,
    pub paper: Color32,
    pub paper_dim: Color32,
    pub ink_panel: Color32,
    pub wire_handle: Color32,
    pub wire_handle_hover: Color32,
    pub line_width: f32,
    pub name_text: Color32,
    pub name_args: Color32,
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self {
            ink: Color32::from_rgb(18, 18, 20),
            paper: Color32::from_rgb(222, 216, 198),
            paper_dim: Color32::from_rgb(120, 116, 104),
            ink_panel: Color32::from_rgb(28, 28, 31),
            wire_handle: Color32::from_rgb(156, 92, 48),
            wire_handle_hover: Color32::from_rgb(196, 128, 72),
            line_width: LINE_W,
            name_text: Color32::from_rgb(222, 216, 198),
            name_args: Color32::from_rgb(120, 116, 104),
        }
    }
}

pub fn init(theme: EditorTheme) {
    THEME.set(theme).ok();
}

fn theme() -> &'static EditorTheme {
    THEME.get().expect("EditorTheme not initialized — call theme::init()")
}

pub fn ink() -> Color32 {
    theme().ink
}

pub fn paper() -> Color32 {
    theme().paper
}

pub fn paper_dim() -> Color32 {
    theme().paper_dim
}

pub fn ink_panel() -> Color32 {
    theme().ink_panel
}

pub fn wire_handle() -> Color32 {
    theme().wire_handle
}

pub fn wire_handle_hover() -> Color32 {
    theme().wire_handle_hover
}

pub fn line_w() -> f32 {
    theme().line_width
}

pub fn name_text() -> Color32 {
    theme().name_text
}

pub fn name_args() -> Color32 {
    theme().name_args
}
