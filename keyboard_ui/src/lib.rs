//! Keyboard-driven sorted layout pane.

pub mod draw;

use eframe::egui::{Key, Rect, Ui};
use patch_graph::{LayoutPreview, NodeId, PatchGraph};
use petgraph::Direction;

use mouse_ui::canvas::{show_patch_scene, CanvasView};

use crate::draw::paint_scene;

#[derive(Default)]
pub struct KeyboardUi {
    pub view: CanvasView,
    pub focus: Option<NodeId>,
    pub auto_focus: bool,
}

impl KeyboardUi {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        graph: &PatchGraph,
        preview: &LayoutPreview,
        editing_node: Option<NodeId>,
        default_scene: Rect,
    ) {
        self.ensure_focus(graph, preview);
        let keyboard_focus = self.focus;
        let mut scene_rect = self.view.scene_rect;
        if !scene_rect.is_positive() {
            scene_rect = default_scene;
        }

        let parent_id = ui.id();
        let parent_order = ui.layer_id().order;

        let scene_response = show_patch_scene(ui, &mut scene_rect, |scene_ui| {
            paint_scene(
                scene_ui,
                graph,
                preview,
                keyboard_focus,
                parent_id,
                parent_order,
            );
        });

        self.view.scene_rect = scene_rect;

        let scene = &scene_response.response;
        if self.auto_focus {
            scene.request_focus();
            self.auto_focus = false;
        } else if scene.clicked() {
            scene.request_focus();
        }

        let pane_active = (scene.has_focus() || scene.hovered()) && editing_node.is_none();
        self.handle_input(ui, graph, preview, pane_active);
    }

    fn default_focus(graph: &PatchGraph, preview: &LayoutPreview) -> Option<NodeId> {
        for row in &preview.rows {
            for &idx in row {
                let id = NodeId::new(idx);
                if graph.contains_node(id) && !graph[id].object.is_comment() {
                    return Some(id);
                }
            }
        }
        None
    }

    fn ensure_focus(&mut self, graph: &PatchGraph, preview: &LayoutPreview) {
        if let Some(id) = self.focus {
            if graph.contains_node(id)
                && preview.nav.contains_key(&id.index())
                && !graph[id].object.is_comment()
            {
                return;
            }
        }
        self.focus = Self::default_focus(graph, preview);
    }

    fn nav_horizontal(
        focus: NodeId,
        preview: &LayoutPreview,
        delta: i32,
    ) -> Option<NodeId> {
        let cell = preview.nav.get(&focus.index())?;
        let row = preview.rows.get(cell.row as usize)?;
        let new_slot = cell.slot as i32 + delta;
        if new_slot < 0 || new_slot as usize >= row.len() {
            return None;
        }
        Some(NodeId::new(row[new_slot as usize]))
    }

    fn nav_vertical(
        graph: &PatchGraph,
        focus: NodeId,
        preview: &LayoutPreview,
        downstream: bool,
    ) -> Option<NodeId> {
        let direction = if downstream {
            Direction::Outgoing
        } else {
            Direction::Incoming
        };
        let mut candidates: Vec<NodeId> = graph.neighbors_directed(focus, direction).collect();
        candidates.retain(|&id| !graph[id].object.is_comment());
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        let focus_x = preview.positions.get(&focus.index())?.x;
        candidates.sort_by(|a, b| {
            let ax = preview.positions.get(&a.index()).map(|p| p.x).unwrap_or(focus_x);
            let bx = preview.positions.get(&b.index()).map(|p| p.x).unwrap_or(focus_x);
            let da = (ax - focus_x).abs();
            let db = (bx - focus_x).abs();
            da.partial_cmp(&db)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.index().cmp(&b.index()))
        });
        Some(candidates[0])
    }

    fn handle_input(
        &mut self,
        ui: &mut Ui,
        graph: &PatchGraph,
        preview: &LayoutPreview,
        pane_active: bool,
    ) {
        if !pane_active {
            return;
        }

        self.ensure_focus(graph, preview);
        let Some(focus) = self.focus else {
            return;
        };

        let (left, right, up, down) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
            )
        });

        let next = if left {
            Self::nav_horizontal(focus, preview, -1)
        } else if right {
            Self::nav_horizontal(focus, preview, 1)
        } else if up {
            Self::nav_vertical(graph, focus, preview, false)
        } else if down {
            Self::nav_vertical(graph, focus, preview, true)
        } else {
            None
        };

        if let Some(id) = next {
            self.focus = Some(id);
            ui.input_mut(|i| {
                if left {
                    i.consume_key(Default::default(), Key::ArrowLeft);
                }
                if right {
                    i.consume_key(Default::default(), Key::ArrowRight);
                }
                if up {
                    i.consume_key(Default::default(), Key::ArrowUp);
                }
                if down {
                    i.consume_key(Default::default(), Key::ArrowDown);
                }
            });
        }
    }
}
