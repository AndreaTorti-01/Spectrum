//! Drawing foundation: theme tokens (light/dark), DPI scaling, cached fonts, anti-aliased shape painting
//! on a 32-bit DIB back buffer, and GDI text.
//!
//! Shapes (rounded rects, circles, lines) are rasterised here with signed-distance coverage so they are
//! anti-aliased without GDI+; text goes through GDI (ClearType) onto the same DIB.

mod canvas;
mod dpi;
mod fonts;
mod rect;
mod text;
mod theme;

pub use canvas::{blend_px, sd_rrect, Canvas, Dib, Shadow};
pub use dpi::{
    apply_window_theme, frame_dpi, sc, scf, set_ui_scale, system_dpi, trim_working_set, window_dpi,
};
pub use fonts::{display_face, font_exists, make_font_px, Fonts};
pub use rect::R;
pub use text::{TA_CENTER, TA_LEFT, TA_RIGHT};
pub use theme::{
    colorref, forced_theme, invalidate_theme, luminance, mix, on_color, set_forced_theme, theme, Theme,
    DEFAULT_ACCENT,
};

/// WM_MOUSELEAVE, re-exported for the UI modules.
pub use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
