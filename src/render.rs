//! Off-screen screenshots: `--render <surface> --theme light|dark --scale <f> [--frame] --out <file>`.
//!
//! Every window paints into an off-screen `Canvas` and only blits it to the screen at the end, so the same
//! `draw` code renders straight into a PNG here: no window is created, nothing takes focus or appears on
//! screen. The data is the fixed showcase state (`crate::showcase`), so the pictures look the same on any
//! machine and at any system DPI. Surfaces:
//!
//! - `main`, `gamut`, `profiles`: the client area, or with `--frame` inside a drawn Windows 11 frame
//!   (`ui::frame`). `hdr`: the HDR section of the main window (client rows between its two dividers).
//! - `traymenu`: a replica of the tray menu (`ui::traymenu`), which Windows draws for the real app.
//! - `layout`: JSON with the main window and gamut plot geometry the website points at.
//! - `demo`: the README demo, frame by frame at 10 fps into the folder `--out` names
//!   (`frame-NNNN.png`): main window, mock taskbar, tray menu and an arrow cursor on the stage color.

use crate::showcase;
use crate::tray::MenuMonitor;
use crate::ui::draw::{sc, scf, theme, Canvas, Shadow, R};
use crate::ui::{frame, gamut_window, main_window, mockbar, profiles_window, traymenu};
use std::fmt::Write as _;
use std::path::Path;

/// Stage color around the windows (README demo, frame corners, menu corners).
pub fn stage_color(dark: bool) -> u32 {
    if dark {
        0x202020
    } else {
        0xE9E6E1
    }
}

fn dpi_for(scale: f64) -> u32 {
    (96.0 * scale.clamp(0.25, 4.0)).round() as u32
}

/// `src` cropped to `r`.
fn crop(src: &mut Canvas, r: R) -> Option<Canvas> {
    let mut c = Canvas::new(r.w(), r.h())?;
    c.draw_canvas(-r.l, -r.t, src);
    Some(c)
}

fn main_offscreen(dpi: u32) -> main_window::Offscreen {
    main_window::Offscreen::new(showcase::state(), dpi)
}

fn gamut(dpi: u32) -> Option<Canvas> {
    let st = showcase::state();
    let (label, file) = st.monitors.first()?.sdr_options.first()?.clone();
    let prims = Some(showcase::monitor_edid().primaries);
    gamut_window::render_showcase(file, label, prims, 0, dpi)
}

fn menu_items() -> Vec<traymenu::Item> {
    let st = showcase::state();
    let mons: Vec<MenuMonitor> = st.monitors.iter().map(MenuMonitor::from_ui).collect();
    traymenu::items(&mons)
}

/// One surface as a canvas.
pub fn surface(name: &str, dark: bool, dpi: u32, framed: bool) -> Result<Canvas, String> {
    let backdrop = stage_color(dark);
    let frame_it = |c: Option<Canvas>, title: &str| -> Option<Canvas> {
        let mut c = c?;
        if framed {
            frame::framed(&mut c, title, dark, true, backdrop, dpi)
        } else {
            Some(c)
        }
    };
    let c = match name {
        "main" => frame_it(main_offscreen(dpi).render(), "Spectrum"),
        "hdr" => {
            let m = main_offscreen(dpi);
            let (w, _) = m.size();
            let (t, b) = m.marks().hdr_rows;
            m.render().and_then(|mut c| crop(&mut c, R::new(0, t, w, b)))
        }
        "gamut" => frame_it(gamut(dpi), "Spectrum: compare gamuts"),
        "profiles" => frame_it(
            profiles_window::render_showcase(showcase::profiles(), dpi),
            "Spectrum: manage profiles",
        ),
        "traymenu" => traymenu::render(&menu_items(), dark, dpi, None, backdrop),
        _ => return Err(format!("unknown surface {name:?}")),
    };
    c.ok_or_else(|| format!("could not draw {name}"))
}

/// Geometry the website reads (`site/src/data/layout.json`), in logical pixels at 100%.
pub fn layout_json() -> String {
    let m = main_offscreen(96);
    let (w, h) = m.size();
    let k = m.marks();
    let pt = |r: R| format!("{{ \"x\": {}, \"y\": {} }}", r.r, (r.t + r.b) / 2);
    let (plot, (gw, gh), (x_min, x_max, y_max)) = gamut_window::plot_geometry(96);
    let mut o = String::new();
    let _ = writeln!(o, "{{");
    let _ = writeln!(o, "  \"main\": {{");
    let _ = writeln!(o, "    \"width\": {w},");
    let _ = writeln!(o, "    \"height\": {h},");
    let _ = writeln!(
        o,
        "    \"callouts\": [{}, {}, {}],",
        pt(k.identify),
        pt(k.sdr),
        pt(k.peak_value)
    );
    let _ = writeln!(o, "    \"hdrRows\": [{}, {}]", k.hdr_rows.0, k.hdr_rows.1);
    let _ = writeln!(o, "  }},");
    let _ = writeln!(o, "  \"gamut\": {{");
    let _ = writeln!(o, "    \"width\": {gw},");
    let _ = writeln!(o, "    \"height\": {gh},");
    let _ = writeln!(
        o,
        "    \"plot\": {{ \"left\": {}, \"top\": {}, \"right\": {}, \"bottom\": {} }},",
        plot.l, plot.t, plot.r, plot.b
    );
    let _ = writeln!(
        o,
        "    \"domain\": {{ \"xMin\": {x_min}, \"xMax\": {x_max}, \"yMax\": {y_max} }}"
    );
    let _ = writeln!(o, "  }}");
    let _ = writeln!(o, "}}");
    o
}

/// `--render`: writes `name` to `out`. Returns the process exit code.
pub fn run(name: &str, dark: bool, scale: f64, framed: bool, out: Option<&Path>) -> i32 {
    crate::ui::draw::set_forced_theme(Some(dark));
    let Some(out) = out else {
        crate::util::log("render: --out is required");
        return 2;
    };
    let dpi = dpi_for(scale);
    let result = match name {
        "layout" => std::fs::write(out, layout_json()).map_err(|e| e.to_string()),
        "demo" => demo(dark, dpi, out),
        _ => surface(name, dark, dpi, framed)
            .and_then(|mut c| std::fs::write(out, c.to_png()).map_err(|e| e.to_string())),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            crate::util::log(&format!("render {name}: {e}"));
            1
        }
    }
}

// ---------------------------------------------------------------------------------------------
// README demo
// ---------------------------------------------------------------------------------------------
//
// About 8 s at 10 fps: the main window idles, the cursor goes to the tray icon on a mock taskbar, the tray
// menu opens, the pointer walks down to Display P3 and picks it, the menu closes, the main window shows
// Display P3, and the cursor drags Peak brightness from 400 to 1000 nits. Nothing is applied anywhere.

// Times in ms from the start of the recording.
const T_MOVE_TRAY: (i64, i64) = (800, 1300);
const T_HOVER_ON: i64 = 1300;
const T_OPEN_MENU: i64 = 1600;
const T_LEAVE_TRAY: (i64, i64) = (1950, 2650);
const T_FIRST_HIGHLIGHT: i64 = 2650;
const T_HIGHLIGHT_STEP: i64 = 250;
const T_TO_ITEM: (i64, i64) = (2650, 3150);
const T_RADIO: i64 = 3500;
const T_CLOSE_MENU: i64 = 3900;
const T_TO_SLIDER: (i64, i64) = (3950, 4450);
const T_SLIDE: (i64, i64) = (4600, 6600);
const T_END: i64 = 7800;
const FRAME_MS: i64 = 100;
/// Margin of stage color around the window, the menu and the taskbar (at 100% scale).
const STAGE_MARGIN: i32 = 32;
const PEAK_FROM: f64 = 400.0;
const PEAK_TO: f64 = 1000.0;
const TARGET: &str = "Display P3";

type Pt = (f32, f32);

fn smooth(f: f64) -> f64 {
    let f = f.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

fn lerp(a: Pt, b: Pt, f: f64) -> Pt {
    (a.0 + (b.0 - a.0) * f as f32, a.1 + (b.1 - a.1) * f as f32)
}

/// Position at fraction `f` of the arc length of a polyline.
fn along(pts: &[Pt], f: f64) -> Pt {
    let len = |a: Pt, b: Pt| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() as f64;
    let total: f64 = pts.windows(2).map(|w| len(w[0], w[1])).sum();
    let mut left = f.clamp(0.0, 1.0) * total;
    for w in pts.windows(2) {
        let l = len(w[0], w[1]);
        if left <= l || l == 0.0 {
            return lerp(w[0], w[1], if l == 0.0 { 1.0 } else { left / l });
        }
        left -= l;
    }
    pts[pts.len() - 1]
}

fn within(t: i64, span: (i64, i64)) -> f64 {
    smooth((t - span.0) as f64 / (span.1 - span.0) as f64)
}

fn peak_at(t: i64) -> f64 {
    PEAK_FROM + (PEAK_TO - PEAK_FROM) * within(t, T_SLIDE)
}

/// Where everything sits in the frame (frame pixels).
struct Scene {
    size: (i32, i32),
    main: R,
    client: (i32, i32),
    menu: R,
    items: Vec<R>,
    bar: R,
    icon_cx: i32,
    target_idx: usize,
}

impl Scene {
    fn new(main_size: (i32, i32), items: &[traymenu::Item], target_idx: usize, dpi: u32) -> Scene {
        let s = |v: i32| sc(v, dpi);
        let m = s(STAGE_MARGIN);
        let (fw, fh, cx, cy) = frame::outer(main_size.0, main_size.1, dpi);
        let main = R::xywh(m, m, fw, fh);
        let (mw, mh, rects) = traymenu::layout(items, dpi);
        // The menu rises from the tray to the right of the window, its bottom edge at the window's.
        let menu = R::xywh(main.r + s(10), main.b - mh, mw, mh);
        let top = main.b.max(menu.b) + s(24);
        let bar = R::new(main.l, top, menu.r.max(main.r), top + s(mockbar::BAR_H));
        Scene {
            size: (bar.r.max(menu.r) + m, bar.b + m),
            main,
            client: (main.l + cx, main.t + cy),
            items: rects.iter().map(|r| r.offset(menu.l, menu.t)).collect(),
            menu,
            icon_cx: (menu.l + menu.r) / 2,
            bar,
            target_idx,
        }
    }

    /// Where the pointer rests on menu item `i`.
    fn item_pt(&self, i: usize, dpi: u32) -> Pt {
        let r = self.items.get(i).copied().unwrap_or(self.menu);
        ((r.r - sc(40, dpi)) as f32, r.cy())
    }

    /// Scripted cursor position at `t` ms.
    fn cursor_at(&self, t: i64, main: &main_window::Offscreen, dpi: u32) -> Pt {
        let thumb = |nits: f64| {
            let (x, y) = main.peak_thumb(nits);
            (x + self.client.0 as f32, y + self.client.1 as f32)
        };
        let bar_cy = self.bar.cy();
        let tray = (self.icon_cx as f32, bar_cy);
        let first = self.item_pt(0, dpi);
        let target = self.item_pt(self.target_idx, dpi);
        if t >= T_SLIDE.0 {
            return thumb(peak_at(t));
        }
        if t >= T_TO_SLIDER.0 {
            return along(&[target, thumb(PEAK_FROM)], within(t, T_TO_SLIDER));
        }
        if t >= T_TO_ITEM.0 {
            return along(&[first, target], within(t, T_TO_ITEM));
        }
        if t >= T_LEAVE_TRAY.0 {
            let outside = ((self.menu.r + sc(18, dpi)) as f32, bar_cy);
            let r = (outside.0, tray.1);
            return along(&[tray, r, outside, first], within(t, T_LEAVE_TRAY));
        }
        let start = (
            (self.menu.l + self.menu.w() * 3 / 5) as f32,
            (self.main.t + sc(130, dpi)) as f32,
        );
        along(&[start, tray], within(t, T_MOVE_TRAY))
    }
}

// DWM shadows, fitted to captures of the real ones on the stage colors.
fn window_shadow(dark: bool) -> Shadow {
    if dark {
        Shadow {
            side: (0.37, 11.0),
            bottom: (0.6, 42.0),
            top: (0.27, 14.0),
        }
    } else {
        Shadow {
            side: (0.17, 12.6),
            bottom: (0.31, 40.0),
            top: (0.12, 9.0),
        }
    }
}

fn menu_shadow(dark: bool) -> Shadow {
    if dark {
        Shadow {
            side: (0.04, 4.0),
            bottom: (0.1, 9.0),
            top: (0.0, 3.0),
        }
    } else {
        Shadow {
            side: (0.03, 4.0),
            bottom: (0.11, 8.0),
            top: (0.02, 3.0),
        }
    }
}

/// Arrow cursor (white, dark outline, soft shadow) with its tip at `p`.
fn draw_cursor(c: &mut Canvas, p: Pt, dpi: u32) {
    const ARROW: [Pt; 7] = [
        (0.0, 0.0),
        (0.0, 16.5),
        (4.1, 12.9),
        (6.9, 19.2),
        (9.4, 18.1),
        (6.7, 11.9),
        (11.8, 11.9),
    ];
    let s = scf(1.0, dpi);
    let pts: Vec<Pt> = ARROW.iter().map(|a| (p.0 + a.0 * s, p.1 + a.1 * s)).collect();
    let shadow: Vec<Pt> = pts.iter().map(|q| (q.0 + 1.1 * s, q.1 + 1.5 * s)).collect();
    c.fill_polygon(&shadow, 0x000000, 0.25);
    c.fill_polygon(&pts, 0xFFFFFF, 1.0);
    c.polyline(&pts, true, 1.4 * s, 0x1A1A1A, 1.0);
}

/// The demo frame at `t` ms.
fn demo_frame(t: i64, dark: bool, dpi: u32) -> Option<Canvas> {
    let backdrop = stage_color(dark);
    let mut st = showcase::state();
    let target_idx = st.monitors[0]
        .sdr_options
        .iter()
        .position(|o| o.0 == TARGET)
        .unwrap_or(0);
    st.monitors[0].hdr.peak_nits = PEAK_FROM;
    if t >= T_CLOSE_MENU {
        st.monitors[0].sdr_current = Some(target_idx);
    }
    let mut items = menu_items();
    let mut main = main_window::Offscreen::new(st, dpi);
    main.set_peak(peak_at(t));
    let scene = Scene::new(main.size(), &items, target_idx, dpi);
    let menu_open = (T_OPEN_MENU..T_CLOSE_MENU).contains(&t);

    let mut c = Canvas::new(scene.size.0, scene.size.1)?;
    c.clear(backdrop);
    c.shadow(scene.main, dpi, &window_shadow(dark));
    let mut client = main.render()?;
    let mut win = frame::framed(&mut client, "Spectrum", dark, !menu_open, backdrop, dpi)?;
    c.draw_canvas(scene.main.l, scene.main.t, &mut win);

    let mut bar = Canvas::new(scene.bar.w(), scene.bar.h())?;
    bar.clear(backdrop);
    let hover = (T_HOVER_ON..T_CLOSE_MENU).contains(&t);
    mockbar::paint(&mut bar, &theme(), dpi, scene.icon_cx - scene.bar.l, hover);
    c.draw_canvas(scene.bar.l, scene.bar.t, &mut bar);

    if menu_open {
        if t >= T_RADIO {
            traymenu::set_radio(&mut items, target_idx);
        }
        let highlight = (0..=target_idx)
            .rev()
            .find(|&i| t >= T_FIRST_HIGHLIGHT + T_HIGHLIGHT_STEP * i as i64);
        c.shadow(scene.menu, dpi, &menu_shadow(dark));
        let mut menu = traymenu::render(&items, dark, dpi, highlight, backdrop)?;
        c.draw_canvas(scene.menu.l, scene.menu.t, &mut menu);
    }
    draw_cursor(&mut c, scene.cursor_at(t, &main, dpi), dpi);
    Some(c)
}

fn demo(dark: bool, dpi: u32, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("frame-") && name.ends_with(".png") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let mut n = 1;
    let mut t = 0;
    while t < T_END {
        let mut f = demo_frame(t, dark, dpi).ok_or("could not draw a demo frame")?;
        std::fs::write(dir.join(format!("frame-{n:04}.png")), f.to_png()).map_err(|e| e.to_string())?;
        n += 1;
        t += FRAME_MS;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_timeline_is_ordered() {
        assert!(T_MOVE_TRAY.1 <= T_HOVER_ON && T_HOVER_ON < T_OPEN_MENU);
        assert!(T_OPEN_MENU < T_LEAVE_TRAY.0 && T_TO_ITEM.1 < T_RADIO && T_RADIO < T_CLOSE_MENU);
        assert!(T_CLOSE_MENU <= T_TO_SLIDER.0 && T_TO_SLIDER.1 <= T_SLIDE.0 && T_SLIDE.1 < T_END);
        assert_eq!(T_END / FRAME_MS, 78);
    }

    #[test]
    fn peak_slides_from_400_to_1000() {
        assert_eq!(peak_at(0), PEAK_FROM);
        assert_eq!(peak_at(T_SLIDE.1), PEAK_TO);
        assert!(peak_at((T_SLIDE.0 + T_SLIDE.1) / 2) > PEAK_FROM);
    }

    #[test]
    fn layout_matches_the_main_window_geometry() {
        let j = layout_json();
        assert!(
            j.contains("\"width\": 440,") && j.contains("\"height\": 680,"),
            "{j}"
        );
        assert!(
            j.contains("{ \"x\": 420, \"y\": 36 }, { \"x\": 420, \"y\": 160 }, { \"x\": 420, \"y\": 409 }"),
            "{j}"
        );
        assert!(j.contains("\"hdrRows\": [232, 585]"), "{j}");
    }

    #[test]
    fn surfaces_have_the_screenshot_sizes() {
        let size = |name: &str, dpi: u32, framed: bool| {
            let c = surface(name, false, dpi, framed).unwrap();
            (c.width(), c.height())
        };
        assert_eq!(size("main", 192, false), (880, 1360));
        assert_eq!(size("main", 96, true), (442, 707));
        assert_eq!(size("hdr", 96, false), (440, 353));
        assert_eq!(size("traymenu", 96, false), (200, 244));
    }

    #[test]
    fn png_has_the_canvas_size() {
        let mut c = Canvas::new(7, 3).unwrap();
        let png = c.to_png();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 7);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 3);
    }

    #[test]
    fn along_follows_the_polyline() {
        let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        assert_eq!(along(&pts, 0.0), (0.0, 0.0));
        assert_eq!(along(&pts, 0.5), (10.0, 0.0));
        assert_eq!(along(&pts, 1.0), (10.0, 10.0));
    }
}
