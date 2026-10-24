//! The main window's layout: one vertical flow of rows built from the design tokens, and the widget list
//! placed on it.

use super::super::design::{
    CAPTION_H, CHECK, CONTROL_H, HEADING_H, PAD, ROW_H, SPACE_L, SPACE_M, SPACE_S, SPACE_XS, TOGGLE_H,
    TOGGLE_W,
};
use super::super::draw::{sc, R};
use super::super::widgets::Kind;
use super::{Id, Main, CLIENT_W, SLIDERS};
use crate::app::UiMonitor;

pub(super) const MARGIN: i32 = PAD;
/// Width of the label and value columns of a slider row.
const LABEL_W: i32 = 128;
const VALUE_W: i32 = 80;
const IDENTIFY_W: i32 = 96;
const MAP_H: i32 = 80;
/// Width of the "HDR" heading before its hint.
const HDR_HEAD_W: i32 = 80;

/// Fixed (non-monitor-dependent) layout in physical pixels.
#[derive(Clone, Copy, Default)]
pub(super) struct Geo {
    pub map: R,
    pub identify: R,
    pub info: R,
    pub sdr_head: R,
    pub sdr: R,
    pub read: R,
    pub compare: R,
    pub manage: R,
    /// Divider between the SDR and HDR sections (a 1 px row).
    pub div1: i32,
    pub hdr_head: R,
    pub hdr_hint: R,
    pub toggle: R,
    pub advanced: R,
    /// (label, slider, value) per slider row, in `SLIDERS` order.
    pub rows: [(R, R, R); 5],
    /// Advanced toggle row: (label, toggle)
    pub cap: (R, R),
    /// Only laid out (non-empty) while HDR is on.
    pub reset: R,
    /// Divider above the footer.
    pub div2: i32,
    pub autostart: R,
    /// Only laid out (non-empty) after an action failed.
    pub error: R,
    /// Client height in physical pixels.
    pub h: i32,
}

/// Which optional groups take space: the Advanced rows, the HDR restore button and the error line.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(super) struct Shape {
    pub advanced: bool,
    pub restore: bool,
    pub error: bool,
}

/// Client height at 96 dpi.
pub(super) fn client_h(shape: Shape) -> i32 {
    geo(96, shape).h
}

/// Places rects at `dpi` from logical coordinates.
struct Flow {
    dpi: u32,
    x0: i32,
    x1: i32,
}

impl Flow {
    fn rect(&self, l: i32, t: i32, r: i32, b: i32) -> R {
        let s = |v: i32| sc(v, self.dpi);
        R::new(s(l), s(t), s(r), s(b))
    }

    /// Full-width row of height `h` at `y`.
    fn row(&self, y: i32, h: i32) -> R {
        self.rect(self.x0, y, self.x1, y + h)
    }

    /// (label, slider, value) at `y`.
    fn slider_row(&self, y: i32) -> (R, R, R) {
        let (x0, x1) = (self.x0, self.x1);
        (
            self.rect(x0, y, x0 + LABEL_W, y + CONTROL_H),
            self.rect(x0 + LABEL_W + SPACE_XS, y, x1 - VALUE_W - SPACE_XS, y + CONTROL_H),
            self.rect(x1 - VALUE_W, y, x1, y + CONTROL_H),
        )
    }

    /// A toggle at the right end of a row of height `row_h` at `y`.
    fn toggle(&self, y: i32, row_h: i32) -> R {
        let ty = y + (row_h - TOGGLE_H) / 2;
        self.rect(self.x1 - TOGGLE_W, ty, self.x1, ty + TOGGLE_H)
    }
}

/// The whole window is one vertical flow built from the design tokens.
pub(super) fn geo(dpi: u32, shape: Shape) -> Geo {
    let f = Flow {
        dpi,
        x0: MARGIN,
        x1: CLIENT_W - MARGIN,
    };
    let (x0, x1) = (f.x0, f.x1);
    let mut g = Geo::default();
    let mut y = PAD;

    // Monitors: map on the left, Identify on the right.
    g.map = f.rect(x0, y, x1 - IDENTIFY_W - SPACE_M, y + MAP_H);
    g.identify = f.rect(x1 - IDENTIFY_W, y, x1, y + CONTROL_H);
    y += MAP_H + SPACE_S;
    g.info = f.row(y, CAPTION_H + 2);
    y += CAPTION_H + 2 + SPACE_L;

    // SDR section: heading and profile dropdown on one row, then the three actions in one row.
    g.sdr_head = f.rect(x0, y, x0 + LABEL_W, y + CONTROL_H);
    g.sdr = f.rect(x0 + LABEL_W + SPACE_XS, y, x1, y + CONTROL_H);
    y += CONTROL_H + SPACE_S;
    let bw = (x1 - x0 - 2 * SPACE_S) / 3;
    g.read = f.rect(x0, y, x0 + bw, y + CONTROL_H);
    g.compare = f.rect(x0 + bw + SPACE_S, y, x0 + 2 * bw + SPACE_S, y + CONTROL_H);
    g.manage = f.rect(x1 - bw, y, x1, y + CONTROL_H);
    y += CONTROL_H + SPACE_L;
    g.div1 = sc(y, dpi);
    y += 1 + SPACE_L;

    // HDR section.
    g.hdr_head = f.rect(x0, y, x0 + HDR_HEAD_W, y + HEADING_H);
    g.hdr_hint = f.rect(x0 + HDR_HEAD_W, y, x1 - TOGGLE_W - SPACE_M, y + HEADING_H);
    g.toggle = f.toggle(y, HEADING_H);
    y += HEADING_H + SPACE_S;
    g.rows[4] = f.slider_row(y);
    y += ROW_H;
    g.advanced = f.row(y, CONTROL_H);
    y += CONTROL_H;
    if shape.advanced {
        y += SPACE_XS;
        for row in g.rows.iter_mut().take(4) {
            *row = f.slider_row(y);
            y += ROW_H;
        }
        g.cap = (
            f.rect(x0, y, x1 - TOGGLE_W - SPACE_M, y + CONTROL_H),
            f.toggle(y, CONTROL_H),
        );
        y += ROW_H;
    }
    if shape.restore {
        y += SPACE_S;
        g.reset = f.row(y, CONTROL_H);
        y += CONTROL_H;
    }
    y += SPACE_L;
    g.div2 = sc(y, dpi);
    y += 1 + SPACE_L;

    // Footer.
    g.autostart = f.row(y, CHECK + 4);
    y += CHECK + 4;
    if shape.error {
        y += SPACE_S;
        g.error = f.row(y, CAPTION_H);
        y += CAPTION_H;
    }
    y += PAD;
    g.h = sc(y, dpi);
    g
}

/// Scaled desktop layout of the monitors fitted into `area`; one tile (with gap) per monitor.
pub(super) fn tile_rects(mons: &[UiMonitor], area: R, dpi: u32) -> Vec<R> {
    let left = mons.iter().map(|m| m.rect.left).min().unwrap_or(0);
    let top = mons.iter().map(|m| m.rect.top).min().unwrap_or(0);
    let right = mons.iter().map(|m| m.rect.right).max().unwrap_or(1);
    let bottom = mons.iter().map(|m| m.rect.bottom).max().unwrap_or(1);
    let (bw, bh) = (((right - left).max(1)) as f32, ((bottom - top).max(1)) as f32);
    let scale = (area.w() as f32 / bw).min(area.h() as f32 / bh);
    let (ow, oh) = (bw * scale, bh * scale);
    let (ox, oy) = (
        area.l as f32 + (area.w() as f32 - ow) / 2.0,
        area.t as f32 + (area.h() as f32 - oh) / 2.0,
    );
    let gap = sc(3, dpi);
    mons.iter()
        .map(|m| {
            let l = ox + (m.rect.left - left) as f32 * scale;
            let t = oy + (m.rect.top - top) as f32 * scale;
            let r = ox + (m.rect.right - left) as f32 * scale;
            let b = oy + (m.rect.bottom - top) as f32 * scale;
            R::new(
                l.round() as i32 + gap,
                t.round() as i32 + gap,
                r.round() as i32 - gap,
                b.round() as i32 - gap,
            )
        })
        .collect()
}

impl Main {
    /// Recomputes the geometry for the current shape and rebuilds the widget list on it. Controls the
    /// shown monitor can't use are disabled; HDR-only rows are left out while they take no space.
    pub(super) fn layout(&mut self) {
        let dpi = self.dpi;
        let g = geo(dpi, self.shape());
        self.geo = g;
        let mon = self.mon().cloned();
        let has = |test: fn(&UiMonitor) -> bool| mon.as_ref().is_some_and(test);
        let hdr_on = self.hdr_enabled();
        let ui = &mut self.ui;
        ui.clear();
        if !self.state.monitors.is_empty() {
            for (i, r) in tile_rects(&self.state.monitors, g.map, dpi)
                .into_iter()
                .enumerate()
            {
                ui.add(Id::Tile(i), Kind::Area, r, true, false);
            }
        }
        ui.add(
            Id::Identify,
            Kind::Button,
            g.identify,
            !self.state.monitors.is_empty(),
            true,
        );
        ui.add(
            Id::Sdr,
            Kind::Dropdown,
            g.sdr,
            has(|m| !m.sdr_options.is_empty()),
            true,
        );
        ui.add(Id::Read, Kind::Button, g.read, has(|m| m.has_edid), true);
        ui.add(Id::Compare, Kind::Button, g.compare, true, true);
        ui.add(Id::Manage, Kind::Button, g.manage, true, true);
        ui.add(
            Id::HdrToggle,
            Kind::Toggle,
            g.toggle,
            has(|m| m.hdr_supported),
            true,
        );
        let white_live = has(|m| m.sdr_white_nits.is_some());
        ui.add(
            Id::SdrWhite,
            Kind::Slider,
            g.rows[4].1,
            hdr_on && white_live,
            true,
        );
        ui.add(Id::Advanced, Kind::Button, g.advanced, true, true);
        if self.state.advanced {
            for (i, id) in SLIDERS.iter().take(4).enumerate() {
                ui.add(*id, Kind::Slider, g.rows[i].1, hdr_on, true);
            }
            if has(|m| m.sdr_cap().is_some()) {
                ui.add(Id::SdrCap, Kind::Toggle, g.cap.1, hdr_on, true);
            }
        }
        if hdr_on {
            ui.add(Id::Reset, Kind::Button, g.reset, true, true);
        }
        ui.add(Id::Autostart, Kind::Checkbox, g.autostart, true, true);
        // Drop focus that points at something now disabled.
        if let Some(f) = ui.focus {
            if !ui.get(f).map(|w| w.enabled).unwrap_or(false) && !matches!(f, Id::Tile(_)) {
                ui.focus = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{enabled, without_hdr};
    use super::*;

    fn all_shapes() -> Vec<Shape> {
        let mut v = Vec::new();
        for bits in 0..8 {
            v.push(Shape {
                advanced: bits & 1 != 0,
                restore: bits & 2 != 0,
                error: bits & 4 != 0,
            });
        }
        v
    }

    #[test]
    fn showcase_shape_is_the_screenshot_size() {
        let full = Shape {
            advanced: true,
            restore: true,
            error: false,
        };
        assert_eq!(client_h(full), 654);
        assert_eq!(geo(192, full).h, 1308);
    }

    #[test]
    fn optional_groups_add_height() {
        let base = client_h(Shape::default());
        let with = |s: Shape| client_h(s) - base;
        let advanced = with(Shape {
            advanced: true,
            ..Shape::default()
        });
        assert_eq!(advanced, SPACE_XS + 5 * ROW_H);
        let restore = with(Shape {
            restore: true,
            ..Shape::default()
        });
        assert_eq!(restore, SPACE_S + CONTROL_H);
        let error = with(Shape {
            error: true,
            ..Shape::default()
        });
        assert_eq!(error, SPACE_S + CAPTION_H);
    }

    #[test]
    fn rows_flow_down_without_overlap_inside_the_client() {
        for dpi in [96, 120, 144, 192] {
            for shape in all_shapes() {
                let g = geo(dpi, shape);
                let width = sc(CLIENT_W, dpi);
                let mut rows = vec![g.map, g.info, g.sdr, g.read, g.toggle, g.rows[4].1, g.advanced];
                if shape.advanced {
                    rows.extend(g.rows[..4].iter().map(|r| r.1));
                    rows.push(g.cap.1);
                }
                if shape.restore {
                    rows.push(g.reset);
                }
                rows.push(g.autostart);
                if shape.error {
                    rows.push(g.error);
                }
                for r in &rows {
                    assert!(
                        r.l >= 0 && r.r <= width && r.t >= 0 && r.b <= g.h,
                        "{shape:?} {dpi}"
                    );
                    assert!(r.w() > 0 && r.h() > 0, "{shape:?} {dpi}");
                }
                assert!(g.read.b < g.div1 && g.div1 < g.hdr_head.t, "{shape:?} {dpi}");
                assert!(
                    g.advanced.b <= g.div2 && g.div2 < g.autostart.t,
                    "{shape:?} {dpi}"
                );
                if shape.advanced {
                    for pair in g.rows[..4].windows(2) {
                        assert!(pair[0].1.b <= pair[1].1.t, "{shape:?} {dpi}");
                    }
                    assert!(g.advanced.b <= g.rows[0].1.t);
                }
                // Label, slider and value of a row sit side by side.
                let (label, slider, value) = g.rows[4];
                assert!(label.r < slider.l && slider.r < value.l, "{dpi}");
            }
        }
    }

    #[test]
    fn sdr_actions_split_the_row_in_three() {
        let g = geo(96, Shape::default());
        assert_eq!(g.read.w(), g.manage.w());
        assert_eq!(g.compare.w(), g.read.w());
        assert_eq!((g.read.l, g.manage.r), (MARGIN, CLIENT_W - MARGIN));
        assert!(g.read.r < g.compare.l && g.compare.r < g.manage.l);
    }

    #[test]
    fn tiles_keep_the_desktop_arrangement() {
        let mut st = crate::showcase::state();
        let mut right = st.monitors[0].clone();
        right.rect = crate::monitors::Rect {
            left: 2560,
            top: 0,
            right: 4480,
            bottom: 1080,
        };
        st.monitors.push(right);
        let area = R::new(0, 0, 300, 80);
        let tiles = tile_rects(&st.monitors, area, 96);
        assert_eq!(tiles.len(), 2);
        assert!(tiles[0].r < tiles[1].l, "side by side with a gap");
        assert!(tiles[0].h() > tiles[1].h(), "the 1440p monitor is taller");
        for t in &tiles {
            assert!(t.l >= area.l && t.r <= area.r && t.t >= area.t && t.b <= area.b);
        }
        assert!(tile_rects(&[], area, 96).is_empty());
    }

    #[test]
    fn hdr_rows_follow_the_hdr_state() {
        let mut st = crate::showcase::state();
        st.advanced = false;
        let main = Main::new(st.clone(), 96);
        assert_eq!(enabled(&main, Id::SdrWhite), Some(true));
        assert!(main.ui.get(Id::Peak).is_none(), "Advanced rows are collapsed");
        assert!(main.ui.get(Id::Reset).is_some());
        st.advanced = true;
        let main = Main::new(st, 96);
        assert_eq!(enabled(&main, Id::Peak), Some(true));
        assert_eq!(enabled(&main, Id::SdrCap), Some(true));
        let main = Main::new(without_hdr(), 96);
        assert!(!main.shape().restore);
        assert!(main.geo.reset.w() == 0, "no restore row without HDR");
    }

    #[test]
    fn focus_on_a_disabled_control_is_dropped() {
        let mut main = Main::new(crate::showcase::state(), 96);
        main.ui.focus = Some(Id::Reset);
        main.state.monitors[0].hdr_on = false;
        main.layout();
        assert_eq!(main.ui.focus, None);
        main.ui.focus = Some(Id::Tile(0));
        main.layout();
        assert_eq!(main.ui.focus, Some(Id::Tile(0)));
    }
}
