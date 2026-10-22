//! Small first-run dialog offering "Install" or "Just run", with a "Start with Windows" checkbox.

use super::draw::{
    apply_window_theme, invalidate_theme, sc, system_dpi, theme, window_dpi, Canvas, Fonts, R, TA_LEFT,
    WM_MOUSELEAVE,
};
use super::widgets::{draw_button, draw_checkbox, BtnStyle, Ctx, Kind, WidgetList};
use crate::util::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE,
    TRACKMOUSEEVENT, VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, GetClientRect,
    GetMessageW, LoadCursorW, PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow,
    SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HICON,
    ICON_BIG, ICON_SMALL, IDC_ARROW, MSG, SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOW, WM_CLOSE, WM_CREATE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_QUIT, WM_SETICON, WM_SETTINGCHANGE, WNDCLASSEXW, WS_CAPTION,
    WS_OVERLAPPED, WS_SYSMENU,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallChoice {
    Install { autostart: bool },
    JustRun,
    Cancel,
}

const CLASS: &str = "SpectrumInstallWindow";
const CLIENT_W: i32 = 380;
const CLIENT_H: i32 = 220;
const ICON_PX: i32 = 48;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Id {
    Autostart,
    JustRun,
    Install,
}

struct State {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    ui: WidgetList<Id>,
    autostart: bool,
    icon_px: i32,
    icon_bgra: Vec<u8>,
    tracking: bool,
    result: Option<InstallChoice>,
    icons: [HICON; 2],
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

impl State {
    fn layout(&mut self) {
        let s = |v: i32| sc(v, self.dpi);
        self.ui.clear();
        self.ui.add(
            Id::Autostart,
            Kind::Checkbox,
            R::new(s(24), s(134), s(CLIENT_W - 24), s(160)),
            true,
            true,
        );
        self.ui.add(
            Id::JustRun,
            Kind::Button,
            R::new(
                s(CLIENT_W - 24 - 96 - 10 - 94),
                s(172),
                s(CLIENT_W - 24 - 96 - 10),
                s(204),
            ),
            true,
            true,
        );
        self.ui.add(
            Id::Install,
            Kind::Button,
            R::new(s(CLIENT_W - 24 - 96), s(172), s(CLIENT_W - 24), s(204)),
            true,
            true,
        );
        self.icon_px = s(ICON_PX);
        self.icon_bgra = crate::icon::render_bgra(self.icon_px as u32);
    }

    fn invalidate(&self) {
        if !self.hwnd.is_null() {
            // SAFETY: valid window handle.
            unsafe {
                InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
        }
    }

    fn activate(&mut self, id: Id) {
        match id {
            Id::Autostart => self.autostart = !self.autostart,
            Id::JustRun => self.result = Some(InstallChoice::JustRun),
            Id::Install => {
                self.result = Some(InstallChoice::Install {
                    autostart: self.autostart,
                })
            }
        }
        self.invalidate();
    }

    fn paint(&mut self, hdc: windows_sys::Win32::Graphics::Gdi::HDC) {
        let mut rc = RECT {
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
        let mut c = match self.canvas.take() {
            Some(c) if c.width() == w && c.height() == h => c,
            _ => match Canvas::new(w, h) {
                Some(c) => c,
                None => return,
            },
        };
        let th = theme();
        let dpi = self.dpi;
        let s = |v: i32| sc(v, dpi);
        let ctx = Ctx {
            th,
            f: &self.fonts,
            dpi,
        };
        c.clear(th.bg);
        c.blit_premul_bgra(s(24), s(22), self.icon_px, self.icon_px, &self.icon_bgra);
        c.text(
            R::new(s(88), s(22), s(CLIENT_W - 24), s(22 + ICON_PX)),
            "Install Spectrum",
            self.fonts.heading,
            th.text,
            TA_LEFT,
        );
        c.text_wrapped(
            R::new(s(24), s(86), s(CLIENT_W - 24), s(126)),
            "Installs for your user only \u{2014} no admin needed. Starts quietly in the tray.",
            self.fonts.body,
            th.text_dim,
        );
        let ui = &self.ui;
        draw_checkbox(
            &mut c,
            &ctx,
            ui.get(Id::Autostart).map(|w| w.rect).unwrap_or_default(),
            "Start with Windows",
            self.autostart,
            ui.state(Id::Autostart),
        );
        draw_button(
            &mut c,
            &ctx,
            ui.get(Id::JustRun).map(|w| w.rect).unwrap_or_default(),
            "Just run",
            ui.state(Id::JustRun),
            BtnStyle::Normal,
        );
        draw_button(
            &mut c,
            &ctx,
            ui.get(Id::Install).map(|w| w.rect).unwrap_or_default(),
            "Install",
            ui.state(Id::Install),
            BtnStyle::Primary,
        );
        c.present(hdc, 0, 0);
        self.canvas = Some(c);
    }
}

/// Modal; runs its own message loop.
pub fn run() -> InstallChoice {
    register_class();
    let dpi = system_dpi();
    let mut st = State {
        hwnd: std::ptr::null_mut(),
        dpi,
        fonts: Fonts::new(dpi),
        canvas: None,
        ui: WidgetList::new(),
        autostart: true,
        icon_px: 0,
        icon_bgra: Vec::new(),
        tracking: false,
        result: None,
        icons: [std::ptr::null_mut(); 2],
    };
    st.layout();
    st.ui.focus = Some(Id::Install);
    STATE.with(|s| *s.borrow_mut() = Some(st));

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
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
    // SAFETY: plain Win32 calls; STATE is not borrowed while the window is created or pumped.
    let result = unsafe {
        AdjustWindowRectExForDpi(&mut r, style, 0, 0, dpi);
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut _, 0);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let x = (wa.left + ((wa.right - wa.left) - w) / 2).max(wa.left);
        let y = (wa.top + ((wa.bottom - wa.top) - h) / 2).max(wa.top);
        let cls = wide(CLASS);
        let title = wide("Spectrum");
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
            STATE.with(|s| *s.borrow_mut() = None);
            return InstallChoice::Cancel;
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        let mut quit = false;
        loop {
            if with(|s| s.result.is_some()).unwrap_or(true) {
                break;
            }
            let got = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if got <= 0 {
                quit = got == 0 && msg.message == WM_QUIT;
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let res = with(|s| s.result).flatten().unwrap_or(InstallChoice::Cancel);
        DestroyWindow(hwnd);
        if quit {
            PostQuitMessage(0);
        }
        res
    };
    result
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

fn lo(lp: LPARAM) -> i32 {
    (lp & 0xFFFF) as i16 as i32
}
fn hi(lp: LPARAM) -> i32 {
    ((lp >> 16) & 0xFFFF) as i16 as i32
}

fn icon_for(hwnd: HWND, metric: i32) -> HICON {
    let dpi = window_dpi(hwnd);
    // SAFETY: plain query.
    let px = unsafe { windows_sys::Win32::UI::HiDpi::GetSystemMetricsForDpi(metric, dpi) };
    crate::icon::create_hicon(px.max(16) as u32)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            apply_window_theme(hwnd, theme().dark);
            let big = icon_for(hwnd, SM_CXICON);
            let small = icon_for(hwnd, SM_CXSMICON);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as LPARAM);
            with(|s| {
                s.hwnd = hwnd;
                s.icons = [big, small];
            });
            0
        }
        WM_ERASEBKGND => 1,
        WM_DPICHANGED => {
            let dpi = (wp & 0xFFFF) as u32;
            let rc = *(lp as *const RECT);
            with(|s| {
                s.dpi = dpi;
                s.fonts = Fonts::new(dpi);
                s.canvas = None;
                s.layout();
            });
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                rc.left,
                rc.top,
                rc.right - rc.left,
                rc.bottom - rc.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            with(|s| s.paint(hdc));
            EndPaint(hwnd, &ps);
            0
        }
        WM_SETTINGCHANGE => {
            if lp != 0 {
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
            with(|s| {
                if !s.tracking {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut t);
                    s.tracking = true;
                }
                let h = s.ui.hit(x, y);
                if h != s.ui.hover {
                    s.ui.hover = h;
                    s.invalidate();
                }
            });
            0
        }
        WM_MOUSELEAVE => {
            with(|s| {
                s.tracking = false;
                s.ui.hover = None;
                s.invalidate();
            });
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (lo(lp), hi(lp));
            with(|s| {
                s.ui.focus_visible = false;
                if let Some(id) = s.ui.hit(x, y) {
                    s.ui.pressed = Some(id);
                    s.ui.focus = Some(id);
                    SetCapture(hwnd);
                }
                s.invalidate();
            });
            0
        }
        WM_LBUTTONUP => {
            let (x, y) = (lo(lp), hi(lp));
            if GetCapture() == hwnd {
                ReleaseCapture();
            }
            with(|s| {
                if let Some(p) = s.ui.pressed.take() {
                    if s.ui.hit(x, y) == Some(p) {
                        s.activate(p);
                    }
                }
                s.invalidate();
            });
            0
        }
        WM_KEYDOWN => {
            let vk = wp as u16;
            with(|s| match vk {
                VK_ESCAPE => s.result = Some(InstallChoice::Cancel),
                VK_TAB => {
                    s.ui.focus_next(GetKeyState(VK_SHIFT as i32) < 0);
                    s.invalidate();
                }
                VK_SPACE => {
                    if let Some(f) = s.ui.focus {
                        s.activate(f);
                    }
                }
                VK_RETURN => {
                    let f = s.ui.focus.unwrap_or(Id::Install);
                    s.activate(if f == Id::Autostart { Id::Install } else { f });
                }
                _ => {}
            });
            0
        }
        WM_CLOSE => {
            with(|s| s.result = Some(InstallChoice::Cancel));
            0
        }
        windows_sys::Win32::UI::WindowsAndMessaging::WM_DESTROY => {
            let st = STATE.with(|s| s.borrow_mut().take());
            if let Some(st) = &st {
                for i in st.icons {
                    if !i.is_null() {
                        DestroyIcon(i);
                    }
                }
            }
            drop(st);
            super::draw::trim_working_set();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
