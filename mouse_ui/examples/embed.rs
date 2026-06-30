use eframe::egui;
use mouse_ui::{PatchCanvas, style};
use patch_graph::PatchGraph;

struct EmbedApp {
    graph: PatchGraph,
    canvas: PatchCanvas,
}

impl eframe::App for EmbedApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: style::INK,
                inner_margin: egui::Margin::ZERO,
                ..Default::default()
            })
            .show_inside(ui, |ui| {
                self.canvas.ui(ui, &mut self.graph, true);
            });
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PatchCanvas embed")
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "PatchCanvas embed",
        native_options,
        Box::new(|cc| {
            style::apply_interlay_visuals(&cc.egui_ctx);
            Ok(Box::new(EmbedApp {
                graph: PatchGraph::default(),
                canvas: PatchCanvas::default(),
            }))
        }),
    )
}
