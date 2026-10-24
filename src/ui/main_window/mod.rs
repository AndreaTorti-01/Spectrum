//! The main window: monitor map, SDR profile picker, HDR sliders. Destroyed on close to free memory.
//!
//! - `layout`: the vertical flow of rows (`Geo`) and the widget list built on it.
//! - `edit`: slider values, clicks and keys turned into pending actions.
//! - `paint`: drawing the client area (also off-screen, see [`Offscreen`]).
//! - `proc`: the window procedure and the actions it flushes after each message.

mod edit;
mod layout;
mod paint;
mod proc;

use super::draw::{sc, system_dpi, theme, Canvas, Fonts, Theme, R};
use super::widgets::{self, peak_scale, WidgetList};
use super::window;
use crate::app::{UiAction, UiMonitor, UiState};
use layout::{geo, Geo, Shape};
use std::cell::{Cell, RefCell};
use std::time::Instant;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, SendMessageW, HICON, WM_CLOSE};

const CLASS: &str = "SpectrumMainWindow";
const CLIENT_W: i32 = 440;

/// Everything the user can point at or tab to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Id {
    Tile(usize),
    Identify,
    Sdr,
    Read,
    Compare,
    Manage,
    HdrToggle,
    Advanced,
    Black,
    Peak,
    Full,
    Sat,
    SdrWhite,
    SdrCap,
    Reset,
    Autostart,
}

/// The slider rows, in `Geo::rows` order: the four Advanced ones, then SDR content.
const SLIDERS: [Id; 5] = [Id::Black, Id::Peak, Id::Full, Id::Sat, Id::SdrWhite];

/// Work queued while the window state is borrowed, run by `proc::flush_pending` after the message.
enum Pending {
    Dispatch(UiAction),
    /// The Advanced row changed the client height.
    Resize,
    OpenDropdown,
    Close,
}

struct Main {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    ui: WidgetList<Id>,
    state: UiState,
    /// Index of the monitor shown (in `state.monitors`).
    selected: usize,
    /// Raw (unrounded) HDR edit values: nits / nits / nits / percent.
    black: f64,
    peak: f64,
    full: f64,
    sat: f64,
    /// Windows "SDR content brightness" in nits. Not part of the MHC2 profile; applied through DisplayConfig.
    sdr_white: f64,
    sdr_white_dirty: bool,
    /// The slider being dragged.
    drag: Option<Id>,
    /// An HDR profile slider changed since the last apply.
    hdr_dirty: bool,
    /// `TIMER_APPLY` is running; the next tick commits whatever changed since the last one.
    apply_armed: bool,
    pending: Vec<Pending>,
    /// `WM_MOUSELEAVE` was requested.
    tracking: bool,
    /// When the dropdown popup last closed: a click on the field right after only dismisses it.
    dropdown_closed_at: Option<Instant>,
    icons: [HICON; 2],
    geo: Geo,
}

thread_local! {
    static MAIN: RefCell<Option<Main>> = const { RefCell::new(None) };
    static HWND_CELL: Cell<isize> = const { Cell::new(0) };
}

/// Runs `f` on the window state unless the window is closed or the state is already borrowed.
fn with<T>(f: impl FnOnce(&mut Main) -> T) -> Option<T> {
    MAIN.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

impl Main {
    /// Window state for `state`, laid out at `dpi`. No window yet (`hwnd` is null until `WM_CREATE`).
    fn new(state: UiState, dpi: u32) -> Main {
        let mut main = Main {
            hwnd: std::ptr::null_mut(),
            dpi,
            fonts: Fonts::new(dpi),
            canvas: None,
            ui: WidgetList::new(),
            state,
            selected: 0,
            black: 0.0,
            peak: 1000.0,
            full: 400.0,
            sat: 0.0,
            sdr_white: 240.0,
            sdr_white_dirty: false,
            drag: None,
            hdr_dirty: false,
            apply_armed: false,
            pending: Vec::new(),
            tracking: false,
            dropdown_closed_at: None,
            icons: [std::ptr::null_mut(); 2],
            geo: geo(dpi, Shape::default()),
        };
        main.sync_hdr();
        main.layout();
        main
    }

    /// The monitor shown.
    fn mon(&self) -> Option<&UiMonitor> {
        self.state.monitors.get(self.selected)
    }

    /// The app theme with the shown monitor's accent.
    fn theme(&self) -> Theme {
        let t = theme();
        match self.mon() {
            Some(m) => t.with_accent(m.accent),
            None => t,
        }
    }

    fn shape(&self) -> Shape {
        Shape {
            advanced: self.state.advanced,
            restore: self.hdr_enabled(),
            error: self.state.error.is_some(),
        }
    }

    /// The shown monitor supports HDR and has it on: the HDR controls are live.
    fn hdr_enabled(&self) -> bool {
        self.mon().map(|m| m.hdr_supported && m.hdr_on).unwrap_or(false)
    }

    /// Keeps the selection valid after the monitor list changed.
    fn fix_selected(&mut self) {
        if self.selected >= self.state.monitors.len() {
            self.selected = 0;
        }
    }

    fn invalidate(&self) {
        window::invalidate(self.hwnd);
    }

    /// Client size of the window in physical pixels: the layout height at 96 dpi, scaled.
    fn window_client_size(&self) -> (i32, i32) {
        (
            sc(CLIENT_W, self.dpi),
            sc(layout::client_h(self.shape()), self.dpi),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

pub fn open() {
    if is_open() {
        window::restore_and_focus(hwnd());
        return;
    }
    proc::register_class();
    let dpi = system_dpi();
    let main = Main::new(super::snapshot(), dpi);
    let (client_w, client_h) = main.window_client_size();
    MAIN.with(|m| *m.borrow_mut() = Some(main));
    // MAIN is not borrowed while the window is created: WM_CREATE runs inside.
    let hwnd = window::create_centered(CLASS, "Spectrum", client_w, client_h, dpi);
    if hwnd.is_null() {
        MAIN.with(|m| *m.borrow_mut() = None);
        return;
    }
    window::show_new(hwnd);
    let actual = super::draw::window_dpi(hwnd);
    if actual != dpi {
        proc::on_dpi_changed(hwnd, actual, None);
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

/// Closes through `WM_CLOSE`, so pending slider edits are committed first.
pub fn request_close() {
    let h = hwnd();
    if !h.is_null() {
        // SAFETY: valid window handle owned by this thread; handled synchronously.
        unsafe {
            SendMessageW(h, WM_CLOSE, 0, 0);
        }
    }
}

pub fn refresh() {
    // HDR turning on or off adds or removes the restore button, so the window may need a new height.
    let resize = with(|main| {
        let before = main.geo.h;
        main.state = super::snapshot();
        main.fix_selected();
        main.sync_hdr();
        main.layout();
        main.invalidate();
        (main.geo.h != before).then_some((main.hwnd, main.dpi))
    })
    .flatten();
    if let Some((h, dpi)) = resize {
        proc::on_dpi_changed(h, dpi, None);
    }
}

pub fn is_open() -> bool {
    !hwnd().is_null()
}

/// Index of the monitor currently selected in the open window (0 when closed).
pub fn selected() -> usize {
    with(|main| main.selected).unwrap_or(0)
}

pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

/// The main window's state and drawing without a window, for off-screen screenshots (`--render`).
pub struct Offscreen(Main);

/// Places the screenshots and the site point at, in client pixels.
pub struct Marks {
    /// Tile of the first monitor in the map (its big number).
    pub monitor: R,
    pub sdr: R,
    /// A point (empty rect) below Read from monitor.
    pub read: R,
    /// Client rows of the HDR section (the `hdr` screenshot): from the divider above it to halfway between
    /// the restore button and the divider below it.
    pub hdr_rows: (i32, i32),
}

impl Offscreen {
    pub fn new(state: UiState, dpi: u32) -> Offscreen {
        Offscreen(Main::new(state, dpi))
    }

    /// Client size in physical pixels.
    pub fn size(&self) -> (i32, i32) {
        (sc(CLIENT_W, self.0.dpi), self.0.geo.h)
    }

    /// Shows `nits` on the Peak slider without applying anything, as the demo does.
    pub fn set_peak(&mut self, nits: f64) {
        let main = &mut self.0;
        main.peak = nits.clamp(widgets::PEAK_MIN_NITS, widgets::PEAK_MAX_NITS);
        main.full = main.full.min(main.peak);
    }

    /// Client position of the Peak slider's thumb when it shows `nits`.
    pub fn peak_thumb(&self, nits: f64) -> (f32, f32) {
        let r = self.0.geo.rows[1].1;
        let (a, b) = widgets::slider_span(r, self.0.dpi);
        (a + (b - a) * peak_scale().to_t(nits) as f32, r.cy())
    }

    pub fn marks(&self) -> Marks {
        let geo = &self.0.geo;
        Marks {
            monitor: self.0.ui.get(Id::Tile(0)).map(|w| w.rect).unwrap_or(geo.map),
            sdr: geo.sdr,
            // Under the button's middle, in the gap above the divider, so a leader line crosses no control.
            read: {
                let (x, y) = ((geo.read.l + geo.read.r) / 2, (geo.read.b + geo.div1) / 2);
                R::new(x, y, x, y)
            },
            hdr_rows: (geo.div1, (geo.reset.b + geo.div2) / 2),
        }
    }

    pub fn render(&self) -> Option<Canvas> {
        let (w, h) = self.size();
        let mut canvas = Canvas::new(w, h)?;
        self.0.draw(&mut canvas);
        Some(canvas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn without_hdr() -> UiState {
        let mut st = crate::showcase::state();
        for m in &mut st.monitors {
            m.hdr_supported = false;
            m.hdr_on = false;
            m.sdr_white_nits = None;
        }
        st
    }

    pub(super) fn enabled(main: &Main, id: Id) -> Option<bool> {
        main.ui.get(id).map(|w| w.enabled)
    }

    #[test]
    fn non_hdr_monitor_greys_out_the_hdr_controls() {
        let mut st = without_hdr();
        st.advanced = true;
        let main = Main::new(st, 96);
        assert_eq!(enabled(&main, Id::HdrToggle), Some(false));
        for id in SLIDERS {
            assert_eq!(enabled(&main, id), Some(false), "{id:?}");
        }
        assert!(main.ui.get(Id::Reset).is_none());
        assert!(!main.hdr_enabled());
        // The SDR side stays usable.
        assert_eq!(enabled(&main, Id::Compare), Some(true));
        assert!(Offscreen(main).render().is_some());
    }

    #[test]
    fn hdr_monitor_enables_the_toggle() {
        let main = Main::new(crate::showcase::state(), 96);
        assert_eq!(enabled(&main, Id::HdrToggle), Some(true));
    }

    #[test]
    fn offscreen_starts_from_the_snapshot_values() {
        let main = Main::new(crate::showcase::state(), 96);
        assert_eq!(
            (main.black, main.peak, main.full, main.sat, main.sdr_white),
            (0.0, 450.0, 250.0, 0.0, 240.0)
        );
        assert!(main.hwnd.is_null() && main.pending.is_empty());
    }

    #[test]
    fn offscreen_peak_moves_the_thumb_and_pulls_full_frame_down() {
        let mut off = Offscreen::new(crate::showcase::state(), 96);
        let (low, y) = off.peak_thumb(400.0);
        let (high, y2) = off.peak_thumb(1000.0);
        assert!(high > low && y == y2);
        off.set_peak(200.0);
        assert_eq!((off.0.peak, off.0.full), (200.0, 200.0));
        off.set_peak(99999.0);
        assert_eq!(off.0.peak, widgets::PEAK_MAX_NITS);
    }

    #[test]
    fn marks_sit_on_the_controls() {
        let off = Offscreen::new(crate::showcase::state(), 96);
        let m = off.marks();
        let geo = &off.0.geo;
        assert!(geo.map.contains(m.monitor.l, m.monitor.t));
        assert_eq!(m.sdr, geo.sdr);
        assert!(m.read.t > geo.read.b && m.read.t < geo.div1);
        assert!(m.hdr_rows.0 < m.hdr_rows.1 && m.hdr_rows.1 < geo.div2);
    }

    #[test]
    fn no_monitors_still_draws() {
        let mut st = crate::showcase::state();
        st.monitors.clear();
        let main = Main::new(st, 96);
        assert_eq!(enabled(&main, Id::Identify), Some(false));
        assert_eq!(enabled(&main, Id::Sdr), Some(false));
        assert!(main.ui.get(Id::Tile(0)).is_none());
        assert!(Offscreen(main).render().is_some());
    }
}
