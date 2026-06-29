#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod graph;
mod layout_adapter;
mod node_autocomplete;
mod operator_library;
mod patch_export;
mod style;

use eframe::egui;
use graph::PdPatchEditor;

fn main() -> eframe::Result {
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pd-style patch")
            .with_inner_size([1024.0, 720.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Pd-style patch",
        native_options,
        Box::new(|cc| {
            style::apply_interlay_visuals(&cc.egui_ctx);
            Ok(Box::new(PdPatchEditor::demo_patch()))
        }),
    )
}
