use crate::graph::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowDirection {
    LeftToRight,
    TopToBottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutConfig {
    pub column_spacing: f32,
    pub row_spacing: f32,
    pub grid_step: f32,
    pub origin: Point,
    pub flow: FlowDirection,
    /// When true, `[out]` / `DelayIn` kinds are pushed to the maximum rank column.
    pub pin_sinks_right: bool,
    /// When true, `[in]` / `DelayOut` kinds start at rank zero.
    pub pin_sources_left: bool,
    /// Minimum horizontal gap between layout columns (unit bounding boxes).
    pub min_column_gap: f32,
    /// Minimum vertical gap between layout rows (unit bounding boxes).
    pub min_row_gap: f32,
    /// Padding between node bounding boxes within a pass-through block.
    pub node_gap: f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            column_spacing: 120.0,
            row_spacing: 80.0,
            grid_step: 15.0,
            origin: Point { x: 80.0, y: 80.0 },
            flow: FlowDirection::LeftToRight,
            pin_sinks_right: true,
            pin_sources_left: true,
            min_column_gap: 30.0,
            min_row_gap: 30.0,
            node_gap: 8.0,
        }
    }
}

impl LayoutConfig {
    /// Horizontal gap between layout columns and parallel units.
    pub fn column_gap(&self) -> f32 {
        self.min_column_gap
    }

    /// Preferred vertical gap when stacking units in a chain (never below [`min_row_gap`]).
    pub fn row_gap(&self) -> f32 {
        self.row_spacing.max(self.min_row_gap)
    }

    pub fn snap(&self, value: f32) -> f32 {
        if self.grid_step <= 0.0 {
            return value;
        }
        (value / self.grid_step).round() * self.grid_step
    }

    pub fn snap_point(&self, point: Point) -> Point {
        Point {
            x: self.snap(point.x),
            y: self.snap(point.y),
        }
    }
}
