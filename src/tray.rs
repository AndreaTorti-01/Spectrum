//! Notification-area icon (Shell_NotifyIconW v4) and the right-click profile menu.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::util::wide;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, InsertMenuItemW, PostMessageW,
    SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenu, HICON, HMENU, MENUITEMINFOW, MFS_CHECKED,
    MFS_DISABLED, MFT_RADIOCHECK, MFT_SEPARATOR, MFT_STRING, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING,
    MIIM_SUBMENU, SM_CXSMICON, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_NULL,
};

pub const WM_TRAY: u32 = WM_APP + 1;

const TRAY_UID: u32 = 1;
const ID_OPEN: u32 = 1;
const ID_EXIT: u32 = 2;
const ID_HDR_BASE: u32 = 900;
const ID_SDR_BASE: u32 = 1000;
/// Menu items per monitor are limited so ids stay decodable.
const MAX_PER_MONITOR: usize = 99;

/// The HICON currently owned by the tray entry (0 = none).
static ICON: AtomicIsize = AtomicIsize::new(0);

pub struct MenuMonitor {
    /// Submenu title for multi-monitor setups, e.g. "1 \u{b7} MAG 271QPX E2". If it does not start with a
    /// digit, "<n> \u{b7} " is prepended.
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

fn notify_data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is plain data; all-zero is a valid starting point.
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    for (i, c) in "Spectrum".encode_utf16().enumerate() {
        nid.szTip[i] = c;
    }
    nid
}

fn make_icon() -> HICON {
    // SAFETY: plain queries.
    let px = unsafe {
        let dpi = GetDpiForSystem();
        GetSystemMetricsForDpi(SM_CXSMICON, if dpi == 0 { 96 } else { dpi })
    };
    crate::icon::create_hicon(px.max(16) as u32)
}

fn swap_icon(new: HICON) {
    let old = ICON.swap(new as isize, Ordering::Relaxed);
    if old != 0 {
        // SAFETY: the old icon was created by us and the shell no longer references it after NIM_ADD/NIM_MODIFY.
        unsafe {
            DestroyIcon(old as HICON);
        }
    }
}

pub fn add(hwnd: HWND) -> bool {
    let icon = make_icon();
    let mut nid = notify_data(hwnd, icon);
    // SAFETY: nid is fully initialised and outlives the calls.
    let ok = unsafe {
        let ok = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
        if ok {
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
        ok
    };
    if ok {
        swap_icon(icon);
    } else if !icon.is_null() {
        // SAFETY: icon created above and not handed to the shell.
        unsafe {
            DestroyIcon(icon);
        }
    }
    ok
}

pub fn remove(hwnd: HWND) {
    let nid = notify_data(hwnd, std::ptr::null_mut());
    // SAFETY: nid initialised.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
    swap_icon(std::ptr::null_mut());
}

/// Re-adds the icon after TaskbarCreated.
pub fn readd(hwnd: HWND) {
    add(hwnd);
}

/// Opt the process' popup menus into the system dark mode (undocumented uxtheme ordinals 135/136).
fn allow_dark_menus() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: calling documented-by-practice uxtheme exports by ordinal; failure is silently ignored.
        unsafe {
            let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
            if lib.is_null() {
                return;
            }
            if let Some(set_mode) = GetProcAddress(lib, 135usize as *const u8) {
                let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
                f(1); // AllowDark
            }
            if let Some(flush) = GetProcAddress(lib, 136usize as *const u8) {
                let f: unsafe extern "system" fn() = std::mem::transmute(flush);
                f();
            }
        }
    });
}

unsafe fn insert_item(menu: HMENU, id: u32, text: &str, state: u32, radio: bool, submenu: HMENU) {
    let mut w = wide(text);
    let mut mii: MENUITEMINFOW = std::mem::zeroed();
    mii.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
    mii.fMask = MIIM_FTYPE | MIIM_STATE | MIIM_STRING | MIIM_ID;
    mii.fType = MFT_STRING | if radio { MFT_RADIOCHECK } else { 0 };
    mii.fState = state;
    mii.wID = id;
    mii.dwTypeData = w.as_mut_ptr();
    if !submenu.is_null() {
        mii.fMask |= MIIM_SUBMENU;
        mii.hSubMenu = submenu;
    }
    InsertMenuItemW(menu, u32::MAX, 1, &mii);
}

unsafe fn insert_separator(menu: HMENU) {
    let mut mii: MENUITEMINFOW = std::mem::zeroed();
    mii.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
    mii.fMask = MIIM_FTYPE;
    mii.fType = MFT_SEPARATOR;
    InsertMenuItemW(menu, u32::MAX, 1, &mii);
}

/// Profiles (radio-checked current), separator, HDR check item.
unsafe fn fill_monitor_menu(menu: HMENU, mon_idx: usize, m: &MenuMonitor) {
    for (i, (label, _file)) in m.profiles.iter().take(MAX_PER_MONITOR).enumerate() {
        let state = if m.current == Some(i) { MFS_CHECKED } else { 0 };
        insert_item(
            menu,
            ID_SDR_BASE + (mon_idx * 100 + i) as u32,
            label,
            state,
            true,
            std::ptr::null_mut(),
        );
    }
    if !m.profiles.is_empty() {
        insert_separator(menu);
    }
    let mut state = 0;
    if m.hdr_on {
        state |= MFS_CHECKED;
    }
    if !m.hdr_supported {
        state |= MFS_DISABLED;
    }
    insert_item(
        menu,
        ID_HDR_BASE + mon_idx as u32,
        "HDR",
        state,
        false,
        std::ptr::null_mut(),
    );
}

/// TrackPopupMenu with TPM_RETURNCMD | TPM_RIGHTBUTTON; SetForegroundWindow first, WM_NULL after.
pub fn show_menu(hwnd: HWND, monitors: &[MenuMonitor]) -> Option<TrayCmd> {
    allow_dark_menus();
    // SAFETY: menu handles are created, used and destroyed within this call.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        if monitors.len() == 1 {
            fill_monitor_menu(menu, 0, &monitors[0]);
            insert_separator(menu);
        } else if monitors.len() > 1 {
            for (i, m) in monitors.iter().enumerate().take(MAX_PER_MONITOR) {
                let sub = CreatePopupMenu();
                fill_monitor_menu(sub, i, m);
                let label = if m
                    .label
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                {
                    m.label.clone()
                } else {
                    format!("{} \u{b7} {}", i + 1, m.label)
                };
                // the submenu handle is owned (and destroyed) by the parent menu
                insert_item(menu, 0, &label, 0, false, sub);
            }
            insert_separator(menu);
        }
        insert_item(menu, ID_OPEN, "Open Spectrum", 0, false, std::ptr::null_mut());
        insert_item(menu, ID_EXIT, "Exit", 0, false, std::ptr::null_mut());
        SetMenuDefaultItem(menu, ID_OPEN, 0);

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let id = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if id > 0 {
            decode_cmd(id as u32)
        } else {
            None
        }
    }
}

/// 1 = Open, 2 = Exit, 900+mon = ToggleHdr, 1000+mon*100+idx = SetSdr.
pub fn decode_cmd(id: u32) -> Option<TrayCmd> {
    match id {
        ID_OPEN => Some(TrayCmd::Open),
        ID_EXIT => Some(TrayCmd::Exit),
        ID_HDR_BASE..=999 => Some(TrayCmd::ToggleHdr {
            mon: (id - ID_HDR_BASE) as usize,
        }),
        i if i >= ID_SDR_BASE => {
            let n = i - ID_SDR_BASE;
            Some(TrayCmd::SetSdr {
                mon: (n / 100) as usize,
                idx: (n % 100) as usize,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fixed_ids() {
        assert_eq!(decode_cmd(1), Some(TrayCmd::Open));
        assert_eq!(decode_cmd(2), Some(TrayCmd::Exit));
        assert_eq!(decode_cmd(0), None);
        assert_eq!(decode_cmd(3), None);
        assert_eq!(decode_cmd(899), None);
    }

    #[test]
    fn decodes_hdr_ids() {
        assert_eq!(decode_cmd(900), Some(TrayCmd::ToggleHdr { mon: 0 }));
        assert_eq!(decode_cmd(902), Some(TrayCmd::ToggleHdr { mon: 2 }));
    }

    #[test]
    fn decodes_sdr_ids() {
        assert_eq!(decode_cmd(1000), Some(TrayCmd::SetSdr { mon: 0, idx: 0 }));
        assert_eq!(decode_cmd(1003), Some(TrayCmd::SetSdr { mon: 0, idx: 3 }));
        assert_eq!(
            decode_cmd(1000 + 2 * 100 + 7),
            Some(TrayCmd::SetSdr { mon: 2, idx: 7 })
        );
        assert_eq!(decode_cmd(1000 + 99), Some(TrayCmd::SetSdr { mon: 0, idx: 99 }));
    }

    #[test]
    fn menu_ids_roundtrip() {
        for mon in 0..4usize {
            for idx in [0usize, 1, 50, 98] {
                let id = ID_SDR_BASE + (mon * 100 + idx) as u32;
                assert_eq!(decode_cmd(id), Some(TrayCmd::SetSdr { mon, idx }));
            }
            assert_eq!(
                decode_cmd(ID_HDR_BASE + mon as u32),
                Some(TrayCmd::ToggleHdr { mon })
            );
        }
    }
}
