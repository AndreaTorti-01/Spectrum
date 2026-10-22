//! Drawing foundation: theme tokens (light/dark), DPI scaling, cached fonts, rounded rects, back-buffer.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
    pub bg: u32,
    pub surface: u32,
    pub text: u32,
    pub text_dim: u32,
    pub line: u32,
    pub accent: u32,
}

/// Follows AppsUseLightTheme unless overridden by `set_forced_theme`.
pub fn theme() -> Theme {
    todo!("ui::draw::theme")
}

pub fn set_forced_theme(_dark: Option<bool>) {
    todo!("ui::draw::set_forced_theme")
}
