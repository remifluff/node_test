//! Mouse-driven patch canvas: shared canvas, styling, and node widgets.
//!
//! API surface modelled after `lop-node-editor` for drop-in compatibility.

pub mod align;
pub mod canvas;
pub mod codebox_editor;
pub mod column_layout;
pub mod crash_log;
pub mod flags;
pub mod layout;
pub mod node_autocomplete;
pub mod node_widget;
pub mod object_ui;
pub mod operator_library;
pub mod sort;
pub mod style;
pub mod theme;

pub use canvas::{BodyDragProbe, Drag, MouseDebugInfo, NodeEditor, PatchState};
pub use codebox_editor::{CodeboxResponse, CodeboxWidget, lop_expr_layout_job};
pub use flags::{CableStyle, Flags};
pub use node_widget::{
    drag_response_screen_delta, layout_job, show_drag_area, NodeResponse, NodeWidget,
};
pub use sort::sort_patch;
pub use theme::EditorTheme;
