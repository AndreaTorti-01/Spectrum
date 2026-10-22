//! Application state and glue: UI snapshot/dispatch, reconcile logic and the hidden message-window loop.

use crate::icc_write::HdrParams;
use crate::monitors::Rect;

#[derive(Clone, Debug)]
pub struct UiMonitor {
    pub num: u32,
    pub rect: Rect,
    pub accent: u32,
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub hdr_supported: bool,
    pub hdr_on: bool,
    /// (label, file)
    pub sdr_options: Vec<(String, String)>,
    pub sdr_current: Option<usize>,
    pub has_edid: bool,
    pub hdr: HdrParams,
    pub hdr_custom_active: bool,
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub monitors: Vec<UiMonitor>,
    pub autostart: bool,
    pub status: Option<String>,
}

#[derive(Clone, Debug)]
pub enum UiAction {
    SetSdr { mon: usize, idx: usize },
    ReadEdid { mon: usize },
    SetHdrEnabled { mon: usize, on: bool },
    ApplyHdr { mon: usize, params: HdrParams },
    ResetHdr { mon: usize },
    SetAutostart(bool),
    OpenGamut,
    Identify,
    Highlight(Option<usize>),
    CloseMain,
}

pub fn snapshot() -> UiState {
    todo!("app::snapshot")
}

/// Synchronous; the UI calls `snapshot()` again afterwards.
pub fn dispatch(_a: UiAction) {
    todo!("app::dispatch")
}

/// Creates the hidden message window and tray icon, reconciles, runs the message loop.
pub fn run_tray() -> i32 {
    todo!("app::run_tray")
}
