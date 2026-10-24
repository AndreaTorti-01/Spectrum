//! CIE 1931 xy gamut comparison window with a checklist of reference spaces and installed profiles.
//!
//! - `entries`: the checklist rows (reference spaces, installed or showcase profiles) and their texts.
//! - `layout`: geometry and the scrolling list.
//! - `paint`: the plot, the list and the info block.
//! - `proc`: the window procedure.

mod entries;
mod layout;
mod paint;
mod proc;

use super::draw::{sc, system_dpi, Canvas, Dib, Fonts, R};
use super::window;
use crate::color::Primaries;
use entries::{build_entries, Entry, Showcase, SHOWCASE};
use layout::{geo, INFO_H_INITIAL};
use std::cell::{Cell, RefCell};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, HICON};

const CLASS: &str = "SpectrumGamutWindow";
const CLIENT_W: i32 = 890;
const CLIENT_H: i32 = 610;
/// Plot domain.
const X_MIN: f64 = -0.05;
const X_MAX: f64 = 0.8;
const Y_MAX: f64 = 0.9;

/// A list row: a section header or an entry (index into `Gamut::entries`).
#[derive(Clone, Copy)]
enum Row {
    Header(&'static str),
    Entry(usize),
}

struct Gamut {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    canvas: Option<Canvas>,
    /// The locus fill at the plot size (rebuilt when the size changes).
    horseshoe: Option<Dib>,
    entries: Vec<Entry>,
    rows: Vec<Row>,
    scroll: i32,
    /// Keyboard cursor (arrow keys); drawn only while `keyboard` is set.
    selected: Option<usize>,
    keyboard: bool,
    hover: Option<usize>,
    /// `WM_MOUSELEAVE` was requested.
    tracking: bool,
    /// Physical height of the info block (measured while painting).
    info_h: i32,
    /// Scrollbar thumb drag: offset of the mouse from the thumb top.
    drag: Option<i32>,
    icons: [HICON; 2],
}

thread_local! {
    static GAMUT: RefCell<Option<Gamut>> = const { RefCell::new(None) };
    static HWND_CELL: Cell<isize> = const { Cell::new(0) };
}

/// Runs `f` on the window state unless the window is closed or the state is already borrowed.
fn with<T>(f: impl FnOnce(&mut Gamut) -> T) -> Option<T> {
    GAMUT.with(|m| m.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

impl Gamut {
    /// Window state at `dpi`, entries built for `selected_file` and monitor `mon`. No window yet.
    fn new(selected_file: Option<&str>, mon: usize, dpi: u32) -> Gamut {
        let (entries, monitor_entry) = build_entries(selected_file, mon);
        let mut gamut = Gamut {
            hwnd: std::ptr::null_mut(),
            dpi,
            fonts: Fonts::new(dpi),
            canvas: None,
            horseshoe: None,
            entries,
            rows: Vec::new(),
            scroll: 0,
            selected: monitor_entry.or(Some(0)),
            keyboard: false,
            hover: None,
            tracking: false,
            info_h: sc(INFO_H_INITIAL, dpi),
            drag: None,
            icons: [std::ptr::null_mut(); 2],
        };
        gamut.rebuild_rows();
        gamut
    }

    /// "Reference spaces" with the six standard spaces, then "Installed profiles" when there are any.
    fn rebuild_rows(&mut self) {
        self.rows.clear();
        self.rows.push(Row::Header("Reference spaces"));
        let reference = self.entries.iter().enumerate().filter(|(_, e)| e.reference);
        self.rows.extend(reference.map(|(i, _)| Row::Entry(i)));
        if self.entries.iter().any(|e| !e.reference) {
            self.rows.push(Row::Header("Installed profiles"));
            let installed = self.entries.iter().enumerate().filter(|(_, e)| !e.reference);
            self.rows.extend(installed.map(|(i, _)| Row::Entry(i)));
        }
    }

    /// Ticks or unticks entry `i` (profiles without primaries can't be plotted).
    fn toggle(&mut self, i: usize) {
        if let Some(entry) = self.entries.get_mut(i).filter(|e| e.prims.is_some()) {
            entry.checked = !entry.checked;
        }
    }

    fn invalidate(&self) {
        window::invalidate(self.hwnd);
    }
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

/// The window handle (null when closed).
pub fn hwnd() -> HWND {
    HWND_CELL.with(|c| c.get()) as HWND
}

/// True while the window exists.
pub fn is_open() -> bool {
    !hwnd().is_null()
}

/// Opens the window with the monitor's EDID-derived profile, sRGB and Display P3 ticked (screenshots).
/// `mon` is the 0-based monitor index whose associations tag the profiles.
pub fn open_showcase(file: String, label: String, prims: Option<Primaries>, mon: usize) {
    SHOWCASE.with(|s| {
        *s.borrow_mut() = Some(Showcase {
            file: file.clone(),
            label,
            prims,
        })
    });
    open(Some(file), mon);
    SHOWCASE.with(|s| *s.borrow_mut() = None);
}

/// `mon` is the 0-based index of the monitor selected in the main window: its SDR and HDR associations
/// decide which profiles are tagged SDR and HDR.
pub fn open(selected_monitor_profile: Option<String>, mon: usize) {
    let existing = hwnd();
    if !existing.is_null() {
        window::restore_and_focus(existing);
        return;
    }
    proc::register_class();
    let dpi = system_dpi();
    let gamut = Gamut::new(selected_monitor_profile.as_deref(), mon, dpi);
    GAMUT.with(|m| *m.borrow_mut() = Some(gamut));
    // GAMUT is not borrowed while the window is created: WM_CREATE runs inside.
    let (w, h) = (sc(CLIENT_W, dpi), sc(CLIENT_H, dpi));
    let hwnd = window::create_centered(CLASS, "Spectrum: compare gamuts", w, h, dpi);
    if hwnd.is_null() {
        GAMUT.with(|m| *m.borrow_mut() = None);
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

/// Off-screen render of the showcase window (`--render gamut`): the same entries and `draw` as
/// `open_showcase`, at `dpi`, without a window.
pub fn render_showcase(
    file: String,
    label: String,
    prims: Option<Primaries>,
    mon: usize,
    dpi: u32,
) -> Option<Canvas> {
    SHOWCASE.with(|s| {
        *s.borrow_mut() = Some(Showcase {
            file: file.clone(),
            label,
            prims,
        })
    });
    let mut gamut = Gamut::new(Some(&file), mon, dpi);
    SHOWCASE.with(|s| *s.borrow_mut() = None);
    gamut.ensure_horseshoe();
    let mut c = Canvas::new(sc(CLIENT_W, dpi), sc(CLIENT_H, dpi))?;
    // The first pass measures the info block, as the window's first paint does.
    gamut.draw(&mut c);
    gamut.ensure_horseshoe();
    gamut.draw(&mut c);
    Some(c)
}

/// Plot rectangle and client size at `dpi`, and the data window it maps: (plot, (w, h), (x_min, x_max, y_max)).
pub fn plot_geometry(dpi: u32) -> (R, (i32, i32), (f64, f64, f64)) {
    (
        geo(dpi, sc(INFO_H_INITIAL, dpi)).plot,
        (sc(CLIENT_W, dpi), sc(CLIENT_H, dpi)),
        (X_MIN, X_MAX, Y_MAX),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn showcase_window(dpi: u32) -> Gamut {
        let st = crate::showcase::state();
        let (label, file) = st.monitors[0].sdr_options[0].clone();
        SHOWCASE.with(|s| {
            *s.borrow_mut() = Some(Showcase {
                file: file.clone(),
                label,
                prims: Some(crate::showcase::monitor_edid().primaries),
            })
        });
        let gamut = Gamut::new(Some(&file), 0, dpi);
        SHOWCASE.with(|s| *s.borrow_mut() = None);
        gamut
    }

    #[test]
    fn rows_list_references_then_installed_profiles() {
        let g = showcase_window(96);
        assert!(matches!(g.rows[0], Row::Header("Reference spaces")));
        assert!(matches!(g.rows[7], Row::Header("Installed profiles")));
        assert_eq!(g.rows.len(), 1 + 6 + 1 + 3);
        // The monitor's profile is first among the installed ones and selected.
        assert!(matches!(g.rows[8], Row::Entry(6)));
        assert_eq!(g.selected, Some(6));
        let ticked: Vec<&str> = g
            .entries
            .iter()
            .filter(|e| e.checked)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(ticked, ["sRGB", "Display P3", "QD-OLED 27 (EDID)"]);
    }

    #[test]
    fn only_plottable_entries_toggle() {
        let mut g = showcase_window(96);
        g.toggle(2);
        assert!(g.entries[2].checked);
        g.toggle(2);
        assert!(!g.entries[2].checked);
        g.entries[2].prims = None;
        g.toggle(2);
        assert!(!g.entries[2].checked);
        g.toggle(999);
    }

    #[test]
    fn showcase_renders_at_the_screenshot_size() {
        let st = crate::showcase::state();
        let (label, file) = st.monitors[0].sdr_options[0].clone();
        let prims = Some(crate::showcase::monitor_edid().primaries);
        let c = render_showcase(file, label, prims, 0, 96).unwrap();
        assert_eq!((c.width(), c.height()), (CLIENT_W, CLIENT_H));
        let (plot, size, domain) = plot_geometry(96);
        assert_eq!(size, (CLIENT_W, CLIENT_H));
        assert_eq!(plot, R::new(56, 40, 566, 580));
        assert_eq!(domain, (X_MIN, X_MAX, Y_MAX));
    }
}
