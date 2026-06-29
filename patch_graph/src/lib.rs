//! Patch graph domain model, parsing, export, and automatic layout.

pub mod export;
pub mod graph;
pub mod graph_ops;
pub mod layout;
pub mod layout_adapter;
pub mod node;
pub mod object;
pub mod parse;
pub mod sizing;

pub use export::{export_patch, format_coord, quote_lop_string, PatchView};
pub use graph::{EdgeId, NodeId, PatchGraph};
pub use graph_ops::{cycle_vertical_bounds, find_cycle_nodes, find_path_nodes};
pub use layout::{
    layout_patch, apply_positions, LayoutConfig, LayoutEdge, LayoutGraph, LayoutNavCell,
    LayoutNode, LayoutResult, LayoutEngine, LayeredDagLayout, NodeKind, Point, SugiyamaLayout,
};
pub use layout_adapter::{
    apply_layout_to_patch, layout_preview, layout_preview_cached, layout_topology_fingerprint,
    organize_patch, LayoutPreview,
};
pub use node::{EdgeData, Node};
pub use object::PdObject;
pub use parse::{
    commit_node_label, object_from_label, parse_delay_hex, parse_pd_object_text,
    random_unused_delay_hex,
};
