#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CableStyle {
    #[default]
    Bezier,
    RightAngle,
    Straight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flags {
    pub snap_to_grid: bool,
    pub cable_style: CableStyle,
    pub show_grid: bool,
    pub direct_cables: bool,
    pub show_cursor: bool,
    pub show_cursor_preview: bool,
    pub grid_connect: bool,
    pub live_sort: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            snap_to_grid: true,
            cable_style: CableStyle::default(),
            show_grid: true,
            direct_cables: true,
            show_cursor: false,
            show_cursor_preview: false,
            grid_connect: false,
            live_sort: true,
        }
    }
}
