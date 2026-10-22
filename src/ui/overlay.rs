//! Click-through topmost per-monitor overlay (colored border + big number) used to identify screens.

use crate::monitors::Rect;

pub fn show(_rect: Rect, _num: u32, _accent: u32) {
    todo!("ui::overlay::show")
}

pub fn hide_all() {
    todo!("ui::overlay::hide_all")
}

/// `(rect, num, accent)` per monitor; auto-hides after 2.5 s.
pub fn identify_all(_mons: &[(Rect, u32, u32)]) {
    todo!("ui::overlay::identify_all")
}
