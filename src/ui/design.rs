//! Spectrum's design language. Every window builds its layout from these tokens and draws with the
//! components in `widgets`, so spacing, sizes and states stay the same everywhere.
//!
//! The rules, adapted from Fluent 2 (the Windows 11 design system) and Nielsen's usability heuristics:
//!
//! - Spacing comes from a 4 px grid: `SPACE_*`. Windows are padded by `PAD` on every side.
//! - One control height, `CONTROL_H` (32 px, the Fluent standard). Rows of label + control are `ROW_H`
//!   apart. Hit targets are never smaller than the control height.
//! - Corners: `RADIUS_CONTROL` on buttons, inputs and checkboxes; `RADIUS_CARD` on containers and tiles.
//! - Type ramp (Fluent): caption 12, body 14, body strong 14 semibold, subtitle 20 semibold, display 28.
//!   Section headings are subtitles. Secondary information is caption in `text_dim`.
//! - Every clickable action is a button (`widgets::draw_button`), never bare colored text. Variants:
//!   `Primary` for the single most important action of a view (at most one), `Secondary` for everything
//!   else, `Danger` for destructive actions, which always ask for a second click. Disclosures use the
//!   expander row, toggles switch a setting on and off immediately, checkboxes are for preferences.
//! - The accent color marks state (selection, on, progress), not decoration. Text on the window
//!   background uses `text` or `text_dim`.
//! - Every interactive element has hover, pressed, disabled and keyboard-focus states (2 px focus ring).
//! - Controls that can't act in the current state are hidden when the whole group is irrelevant
//!   (HDR off), and disabled with a short hint otherwise.

use super::draw::{mix, Theme};

/// Window padding on every side.
pub const PAD: i32 = 20;
pub const SPACE_XS: i32 = 4;
pub const SPACE_S: i32 = 8;
pub const SPACE_M: i32 = 12;
pub const SPACE_L: i32 = 16;
pub const SPACE_XL: i32 = 24;

/// Height of buttons, dropdowns and slider hit areas.
pub const CONTROL_H: i32 = 32;
/// Pitch between rows of label + control.
pub const ROW_H: i32 = 36;
/// Height of a section heading line (subtitle).
pub const HEADING_H: i32 = 28;
/// Height of a caption line.
pub const CAPTION_H: i32 = 18;

pub const RADIUS_CONTROL: f32 = 4.0;
pub const RADIUS_CARD: f32 = 8.0;

pub const TOGGLE_W: i32 = 40;
pub const TOGGLE_H: i32 = 20;
pub const CHECK: i32 = 20;

/// Type ramp in pixels at 96 dpi.
pub const FONT_CAPTION: i32 = 12;
pub const FONT_BODY: i32 = 14;
pub const FONT_SUBTITLE: i32 = 20;
pub const FONT_DISPLAY: i32 = 28;

/// Text and fills for destructive actions.
pub fn danger(th: &Theme) -> u32 {
    if th.dark {
        0xFF99A4
    } else {
        0xC42B1C
    }
}

/// Warnings that are not errors (a value above the monitor's peak).
pub fn warning(th: &Theme) -> u32 {
    if th.dark {
        0xFFB347
    } else {
        0xB45309
    }
}

/// Text color for a disabled label.
pub fn disabled_text(th: &Theme) -> u32 {
    mix(th.text, th.bg, 0.55)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_and_sizes_sit_on_the_4px_grid() {
        for v in [
            PAD, SPACE_XS, SPACE_S, SPACE_M, SPACE_L, SPACE_XL, CONTROL_H, ROW_H, HEADING_H, TOGGLE_W,
            TOGGLE_H, CHECK,
        ] {
            assert_eq!(v % 4, 0, "{v}");
        }
        const { assert!(ROW_H > CONTROL_H, "rows leave a gap between controls") };
        const { assert!(SPACE_XS < SPACE_S && SPACE_S < SPACE_M && SPACE_M < SPACE_L && SPACE_L < SPACE_XL) };
        const { assert!(FONT_CAPTION < FONT_BODY && FONT_BODY < FONT_SUBTITLE && FONT_SUBTITLE < FONT_DISPLAY) };
    }

    #[test]
    fn state_colors_differ_per_theme() {
        let light = Theme {
            dark: false,
            bg: 0xF7F5F2,
            surface: 0xFFFFFF,
            text: 0x1A1917,
            text_dim: 0x6B655D,
            line: 0xE2DDD5,
            accent: 0xFF5A3C,
        };
        let dark = Theme { dark: true, ..light };
        assert_ne!(danger(&light), danger(&dark));
        assert_ne!(warning(&light), warning(&dark));
        assert_eq!(disabled_text(&light), mix(light.text, light.bg, 0.55));
    }
}
