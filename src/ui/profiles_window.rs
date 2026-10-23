//! The profiles window: every .icc/.icm in the color folder with its type, the monitors using it and a
//! two-step Delete button. Destroyed on close, like the gamut window.

#![allow(clippy::too_many_arguments)]

use super::design;
use super::draw::{
    apply_window_theme, invalidate_theme, mix, sc, scf, system_dpi, theme, trim_working_set, window_dpi,
    Canvas, Fonts, R, TA_CENTER, TA_LEFT, WM_MOUSELEAVE,
};
use super::widgets::{draw_button, BtnStyle, Ctx, WState};
use crate::app::{ProfileRow, UiAction};
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, ScreenToClient, PAINTSTRUCT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, IsIconic,
    KillTimer, LoadCursorW, RegisterClassExW, SendMessageW, SetCursor, SetForegroundWindow, SetTimer,
    SetWindowPos, ShowWindow, SystemParametersInfoW, CS_HREDRAW, CS_VREDRAW, HICON, ICON_BIG, ICON_SMALL,
    IDC_ARROW, IDC_HAND, SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE,
    SW_SHOW, WM_CAPTURECHANGED, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR, WM_SETICON,
    WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

const CLASS: &str = "SpectrumProfilesWindow";
const CLIENT_W: i32 = 780;
const MARGIN: i32 = design::PAD;
const TOP: i32 = design::PAD + design::HEADING_H + design::SPACE_M;
const PAD: i32 = 4;
const ROW_H: i32 = 62;
const VISIBLE_ROWS: i32 = 6;
const STATUS_H: i32 = 20;
const CLIENT_H: i32 = TOP + 2 * PAD + VISIBLE_ROWS * ROW_H + 12 + STATUS_H + 20;
const SCROLLBAR_W: i32 = 12;
const BTN_W: i32 = 132;
const BTN_H: i32 = design::CONTROL_H;
const TIMER_CONFIRM: usize = 1;
/// How long the button stays on "Click to confirm".
const CONFIRM_MS: u32 = 3000;

struct Profiles {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    rows: Vec<ProfileRow>,
    scroll: i32,
    /// Row under the mouse and the mouse position (client px).
    hover: Option<usize>,
    mouse: (i32, i32),
    /// Row whose button was pressed and not yet released.
    pressed: Option<usize>,
    /// Row waiting for the second click.
    confirm: Option<usize>,
    status: Option<String>,
    tracking: bool,
    drag: Option<i32>,
    icons: [HICON; 2],
    /// Showcase rows: Delete only edits the list, nothing on the system changes.
    mock: bool,
}

thread_local! {
    static PROFILES: RefCell<Option<Profiles>> = const { RefCell::new(None) };
    static HWND_CELL: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn with<T>(f: impl FnOnce(&mut Profiles) -> T) -> Option<T> {
    PROFILES.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

#[derive(Clone, Copy)]
struct Geo {
    panel: R,
    list: R,
    status: R,
    head: R,
}

fn geo(dpi: u32) -> Geo {
    let s = |v: i32| sc(v, dpi);
    let panel = R::new(
        s(MARGIN),
        s(TOP),
        s(CLIENT_W - MARGIN),
        s(TOP) + 2 * s(PAD) + VISIBLE_ROWS * s(ROW_H),
    );
    Geo {
        panel,
        list: R::new(
            panel.l + s(PAD),
            panel.t + s(PAD),
            panel.r - s(PAD),
            panel.b - s(PAD),
        ),
        status: R::new(
            s(MARGIN),
            panel.b + s(12),
            s(CLIENT_W - MARGIN),
            panel.b + s(12 + STATUS_H),
        ),
        head: R::new(
            s(MARGIN),
            s(design::PAD),
            s(CLIENT_W - MARGIN),
            s(design::PAD + design::HEADING_H),
        ),
    }
}

impl Profiles {
    fn content_h(&self) -> i32 {
        self.rows.len() as i32 * sc(ROW_H, self.dpi)
    }

    fn overflow(&self) -> bool {
        self.content_h() > geo(self.dpi).list.h()
    }

    fn clamp_scroll(&mut self) {
        let g = geo(self.dpi);
        let row = sc(ROW_H, self.dpi);
        let max = (self.content_h() - g.list.h()).max(0);
        self.scroll = ((self.scroll + row / 2) / row * row).clamp(0, max);
    }

    /// Scrollbar geometry: (track, thumb top, thumb length, max scroll).
    fn scrollbar(&self) -> Option<(R, i32, i32, i32)> {
        let g = geo(self.dpi);
        let content = self.content_h();
        if content <= g.list.h() {
            return None;
        }
        let max_scroll = content - g.list.h();
        let dpi = self.dpi;
        let tx = g.list.r - sc(7, dpi);
        let track = R::new(tx, g.list.t + sc(2, dpi), tx + sc(4, dpi), g.list.b - sc(2, dpi));
        let len = ((track.h() as f32 * g.list.h() as f32 / content as f32).max(scf(24.0, dpi))) as i32;
        let t = track.t + ((track.h() - len) as f32 * self.scroll as f32 / max_scroll as f32) as i32;
        Some((track, t, len, max_scroll))
    }

    fn scrollbar_press(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some((_, t, len, _)) = self.scrollbar() else {
            return false;
        };
        let g = geo(self.dpi);
        let zone = R::new(g.list.r - sc(SCROLLBAR_W, self.dpi), g.list.t, g.list.r, g.list.b);
        if !zone.contains(x, y) {
            return false;
        }
        if y >= t && y < t + len {
            self.drag = Some(y - t);
            // SAFETY: valid window handle; released on button up or capture loss.
            unsafe { SetCapture(hwnd) };
        } else {
            let page = g.list.h() / sc(ROW_H, self.dpi) * sc(ROW_H, self.dpi);
            self.scroll += if y < t { -page } else { page };
            self.clamp_scroll();
        }
        true
    }

    fn scrollbar_drag_to(&mut self, thumb_top: i32) {
        let Some((track, _, len, max_scroll)) = self.scrollbar() else {
            return;
        };
        let span = (track.h() - len).max(1);
        let f = ((thumb_top - track.t) as f32 / span as f32).clamp(0.0, 1.0);
        self.scroll = (f * max_scroll as f32).round() as i32;
        self.clamp_scroll();
    }

    /// Top of row `i` in client px (may be outside the viewport).
    fn row_top(&self, i: usize) -> i32 {
        geo(self.dpi).list.t - self.scroll + i as i32 * sc(ROW_H, self.dpi)
    }

    fn row_rect(&self, i: usize) -> R {
        let g = geo(self.dpi);
        let sb = if self.overflow() {
            sc(SCROLLBAR_W, self.dpi)
        } else {
            0
        };
        let t = self.row_top(i);
        R::new(g.list.l, t, g.list.r - sb, t + sc(ROW_H, self.dpi))
    }

    fn button_rect(&self, i: usize) -> R {
        let r = self.row_rect(i);
        let (w, h) = (sc(BTN_W, self.dpi), sc(BTN_H, self.dpi));
        R::xywh(r.r - sc(12, self.dpi) - w, r.t + (r.h() - h) / 2, w, h)
    }

    fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let g = geo(self.dpi);
        if !g.list.contains(x, y) {
            return None;
        }
        let i = (y - g.list.t + self.scroll) / sc(ROW_H, self.dpi);
        let i = usize::try_from(i).ok()?;
        (i < self.rows.len() && self.row_rect(i).contains(x, y)).then_some(i)
    }

    /// The row whose enabled Delete button is at the point.
    fn button_at(&self, x: i32, y: i32) -> Option<usize> {
        let i = self.row_at(x, y)?;
        (self.rows[i].deletable.is_ok() && self.button_rect(i).contains(x, y)).then_some(i)
    }

    fn invalidate(&self) {
        if !self.hwnd.is_null() {
            // SAFETY: valid window handle.
            unsafe {
                InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
        }
    }

    fn paint(&mut self, hdc: windows_sys::Win32::Graphics::Gdi::HDC) {
        let mut rc: RECT = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: valid window handle and out pointer.
        unsafe {
            GetClientRect(self.hwnd, &mut rc);
        }
        let (w, h) = (rc.right, rc.bottom);
        if w <= 0 || h <= 0 {
            return;
        }
        let mut cv = match self.canvas.take() {
            Some(c) if c.width() == w && c.height() == h => c,
            _ => match Canvas::new(w, h) {
                Some(c) => c,
                None => return,
            },
        };
        self.draw(&mut cv);
        cv.present(hdc, 0, 0);
        self.canvas = Some(cv);
    }

    fn draw(&mut self, c: &mut Canvas) {
        let th = theme();
        let dpi = self.dpi;
        let g = geo(dpi);
        c.clear(th.bg);
        c.text(g.head, "Manage profiles", self.fonts.subtitle, th.text, TA_LEFT);
        let radius = scf(design::RADIUS_CARD, dpi);
        c.fill_rrect(g.panel, radius, th.surface, 1.0);
        let overflow = self.overflow();
        if self.rows.is_empty() {
            c.text(
                g.list,
                "No profiles found",
                self.fonts.body,
                th.text_dim,
                TA_CENTER,
            );
        }
        for i in 0..self.rows.len() {
            let r = self.row_rect(i);
            if r.t >= g.list.t && r.b <= g.list.b {
                self.draw_row(c, i, r);
            }
        }
        if overflow {
            if let Some((track, t, len, _)) = self.scrollbar() {
                c.fill_rrect(track, scf(2.0, dpi), mix(th.surface, th.text_dim, 0.22), 1.0);
                let alpha = if self.drag.is_some() { 1.0 } else { 0.85 };
                c.fill_rrect(
                    R::new(track.l, t, track.r, t + len),
                    scf(2.0, dpi),
                    th.text_dim,
                    alpha,
                );
            }
        }
        c.stroke_rrect(g.panel, radius, scf(1.0, dpi).floor().max(1.0), th.line, 1.0);
        if let Some(s) = &self.status {
            c.text(g.status, s, self.fonts.caption, th.text_dim, TA_LEFT);
        }
    }

    fn draw_row(&mut self, c: &mut Canvas, i: usize, r: R) {
        let th = theme();
        let dpi = self.dpi;
        let row = self.rows[i].clone();
        if self.hover == Some(i) {
            c.fill_rrect(r.inset(sc(1, dpi)), scf(6.0, dpi), th.hover_fill(), 0.6);
        }
        let btn = self.button_rect(i);
        let left = r.l + sc(12, dpi);
        let right = btn.l - sc(12, dpi);
        let line = |k: i32| R::new(left, r.t + sc(k, dpi), right, r.t + sc(k + 18, dpi));
        // line 1: name and type tag
        let name_w = c.text_width(&row.name, self.fonts.body);
        let tag = row.kind.label();
        let tag_w = c.text_width(tag, self.fonts.caption) + sc(12, dpi);
        let name_max = (right - left - tag_w - sc(8, dpi)).max(sc(40, dpi));
        let name_w = name_w.min(name_max);
        c.text(
            R::new(left, r.t + sc(7, dpi), left + name_w, r.t + sc(27, dpi)),
            &row.name,
            self.fonts.body,
            th.text,
            TA_LEFT,
        );
        let pill = R::new(
            left + name_w + sc(8, dpi),
            r.t + sc(9, dpi),
            left + name_w + sc(8, dpi) + tag_w,
            r.t + sc(25, dpi),
        );
        c.stroke_rrect(pill, scf(8.0, dpi), scf(1.0, dpi).floor().max(1.0), th.line, 1.0);
        c.text(pill, tag, self.fonts.caption, th.text_dim, TA_CENTER);
        // line 2: file name; line 3: usage and, for a locked row, why
        c.text(line(27), &row.file, self.fonts.caption, th.text_dim, TA_LEFT);
        let third = match row.deletable {
            Err(why) => format!("{} \u{b7} {}", row.used, why.hint()),
            Ok(()) => row.used.clone(),
        };
        c.text(line(42), &third, self.fonts.caption, th.text_dim, TA_LEFT);
        // button
        let confirming = self.confirm == Some(i);
        let (label, style) = if confirming {
            ("Click to confirm", BtnStyle::DangerConfirm)
        } else {
            ("Delete", BtnStyle::Danger)
        };
        let enabled = row.deletable.is_ok();
        let over =
            self.mouse.0 >= btn.l && self.mouse.0 < btn.r && self.mouse.1 >= btn.t && self.mouse.1 < btn.b;
        let st = WState {
            hover: enabled && self.hover == Some(i) && over,
            pressed: enabled && self.pressed == Some(i),
            focus: false,
            enabled,
        };
        let ctx = Ctx {
            th,
            f: &self.fonts,
            dpi,
        };
        draw_button(c, &ctx, btn, label, st, style);
    }
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

pub fn is_open() -> bool {
    !hwnd().is_null()
}

/// Opens the window on the color folder's real profiles.
pub fn open() {
    open_rows(None);
}

/// Opens the window on made-up rows (screenshots). Delete only edits the list.
pub fn open_showcase(rows: Vec<ProfileRow>) {
    open_rows(Some(rows));
}

fn open_rows(mock: Option<Vec<ProfileRow>>) {
    let existing = hwnd();
    if !existing.is_null() {
        // SAFETY: valid window handle.
        unsafe {
            if IsIconic(existing) != 0 {
                ShowWindow(existing, SW_RESTORE);
            }
            SetForegroundWindow(existing);
        }
        return;
    }
    register_class();
    let dpi = system_dpi();
    let is_mock = mock.is_some();
    let rows = mock.unwrap_or_else(crate::app::list_profiles);
    let p = Profiles {
        hwnd: std::ptr::null_mut(),
        dpi,
        fonts: Fonts::new(dpi),
        canvas: None,
        rows,
        scroll: 0,
        hover: None,
        mouse: (-1, -1),
        pressed: None,
        confirm: None,
        status: None,
        tracking: false,
        drag: None,
        icons: [std::ptr::null_mut(); 2],
        mock: is_mock,
    };
    PROFILES.with(|m| *m.borrow_mut() = Some(p));

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: sc(CLIENT_W, dpi),
        bottom: sc(CLIENT_H, dpi),
    };
    let mut wa = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    // SAFETY: plain Win32 calls with valid pointers; PROFILES is not borrowed during window creation.
    unsafe {
        AdjustWindowRectExForDpi(&mut r, style, 0, 0, super::draw::frame_dpi(dpi));
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut _, 0);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let x = (wa.left + ((wa.right - wa.left) - w) / 2).max(wa.left);
        let y = (wa.top + ((wa.bottom - wa.top) - h) / 2).max(wa.top);
        let cls = wide(CLASS);
        let title = wide("Spectrum: manage profiles");
        let hwnd = CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            PROFILES.with(|m| *m.borrow_mut() = None);
            return;
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        let actual = window_dpi(hwnd);
        if actual != dpi {
            on_dpi_changed(hwnd, actual, None);
        }
    }
}

pub fn close() {
    let h = hwnd();
    if !h.is_null() {
        // SAFETY: valid window handle owned by this thread.
        unsafe {
            DestroyWindow(h);
        }
    }
}

/// Reloads the rows (after a profile changed elsewhere).
pub fn refresh() {
    if !is_open() || with(|p| p.mock).unwrap_or(true) {
        return;
    }
    let rows = crate::app::list_profiles();
    with(|p| {
        p.rows = rows;
        p.confirm = None;
        p.clamp_scroll();
        p.invalidate();
    });
}

fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let cls = wide(CLASS);
        // SAFETY: class data outlives the call.
        unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.lpfnWndProc = Some(wnd_proc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            wc.lpszClassName = cls.as_ptr();
            RegisterClassExW(&wc);
        }
    });
}

fn on_dpi_changed(hwnd: HWND, dpi: u32, suggested: Option<RECT>) {
    with(|p| {
        p.dpi = dpi;
        p.fonts = Fonts::new(dpi);
        p.canvas = None;
        p.clamp_scroll();
    });
    refresh_icons(hwnd, dpi);
    // SAFETY: valid window; PROFILES is not borrowed here.
    unsafe {
        let r = match suggested {
            Some(r) => r,
            None => {
                let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
                let mut r = RECT {
                    left: 0,
                    top: 0,
                    right: sc(CLIENT_W, dpi),
                    bottom: sc(CLIENT_H, dpi),
                };
                AdjustWindowRectExForDpi(&mut r, style, 0, 0, super::draw::frame_dpi(dpi));
                let mut cur: RECT = std::mem::zeroed();
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut cur);
                RECT {
                    left: cur.left,
                    top: cur.top,
                    right: cur.left + r.right - r.left,
                    bottom: cur.top + r.bottom - r.top,
                }
            }
        };
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

fn lo(lp: LPARAM) -> i32 {
    (lp & 0xFFFF) as i16 as i32
}
fn hi(lp: LPARAM) -> i32 {
    ((lp >> 16) & 0xFFFF) as i16 as i32
}

fn icon_at(dpi: u32, metric: i32) -> HICON {
    // SAFETY: plain query.
    let px = unsafe { windows_sys::Win32::UI::HiDpi::GetSystemMetricsForDpi(metric, dpi) };
    crate::icon::create_hicon(px.max(16) as u32)
}

fn refresh_icons(hwnd: HWND, dpi: u32) {
    if with(|_| ()).is_none() {
        return;
    }
    let big = icon_at(dpi, SM_CXICON);
    let small = icon_at(dpi, SM_CXSMICON);
    // SAFETY: valid window; the old icons are released only after the window stops using them.
    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as LPARAM);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as LPARAM);
        if let Some(old) = with(|x| std::mem::replace(&mut x.icons, [big, small])) {
            for i in old {
                if !i.is_null() {
                    DestroyIcon(i);
                }
            }
        }
    }
}

/// Second click: deletes through the app (same path as `--debug-delete-profile`), then reloads the rows.
/// Runs with PROFILES unborrowed: the Win32 calls below can send messages back to this window.
fn delete_now(hwnd: HWND, file: String) {
    // SAFETY: valid window; killing the timer twice is harmless.
    unsafe { KillTimer(hwnd, TIMER_CONFIRM) };
    let mock = with(|p| p.mock).unwrap_or(false);
    let result = if mock {
        let name = with(|p| p.rows.iter().find(|r| r.file == file).map(|r| r.name.clone()))
            .flatten()
            .unwrap_or_else(|| file.clone());
        Ok(format!("Deleted {name}"))
    } else {
        crate::app::dispatch_result(UiAction::DeleteProfile { file: file.clone() })
    };
    let rows = if mock {
        None
    } else {
        Some(crate::app::list_profiles())
    };
    with(|p| {
        p.confirm = None;
        p.status = Some(result.unwrap_or_else(|e| e));
        match rows {
            Some(r) => p.rows = r,
            None => p.rows.retain(|r| r.file != file),
        }
        p.clamp_scroll();
        p.invalidate();
    });
    if !mock && super::main_window::is_open() {
        super::main_window::refresh();
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            HWND_CELL.with(|c| c.set(hwnd as isize));
            apply_window_theme(hwnd, theme().dark);
            let dpi = window_dpi(hwnd);
            let big = icon_at(dpi, SM_CXICON);
            let small = icon_at(dpi, SM_CXSMICON);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as LPARAM);
            with(|p| {
                p.hwnd = hwnd;
                p.icons = [big, small];
            });
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with(|p| p.paint(hdc));
            EndPaint(hwnd, &ps);
            0
        }
        WM_DPICHANGED => {
            let rc = *(lp as *const RECT);
            on_dpi_changed(hwnd, (wp & 0xFFFF) as u32, Some(rc));
            0
        }
        WM_SETTINGCHANGE => {
            if wp == 0 && lp != 0 {
                let p = lp as *const u16;
                let mut n = 0;
                while n < 64 && *p.add(n) != 0 {
                    n += 1;
                }
                if String::from_utf16_lossy(std::slice::from_raw_parts(p, n)) == "ImmersiveColorSet" {
                    invalidate_theme();
                    apply_window_theme(hwnd, theme().dark);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = (lo(lp), hi(lp));
            with(|p| {
                if let Some(off) = p.drag {
                    p.scrollbar_drag_to(y - off);
                    p.invalidate();
                    return;
                }
                if !p.tracking {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut t);
                    p.tracking = true;
                }
                p.mouse = (x, y);
                p.hover = p.row_at(x, y);
                p.invalidate();
            });
            0
        }
        WM_MOUSELEAVE => {
            with(|p| {
                p.tracking = false;
                p.hover = None;
                p.mouse = (-1, -1);
                p.invalidate();
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (lo(lp), hi(lp));
            SetFocus(hwnd);
            with(|p| {
                if p.scrollbar_press(hwnd, x, y) {
                    p.invalidate();
                } else if let Some(i) = p.button_at(x, y) {
                    p.pressed = Some(i);
                    SetCapture(hwnd);
                    p.invalidate();
                }
            });
            0
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            let (x, y) = (lo(lp), hi(lp));
            // Decide inside the borrow, act after it: the delete calls into Win32.
            let delete = with(|p| {
                let was_drag = p.drag.take().is_some();
                let pressed = p.pressed.take();
                p.invalidate();
                if was_drag || msg == WM_CAPTURECHANGED {
                    return None;
                }
                let i = pressed?;
                if p.button_at(x, y) != Some(i) {
                    return None;
                }
                if p.confirm == Some(i) {
                    return Some(p.rows[i].file.clone());
                }
                p.confirm = Some(i);
                SetTimer(hwnd, TIMER_CONFIRM, CONFIRM_MS, None);
                None
            })
            .flatten();
            if msg == WM_LBUTTONUP {
                ReleaseCapture();
            }
            if let Some(file) = delete {
                delete_now(hwnd, file);
            }
            0
        }
        WM_TIMER => {
            if wp == TIMER_CONFIRM {
                KillTimer(hwnd, TIMER_CONFIRM);
                with(|p| {
                    p.confirm = None;
                    p.invalidate();
                });
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT { x: lo(lp), y: hi(lp) };
            ScreenToClient(hwnd, &mut pt);
            with(|p| {
                let steps = (delta.abs() / 120).max(1) * delta.signum();
                p.scroll -= steps * sc(ROW_H, p.dpi);
                p.clamp_scroll();
                p.hover = p.row_at(pt.x, pt.y);
                p.invalidate();
            });
            0
        }
        WM_SETCURSOR => {
            if (lp & 0xFFFF) as u32 == 1 {
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                ScreenToClient(hwnd, &mut pt);
                let hand = with(|p| p.button_at(pt.x, pt.y).is_some()).unwrap_or(false);
                SetCursor(LoadCursorW(
                    std::ptr::null_mut(),
                    if hand { IDC_HAND } else { IDC_ARROW },
                ));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_KEYDOWN => {
            if wp as u16 == VK_ESCAPE {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_CONFIRM);
            HWND_CELL.with(|c| c.set(0));
            let p = PROFILES.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.take()));
            if let Some(p) = &p {
                for i in p.icons {
                    if !i.is_null() {
                        DestroyIcon(i);
                    }
                }
            }
            drop(p); // fonts and canvas
            trim_working_set();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_is_whole_rows() {
        for dpi in [96, 120, 144, 192] {
            let g = geo(dpi);
            assert_eq!(g.list.h() % sc(ROW_H, dpi), 0, "dpi {dpi}");
        }
    }
}
