//! The profiles window: every .icc/.icm in the color folder with its type, the monitors using it and a
//! two-step Delete button. Destroyed on close, like the gamut window.
//!
//! - `paint`: the list and its rows.
//! - `proc`: the window procedure and the delete flow.

mod paint;
mod proc;

use super::design;
use super::draw::{sc, system_dpi, Canvas, Fonts, R};
use super::widgets::ScrollList;
use super::window;
use crate::app::ProfileRow;
use std::cell::{Cell, RefCell};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, HICON};

const CLASS: &str = "SpectrumProfilesWindow";
const CLIENT_W: i32 = 780;
const MARGIN: i32 = design::PAD;
const TOP: i32 = design::PAD + design::HEADING_H + design::SPACE_M;
/// Inner padding of the list container.
const PAD: i32 = 4;
const ROW_H: i32 = 62;
const VISIBLE_ROWS: i32 = 6;
const STATUS_H: i32 = 20;
/// Space between the list container and the status line.
const STATUS_GAP: i32 = 12;
const CLIENT_H: i32 = TOP + 2 * PAD + VISIBLE_ROWS * ROW_H + STATUS_GAP + STATUS_H + 20;
const BTN_W: i32 = 132;
const BTN_H: i32 = design::CONTROL_H;

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
    /// The last delete's result.
    status: Option<String>,
    /// `WM_MOUSELEAVE` was requested.
    tracking: bool,
    /// Scrollbar thumb drag: offset of the mouse from the thumb top.
    drag: Option<i32>,
    icons: [HICON; 2],
    /// Showcase rows: Delete only edits the list, nothing on the system changes.
    mock: bool,
}

thread_local! {
    static PROFILES: RefCell<Option<Profiles>> = const { RefCell::new(None) };
    static HWND_CELL: Cell<isize> = const { Cell::new(0) };
}

/// Runs `f` on the window state unless the window is closed or the state is already borrowed.
fn with<T>(f: impl FnOnce(&mut Profiles) -> T) -> Option<T> {
    PROFILES.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

#[derive(Clone, Copy, Debug)]
struct Geo {
    /// The list container.
    panel: R,
    /// The rows' viewport inside the container's padding.
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
            panel.b + s(STATUS_GAP),
            s(CLIENT_W - MARGIN),
            panel.b + s(STATUS_GAP + STATUS_H),
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
    /// Window state at `dpi`. No window yet.
    fn new(rows: Vec<ProfileRow>, mock: bool, dpi: u32) -> Profiles {
        Profiles {
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
            mock,
        }
    }

    fn row_h(&self) -> i32 {
        sc(ROW_H, self.dpi)
    }

    /// The list's scrolling geometry.
    fn scroll_list(&self) -> ScrollList {
        ScrollList {
            viewport: geo(self.dpi).list,
            content_h: self.rows.len() as i32 * self.row_h(),
            row_h: self.row_h(),
            dpi: self.dpi,
        }
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll_list().clamp(self.scroll);
    }

    /// Row `i` in client px (may be outside the viewport); the scrollbar strip is left out.
    fn row_rect(&self, i: usize) -> R {
        let list = self.scroll_list();
        let top = list.viewport.t - self.scroll + i as i32 * self.row_h();
        R::new(
            list.viewport.l,
            top,
            list.viewport.r - list.bar_width(),
            top + self.row_h(),
        )
    }

    /// The Delete button of row `i`, at the row's right end.
    fn button_rect(&self, i: usize) -> R {
        let r = self.row_rect(i);
        let (w, h) = (sc(BTN_W, self.dpi), sc(BTN_H, self.dpi));
        R::xywh(r.r - sc(12, self.dpi) - w, r.t + (r.h() - h) / 2, w, h)
    }

    fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let list = geo(self.dpi).list;
        if !list.contains(x, y) {
            return None;
        }
        let i = usize::try_from((y - list.t + self.scroll) / self.row_h()).ok()?;
        (i < self.rows.len() && self.row_rect(i).contains(x, y)).then_some(i)
    }

    /// The row whose enabled Delete button is at the point.
    fn button_at(&self, x: i32, y: i32) -> Option<usize> {
        let i = self.row_at(x, y)?;
        (self.rows[i].deletable.is_ok() && self.button_rect(i).contains(x, y)).then_some(i)
    }

    fn invalidate(&self) {
        window::invalidate(self.hwnd);
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
        window::restore_and_focus(existing);
        return;
    }
    proc::register_class();
    let dpi = system_dpi();
    let is_mock = mock.is_some();
    let rows = mock.unwrap_or_else(crate::app::list_profiles);
    PROFILES.with(|m| *m.borrow_mut() = Some(Profiles::new(rows, is_mock, dpi)));
    // PROFILES is not borrowed while the window is created: WM_CREATE runs inside.
    let (w, h) = (sc(CLIENT_W, dpi), sc(CLIENT_H, dpi));
    let hwnd = window::create_centered(CLASS, "Spectrum: manage profiles", w, h, dpi);
    if hwnd.is_null() {
        PROFILES.with(|m| *m.borrow_mut() = None);
        return;
    }
    window::show_new(hwnd);
    // SAFETY: valid window handle.
    unsafe { SetFocus(hwnd) };
    let actual = super::draw::window_dpi(hwnd);
    if actual != dpi {
        proc::on_dpi_changed(hwnd, actual, None);
    }
}

/// Off-screen render of `rows` at `dpi` (`--render profiles`), without a window.
pub fn render_showcase(rows: Vec<ProfileRow>, dpi: u32) -> Option<Canvas> {
    let mut profiles = Profiles::new(rows, true, dpi);
    let mut c = Canvas::new(sc(CLIENT_W, dpi), sc(CLIENT_H, dpi))?;
    profiles.draw(&mut c);
    Some(c)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn many_rows(n: usize) -> Vec<ProfileRow> {
        let base = crate::showcase::profiles();
        (0..n).map(|i| base[i % base.len()].clone()).collect()
    }

    #[test]
    fn viewport_is_whole_rows() {
        for dpi in [96, 120, 144, 192] {
            let g = geo(dpi);
            assert_eq!(g.list.h() % sc(ROW_H, dpi), 0, "dpi {dpi}");
            assert!(g.status.t > g.panel.b && g.status.b <= sc(CLIENT_H, dpi));
            assert!(g.head.b <= g.panel.t);
        }
    }

    #[test]
    fn buttons_sit_inside_their_rows() {
        let p = Profiles::new(crate::showcase::profiles(), true, 96);
        for i in 0..p.rows.len() {
            let (row, button) = (p.row_rect(i), p.button_rect(i));
            assert!(row.contains(button.l, button.t) && row.contains(button.r - 1, button.b - 1));
        }
        let b = p.button_rect(0);
        assert_eq!(p.button_at(b.l + 1, b.t + 1), Some(0));
        assert_eq!(p.row_at(b.l - 50, b.t + 1), Some(0));
        assert_eq!(p.button_at(b.l - 50, b.t + 1), None, "not on the button");
        // The inbox sRGB row can't be deleted, so its button does not count.
        let locked = p.button_rect(3);
        assert_eq!(p.button_at(locked.l + 1, locked.t + 1), None);
    }

    #[test]
    fn a_long_list_scrolls_and_narrows_the_rows() {
        let short = Profiles::new(many_rows(3), true, 96);
        let mut long = Profiles::new(many_rows(20), true, 96);
        assert!(!short.scroll_list().overflows());
        assert!(long.scroll_list().overflows());
        assert_eq!(short.row_rect(0).w() - long.row_rect(0).w(), 12);
        long.scroll = 10_000;
        long.clamp_scroll();
        assert_eq!(long.scroll, (20 - VISIBLE_ROWS) * ROW_H);
        let list = geo(96).list;
        assert_eq!(
            long.row_at(list.l + 5, list.t + 1),
            Some(20 - VISIBLE_ROWS as usize)
        );
    }

    #[test]
    fn showcase_renders_at_the_screenshot_size() {
        let c = render_showcase(crate::showcase::profiles(), 96).unwrap();
        assert_eq!((c.width(), c.height()), (CLIENT_W, CLIENT_H));
        let empty = render_showcase(Vec::new(), 144).unwrap();
        assert_eq!(empty.width(), sc(CLIENT_W, 144));
    }
}
