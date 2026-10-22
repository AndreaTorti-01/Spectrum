//! Custom-drawn Win32/GDI user interface: theme and drawing helpers, widgets and the individual windows.
//!
//! The UI reads state only through [`snapshot`] and sends intents only through [`dispatch`]. Both forward to
//! `crate::app` unless a mock state is installed with [`set_mock`] (used by `examples/uipreview.rs`).

pub mod draw;
pub mod gamut_window;
pub mod main_window;
pub mod overlay;
pub mod widgets;

use crate::app::{self, UiAction, UiState};
use std::cell::RefCell;

thread_local! {
    static MOCK: RefCell<Option<UiState>> = const { RefCell::new(None) };
}

/// Installs (or clears) a mock data provider. While set, the UI reads this state instead of
/// `app::snapshot()`, and `dispatch` only logs and applies the obvious effect to the mock state
/// (SDR choice, HDR on/off, HDR parameters, autostart) instead of calling `app::dispatch`.
/// Intended for previews and tests; must be called on the UI thread.
pub fn set_mock(state: Option<UiState>) {
    MOCK.with(|m| *m.borrow_mut() = state);
}

/// True while a mock state is installed.
pub fn is_mock() -> bool {
    MOCK.with(|m| m.borrow().is_some())
}

/// Current state: the mock if set, otherwise `app::snapshot()`.
pub fn snapshot() -> UiState {
    if let Some(s) = MOCK.with(|m| m.borrow().clone()) {
        return s;
    }
    app::snapshot()
}

/// Sends a user intent. With a mock installed it is logged and applied to the mock state; the main window
/// is refreshed afterwards either way.
pub fn dispatch(a: UiAction) {
    if is_mock() {
        crate::util::log(&format!("ui(mock) dispatch {a:?}"));
        mock_apply(&a);
    } else {
        app::dispatch(a.clone());
    }
    if !matches!(a, UiAction::CloseMain) && main_window::is_open() {
        main_window::refresh();
    }
}

fn mock_apply(a: &UiAction) {
    use crate::monitors::Rect;
    // Overlays and the gamut window are driven by the app in real mode; do it here for previews.
    let rects: Vec<(Rect, u32, u32)> = MOCK.with(|m| {
        m.borrow()
            .as_ref()
            .map(|s| s.monitors.iter().map(|u| (u.rect, u.num, u.accent)).collect())
            .unwrap_or_default()
    });
    MOCK.with(|m| {
        let mut b = m.borrow_mut();
        let Some(s) = b.as_mut() else { return };
        match a {
            UiAction::SetSdr { mon, idx } => {
                if let Some(u) = s.monitors.get_mut(*mon) {
                    u.sdr_current = Some(*idx);
                }
            }
            UiAction::SetHdrEnabled { mon, on } => {
                if let Some(u) = s.monitors.get_mut(*mon) {
                    u.hdr_on = *on;
                }
            }
            UiAction::ApplyHdr { mon, params } => {
                if let Some(u) = s.monitors.get_mut(*mon) {
                    u.hdr = *params;
                    u.hdr_custom_active = true;
                }
            }
            UiAction::ResetHdr { mon } => {
                if let Some(u) = s.monitors.get_mut(*mon) {
                    u.hdr_custom_active = false;
                }
            }
            UiAction::SetAutostart(on) => s.autostart = *on,
            _ => {}
        }
    });
    match a {
        UiAction::Highlight(Some(i)) => {
            if let Some((r, n, c)) = rects.get(*i) {
                overlay::show(*r, *n, *c);
            }
        }
        UiAction::Highlight(None) => overlay::hide_all(),
        UiAction::Identify => overlay::identify_all(&rects),
        UiAction::OpenGamut => gamut_window::open(None),
        _ => {}
    }
}
