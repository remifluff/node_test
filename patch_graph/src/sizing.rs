pub const BOX_H: f32 = 22.0;
pub const GRID_STEP: f32 = 15.0;
pub const LABEL_INSET_X: f32 = 3.0;
pub const END_CAP_W: f32 = 3.0;
pub const CHAR_W: f32 = 6.0;

pub fn strip_brackets(name: &str) -> &str {
    name.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(name)
}

pub fn min_box_width(name: &str, inlets: usize) -> f32 {
    let label = strip_brackets(if name.is_empty() { "?" } else { name });
    let text_w = label.len() as f32 * CHAR_W;
    let label_w = END_CAP_W + LABEL_INSET_X + text_w + CHAR_W + END_CAP_W;
    let inlet_w = inlets.max(1) as f32 * GRID_STEP;
    label_w.max(inlet_w)
}
