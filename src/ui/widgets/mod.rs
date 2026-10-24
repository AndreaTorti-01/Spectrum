//! Custom-drawn controls (Button, Toggle, Checkbox, Slider, Dropdown field + popup list) with a small
//! retained widget list for hit-testing and hover/pressed/focus state, plus the slider value mappings and
//! the scrolling list the gamut and profiles windows share.

mod controls;
mod dropdown;
mod list;
mod scroll;
mod slider;

pub use controls::{
    draw_button, draw_checkbox, draw_dropdown, draw_expander, draw_slider, draw_slider_tick, draw_toggle,
    focus_ring, BtnStyle, Ctx,
};
pub use dropdown::{dropdown_is_open, open_dropdown, DROPDOWN_DISMISSED};
pub use list::{Kind, WState, Widget, WidgetList};
pub use scroll::{draw_scrollbar, Press, ScrollList, Scrollbar};
pub use slider::{
    black_scale, full_scale, key_to_t, move_full, move_peak, peak_scale, round_black, round_nits,
    round_sdr_white_nits, sat_scale, sdr_peak_target, slider_span, slider_t_at, white_scale, Scale,
    BLACK_MAX_NITS, PEAK_MAX_NITS, PEAK_MIN_NITS,
};
