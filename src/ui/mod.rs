//! Custom-drawn Win32/GDI user interface: theme and drawing helpers, widgets and the individual windows.
//!
//! The UI reads state only through [`snapshot`] and sends intents only through [`dispatch`]. Both forward to
//! `crate::app` unless a mock state is installed with [`set_mock`] (used by `examples/uipreview.rs`).

pub mod design;
pub mod draw;
pub mod frame;
pub mod gamut_window;
pub mod main_window;
pub mod mockbar;
pub mod overlay;
pub mod profiles_window;
pub mod traymenu;
pub mod widgets;
pub mod window;

use crate::app::{self, UiAction, UiState};
use crate::monitors::Rect;
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
pub fn dispatch(action: UiAction) {
    if is_mock() {
        crate::util::log(&format!("ui(mock) dispatch {action:?}"));
        mock_apply(&action);
    } else {
        app::dispatch(action.clone());
    }
    if !matches!(action, UiAction::CloseMain) && main_window::is_open() {
        main_window::refresh();
    }
}

/// The obvious effect of `action` on a mock state. Window actions are left to [`mock_apply`].
fn apply_to_state(state: &mut UiState, action: &UiAction) {
    fn monitor(state: &mut UiState, mon: usize) -> Option<&mut app::UiMonitor> {
        state.monitors.get_mut(mon)
    }
    match action {
        UiAction::SetSdr { mon, idx } => {
            if let Some(u) = monitor(state, *mon) {
                u.sdr_current = Some(*idx);
            }
        }
        UiAction::SetHdrEnabled { mon, on } => {
            if let Some(u) = monitor(state, *mon) {
                u.hdr_on = *on;
            }
        }
        UiAction::ApplyHdr { mon, params } => {
            if let Some(u) = monitor(state, *mon) {
                u.hdr = *params;
                u.hdr_custom_active = true;
            }
        }
        UiAction::SetSdrWhite { mon, nits } => {
            if let Some(u) = monitor(state, *mon) {
                u.sdr_white_nits = Some(*nits);
            }
        }
        UiAction::ResetHdr { mon } => {
            if let Some(u) = monitor(state, *mon) {
                u.hdr = app::hdr_defaults_from(u.edid_hdr.as_ref());
                u.hdr_custom_active = true;
            }
        }
        UiAction::SetAutostart(on) => state.autostart = *on,
        UiAction::SetAdvanced(on) => state.advanced = *on,
        _ => {}
    }
}

fn mock_apply(action: &UiAction) {
    // The identify overlay and the gamut window are driven by the app in real mode; do it here for previews.
    let rects: Vec<(Rect, u32, u32)> = MOCK.with(|m| {
        m.borrow()
            .as_ref()
            .map(|s| s.monitors.iter().map(|u| (u.rect, u.num, u.accent)).collect())
            .unwrap_or_default()
    });
    MOCK.with(|m| {
        if let Some(state) = m.borrow_mut().as_mut() {
            apply_to_state(state, action);
        }
    });
    match action {
        UiAction::Identify => overlay::identify_all(&rects),
        UiAction::OpenGamut => gamut_window::open(None, main_window::selected()),
        UiAction::OpenProfiles => profiles_window::open_showcase(crate::debug::showcase_profiles()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icc_write::HdrParams;

    #[test]
    fn mock_state_takes_the_obvious_effect() {
        let mut st = crate::showcase::state();
        apply_to_state(&mut st, &UiAction::SetSdr { mon: 0, idx: 3 });
        apply_to_state(&mut st, &UiAction::SetHdrEnabled { mon: 0, on: false });
        apply_to_state(&mut st, &UiAction::SetSdrWhite { mon: 0, nits: 300.0 });
        apply_to_state(&mut st, &UiAction::SetAutostart(false));
        apply_to_state(&mut st, &UiAction::SetAdvanced(false));
        // Out-of-range monitors are ignored.
        apply_to_state(&mut st, &UiAction::SetSdr { mon: 9, idx: 1 });
        let m = &st.monitors[0];
        assert_eq!(m.sdr_current, Some(3));
        assert!(!m.hdr_on);
        assert_eq!(m.sdr_white_nits, Some(300.0));
        assert!(!st.autostart && !st.advanced);

        let params = HdrParams {
            min_nits: 0.1,
            peak_nits: 800.0,
            full_frame_nits: 300.0,
            saturation: 0.5,
        };
        apply_to_state(&mut st, &UiAction::ApplyHdr { mon: 0, params });
        assert_eq!(st.monitors[0].hdr, params);
        apply_to_state(&mut st, &UiAction::ResetHdr { mon: 0 });
        let reset = st.monitors[0].hdr;
        assert_eq!(reset.saturation, 0.0);
        assert!((reset.peak_nits - 455.5).abs() < 1.0, "{reset:?}");
    }
}
