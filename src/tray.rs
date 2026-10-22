//! Notification-area icon (Shell_NotifyIconW v4) and the right-click profile menu.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_TRAY: u32 = WM_APP + 1;

pub struct MenuMonitor {
    pub label: String,
    /// (label, file)
    pub profiles: Vec<(String, String)>,
    pub current: Option<usize>,
    pub hdr_supported: bool,
    pub hdr_on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCmd {
    Open,
    Exit,
    SetSdr { mon: usize, idx: usize },
    ToggleHdr { mon: usize },
}

pub fn add(_hwnd: HWND) -> bool {
    todo!("tray::add")
}

pub fn remove(_hwnd: HWND) {
    todo!("tray::remove")
}

/// Re-adds the icon after TaskbarCreated.
pub fn readd(_hwnd: HWND) {
    todo!("tray::readd")
}

/// TrackPopupMenu with TPM_RETURNCMD | TPM_RIGHTBUTTON; SetForegroundWindow first, WM_NULL after.
pub fn show_menu(_hwnd: HWND, _monitors: &[MenuMonitor]) -> Option<TrayCmd> {
    todo!("tray::show_menu")
}

/// 1 = Open, 2 = Exit, 900+mon = ToggleHdr, 1000+mon*100+idx = SetSdr.
pub fn decode_cmd(_id: u32) -> Option<TrayCmd> {
    todo!("tray::decode_cmd")
}
