//! Drawing the gamut window: the xy plot with the spectral locus, the checklist and the info block.

use super::super::draw::{mix, sc, scf, theme, Canvas, Dib, Theme, R, TA_CENTER, TA_LEFT, TA_RIGHT};
use super::super::widgets::{draw_checkbox, draw_scrollbar, Ctx, WState};
use super::super::window;
use super::layout::Geo;
use super::{Gamut, Row, X_MAX, X_MIN, Y_MAX};
use crate::color::{self, Xy, SPECTRAL_LOCUS};
use windows_sys::Win32::Graphics::Gdi::HDC;

/// Wavelengths (nm) labeled along the locus.
const WAVELENGTHS: [u16; 9] = [460, 480, 500, 520, 540, 560, 580, 600, 620];
/// Height of the fades at the list's scrolled edges, at 96 dpi.
const FADE_H: i32 = 24;

impl Gamut {
    /// `WM_PAINT`: draws into the back buffer and blits it.
    pub(super) fn paint(&mut self, hdc: HDC) {
        let Some(mut canvas) = window::client_canvas(self.hwnd, &mut self.canvas) else {
            return;
        };
        self.ensure_horseshoe();
        self.draw(&mut canvas);
        canvas.present(hdc, 0, 0);
        self.canvas = Some(canvas);
    }

    /// Builds the locus fill for the current plot size, unless it is already there.
    pub(super) fn ensure_horseshoe(&mut self) {
        let plot = self.geo().plot;
        let (w, h) = (plot.w(), plot.h());
        if self.horseshoe.as_ref().map(|d| (d.w, d.h)) != Some((w, h)) {
            self.horseshoe = build_horseshoe(w, h);
        }
    }

    /// Chromaticity (x, y) to client pixels.
    fn plot_xy(&self, x: f64, y: f64) -> (f32, f32) {
        let plot = self.geo().plot;
        (
            plot.l as f32 + ((x - X_MIN) / (X_MAX - X_MIN)) as f32 * plot.w() as f32,
            plot.t as f32 + ((Y_MAX - y) / Y_MAX) as f32 * plot.h() as f32,
        )
    }

    pub(super) fn draw(&mut self, c: &mut Canvas) {
        let th = theme();
        let width = self.geo().info.w();
        let need = self.info_height(c, width);
        if need != self.info_h {
            self.info_h = need;
            self.clamp_scroll();
            if let Some(i) = self.selected {
                self.ensure_visible(i);
            }
        }
        let geo = self.geo();
        c.clear(th.bg);
        self.draw_plot(c, &th, &geo);
        self.draw_list(c, &th, &geo);
        self.draw_info(c, &th, &geo);
    }

    /// Left half: heading, locus fill, grid with axis labels, locus outline with wavelengths, triangles.
    fn draw_plot(&mut self, c: &mut Canvas, th: &Theme, geo: &Geo) {
        c.text(
            geo.head_l,
            "CIE 1931 xy chromaticity",
            self.fonts.subtitle,
            th.text,
            TA_LEFT,
        );
        c.fill_rect(geo.plot, th.surface);
        if let Some(h) = self.horseshoe.as_mut() {
            c.blit_masked(geo.plot.l, geo.plot.t, h);
        }
        self.draw_grid(c, th, geo);
        self.draw_locus(c, th, geo);
        self.draw_triangles(c);
    }

    /// Grid lines every 0.1 with their labels, then the plot border.
    fn draw_grid(&self, c: &mut Canvas, th: &Theme, geo: &Geo) {
        let (dpi, plot) = (self.dpi, geo.plot);
        let line_w = 1.0_f32.max(scf(1.0, dpi).floor());
        for i in 0..=8 {
            let v = i as f64 * 0.1;
            let (px, _) = self.plot_xy(v, 0.0);
            c.line(px, plot.t as f32, px, plot.b as f32, line_w, th.line, 0.9);
            c.text(
                R::new(
                    px as i32 - sc(16, dpi),
                    plot.b + sc(3, dpi),
                    px as i32 + sc(16, dpi),
                    plot.b + sc(19, dpi),
                ),
                &format!("{v:.1}"),
                self.fonts.caption,
                th.text_dim,
                TA_CENTER,
            );
        }
        for i in 0..=9 {
            let v = i as f64 * 0.1;
            let (_, py) = self.plot_xy(0.0, v);
            c.line(plot.l as f32, py, plot.r as f32, py, line_w, th.line, 0.9);
            c.text(
                R::new(
                    plot.l - sc(40, dpi),
                    py as i32 - sc(9, dpi),
                    plot.l - sc(6, dpi),
                    py as i32 + sc(9, dpi),
                ),
                &format!("{v:.1}"),
                self.fonts.caption,
                th.text_dim,
                TA_RIGHT,
            );
        }
        c.stroke_rrect(plot, 0.0, line_w, mix(th.line, th.text_dim, 0.3), 1.0);
    }

    /// The spectral locus outline, a tick and a haloed label per wavelength, and "nm".
    fn draw_locus(&self, c: &mut Canvas, th: &Theme, geo: &Geo) {
        let (dpi, plot) = (self.dpi, geo.plot);
        let pts: Vec<(f32, f32)> = SPECTRAL_LOCUS
            .iter()
            .map(|&(_, x, y)| self.plot_xy(x, y))
            .collect();
        c.polyline(&pts, true, scf(1.5, dpi), mix(th.text_dim, th.text, 0.35), 0.9);
        let centroid = self.plot_xy(0.33, 0.33);
        for nm in WAVELENGTHS {
            let Some(i) = SPECTRAL_LOCUS.iter().position(|p| p.0 == nm) else {
                continue;
            };
            let (p, a, b) = (pts[i], pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
            // Outward normal of the locus at the point.
            let (tx, ty) = (b.0 - a.0, b.1 - a.1);
            let len = (tx * tx + ty * ty).sqrt().max(1e-3);
            let (mut nx, mut ny) = (-ty / len, tx / len);
            if nx * (p.0 - centroid.0) + ny * (p.1 - centroid.1) < 0.0 {
                nx = -nx;
                ny = -ny;
            }
            let tick = scf(5.0, dpi);
            c.line(
                p.0,
                p.1,
                p.0 + nx * tick,
                p.1 + ny * tick,
                scf(1.2, dpi),
                th.text_dim,
                1.0,
            );
            self.draw_wavelength_label(
                c,
                th,
                plot,
                nm,
                (p.0 + nx * scf(17.0, dpi), p.1 + ny * scf(17.0, dpi)),
            );
        }
        c.text(
            R::new(
                plot.r - sc(70, dpi),
                plot.t + sc(2, dpi),
                plot.r - sc(4, dpi),
                plot.t + sc(18, dpi),
            ),
            "nm",
            self.fonts.caption,
            th.text_dim,
            TA_RIGHT,
        );
    }

    /// Label `nm` centered near `at`, clamped inside the plot so it never reaches the axis labels, with a
    /// 2 px halo in the plot background color so it reads over the locus fill.
    fn draw_wavelength_label(&self, c: &mut Canvas, th: &Theme, plot: R, nm: u16, at: (f32, f32)) {
        let dpi = self.dpi;
        let label = nm.to_string();
        let (tw, text_h) = (c.text_width(&label, self.fonts.caption), sc(14, dpi));
        let (hw, hh) = (tw / 2, text_h / 2);
        let margin = sc(4, dpi);
        let lx =
            at.0.clamp((plot.l + margin + hw) as f32, (plot.r - margin - hw) as f32);
        let ly =
            at.1.clamp((plot.t + margin + hh) as f32, (plot.b - margin - hh) as f32);
        let rect_at = |dx: i32, dy: i32| {
            R::new(
                lx as i32 - hw - sc(2, dpi) + dx,
                ly as i32 - hh - sc(2, dpi) + dy,
                lx as i32 + hw + sc(2, dpi) + dx,
                ly as i32 + hh + sc(2, dpi) + dy,
            )
        };
        let d = sc(2, dpi).max(2);
        for (dx, dy) in [
            (-d, 0),
            (d, 0),
            (0, -d),
            (0, d),
            (-d, -d),
            (d, -d),
            (-d, d),
            (d, d),
        ] {
            c.text(rect_at(dx, dy), &label, self.fonts.caption, th.surface, TA_CENTER);
        }
        c.text(rect_at(0, 0), &label, self.fonts.caption, th.text_dim, TA_CENTER);
    }

    /// The ticked gamuts: a shadowed triangle with vertex dots and a white-point cross each.
    fn draw_triangles(&self, c: &mut Canvas) {
        let dpi = self.dpi;
        for (i, entry) in self.entries.iter().enumerate() {
            let (true, Some(p)) = (entry.checked, entry.prims) else {
                continue;
            };
            let corners = [p.r, p.g, p.b].map(|v| self.plot_xy(v.x, v.y));
            let width = if self.selected == Some(i) { 2.8 } else { 2.0 };
            c.polyline(&corners, true, scf(width + 1.5, dpi), 0x000000, 0.35);
            c.polyline(&corners, true, scf(width, dpi), entry.color, 1.0);
            for v in corners {
                c.fill_circle(v.0, v.1, scf(2.6, dpi), entry.color, 1.0);
            }
            let (wx, wy) = self.plot_xy(p.w.x, p.w.y);
            let s = scf(5.0, dpi);
            c.line(wx - s, wy, wx + s, wy, scf(2.5, dpi), 0x000000, 0.4);
            c.line(wx, wy - s, wx, wy + s, scf(2.5, dpi), 0x000000, 0.4);
            c.line(wx - s, wy, wx + s, wy, scf(1.4, dpi), entry.color, 1.0);
            c.line(wx, wy - s, wx, wy + s, scf(1.4, dpi), entry.color, 1.0);
        }
    }

    /// Right column: the list container with its rows, fades, "N more" pill and scrollbar, and the heading.
    fn draw_list(&self, c: &mut Canvas, th: &Theme, geo: &Geo) {
        let dpi = self.dpi;
        let ctx = Ctx {
            th: *th,
            fonts: &self.fonts,
            dpi,
        };
        let radius = scf(6.0, dpi);
        c.fill_rrect(geo.panel, radius, th.surface, 1.0);
        let scroll_list = self.scroll_list();
        let list = geo.list;
        let rows_r = list.r - scroll_list.bar_width();
        let row_h = self.row_h();
        // Rows are whole and aligned with the viewport, so they clip exactly at its edges.
        let mut top = list.t - self.scroll;
        for r in &self.rows {
            if top >= list.t && top + row_h <= list.b {
                match r {
                    Row::Header(t) => {
                        c.text(
                            R::new(list.l + sc(8, dpi), top, rows_r, top + row_h),
                            t,
                            self.fonts.caption,
                            th.text_dim,
                            TA_LEFT,
                        );
                    }
                    Row::Entry(i) => self.draw_row(c, &ctx, *i, R::new(list.l, top, rows_r, top + row_h)),
                }
            }
            top += row_h;
        }
        if let Some(bar) = scroll_list.scrollbar(self.scroll) {
            if self.scroll > 0 {
                fade(c, th, list, rows_r, sc(FADE_H, dpi), true);
            }
            if self.scroll < bar.max_scroll {
                fade(c, th, list, rows_r, sc(FADE_H, dpi), false);
                self.draw_more_pill(c, th, list, rows_r);
            }
            draw_scrollbar(c, th, &bar, self.drag.is_some(), dpi);
        }
        c.stroke_rrect(geo.panel, radius, scf(1.0, dpi).floor().max(1.0), th.line, 1.0);
        c.text(
            geo.head_r,
            "Compare gamuts",
            self.fonts.subtitle,
            th.text,
            TA_LEFT,
        );
    }

    /// "N more" at the bottom right while entries are hidden below the viewport.
    fn draw_more_pill(&self, c: &mut Canvas, th: &Theme, list: R, rows_r: i32) {
        let dpi = self.dpi;
        let first_hidden_top = self.scroll + list.h();
        let row_h = self.row_h();
        let more = self
            .rows
            .iter()
            .enumerate()
            .filter(|(i, r)| *i as i32 * row_h >= first_hidden_top && matches!(r, Row::Entry(_)))
            .count();
        if more == 0 {
            return;
        }
        let label = format!("{more} more");
        let w = c.text_width(&label, self.fonts.caption) + sc(14, dpi);
        let pill = R::new(
            rows_r - sc(6, dpi) - w,
            list.b - sc(22, dpi),
            rows_r - sc(6, dpi),
            list.b - sc(4, dpi),
        );
        c.fill_rrect(pill, scf(9.0, dpi), th.surface, 1.0);
        c.text(pill, &label, self.fonts.caption, th.text_dim, TA_CENTER);
    }

    /// One info block anchored to the bottom of the right column, under a divider.
    fn draw_info(&self, c: &mut Canvas, th: &Theme, geo: &Geo) {
        let (dpi, info) = (self.dpi, geo.info);
        let mut y = info.b;
        if let Some((title, body)) = self.info() {
            let h = c.measure_wrapped(info.w(), &body, self.fonts.caption);
            y -= h;
            c.text_wrapped(
                R::new(info.l, y, info.r, y + h),
                &body,
                self.fonts.caption,
                th.text_dim,
            );
            y -= sc(18, dpi);
            c.text(
                R::new(info.l, y, info.r, y + sc(18, dpi)),
                &title,
                self.fonts.strong,
                th.text,
                TA_LEFT,
            );
        }
        let line_y = info.t + sc(8, dpi);
        c.fill_rect(
            R::new(info.l, line_y, info.r, line_y + sc(1, dpi).max(1)),
            th.line,
        );
    }

    /// A checklist row: hover fill, keyboard ring, checkbox in the entry's color, name and tag.
    fn draw_row(&self, c: &mut Canvas, ctx: &Ctx, i: usize, r: R) {
        let th = &ctx.th;
        let dpi = self.dpi;
        let entry = &self.entries[i];
        let plottable = entry.prims.is_some();
        let inner = r.inset(sc(1, dpi));
        if self.hover == Some(i) {
            c.fill_rrect(inner, scf(6.0, dpi), th.hover_fill(), 0.5);
        }
        if self.keyboard && self.selected == Some(i) {
            c.stroke_rrect(inner, scf(6.0, dpi), scf(1.5, dpi), th.text_dim, 1.0);
        }
        let b = sc(18, dpi);
        let check = R::xywh(r.l + sc(10, dpi), r.t + (r.h() - b) / 2, b, b);
        let check_ctx = Ctx {
            th: th.with_accent(entry.color),
            fonts: ctx.fonts,
            dpi,
        };
        let state = WState {
            hover: self.hover == Some(i),
            pressed: false,
            focus: false,
            enabled: plottable,
        };
        if plottable {
            draw_checkbox(c, &check_ctx, check, "", entry.checked, state);
        } else {
            c.stroke_rrect(check, scf(4.0, dpi), 1.2, mix(th.text_dim, th.bg, 0.5), 1.0);
        }
        let tag_w = if entry.tag.is_empty() {
            0
        } else {
            c.text_width(&entry.tag, ctx.fonts.caption) + sc(10, dpi)
        };
        let name_color = if plottable {
            th.text
        } else {
            mix(th.text_dim, th.bg, 0.3)
        };
        c.text(
            R::new(check.r + sc(10, dpi), r.t, r.r - tag_w - sc(10, dpi), r.b),
            &entry.name,
            ctx.fonts.body,
            name_color,
            TA_LEFT,
        );
        if tag_w > 0 {
            c.text(
                R::new(r.r - tag_w - sc(8, dpi), r.t, r.r - sc(10, dpi), r.b),
                &entry.tag,
                ctx.fonts.caption,
                mix(th.text_dim, th.bg, if plottable { 0.0 } else { 0.3 }),
                TA_RIGHT,
            );
        }
    }
}

/// Fades the rows into the container color over `fade_h` px at the top or bottom edge of `list`.
fn fade(c: &mut Canvas, th: &Theme, list: R, rows_r: i32, fade_h: i32, at_top: bool) {
    for k in 0..fade_h {
        // 0 at the inner end of the fade, ~1 at the container edge
        let t = (k + 1) as f32 / fade_h as f32;
        let alpha = 0.96 * t * t;
        let y = if at_top {
            list.t + fade_h - 1 - k
        } else {
            list.b - fade_h + k
        };
        c.blend_rect(R::new(list.l, y, rows_r, y + 1), th.surface, alpha);
    }
}

fn xy_rgb8(x: f64, y: f64) -> [u8; 3] {
    color::xyz_to_srgb8(color::xy_to_xyz(Xy { x, y }, 1.0))
}

/// Fills the horseshoe once: ARGB pixels (alpha 0xFF inside the locus, 0 outside), colors dimmed to 55 %.
fn build_horseshoe(w: i32, h: i32) -> Option<Dib> {
    let mut dib = Dib::new(w, h)?;
    let px = dib.pixels();
    px.fill(0);
    let poly: Vec<(f64, f64)> = SPECTRAL_LOCUS.iter().map(|&(_, x, y)| (x, y)).collect();
    let n = poly.len();
    for row in 0..h {
        let y = Y_MAX - (row as f64 + 0.5) / h as f64 * Y_MAX;
        // Where the scanline crosses the closed locus, sorted: inside between pairs.
        let mut xs: Vec<f64> = Vec::with_capacity(8);
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.1 <= y && b.1 > y) || (b.1 <= y && a.1 > y) {
                let t = (y - a.1) / (b.1 - a.1);
                xs.push(a.0 + t * (b.0 - a.0));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let c0 = (((pair[0] - X_MIN) / (X_MAX - X_MIN)) * w as f64 - 0.5)
                .ceil()
                .max(0.0) as i32;
            let c1 = (((pair[1] - X_MIN) / (X_MAX - X_MIN)) * w as f64 - 0.5)
                .floor()
                .min(w as f64 - 1.0) as i32;
            for col in c0..=c1 {
                let x = X_MIN + (col as f64 + 0.5) / w as f64 * (X_MAX - X_MIN);
                let rgb = xy_rgb8(x, y);
                let dim = |v: u8| (v as f32 * 0.55).round() as u32;
                px[(row * w + col) as usize] =
                    0xFF00_0000 | (dim(rgb[0]) << 16) | (dim(rgb[1]) << 8) | dim(rgb[2]);
            }
        }
    }
    Some(dib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horseshoe_covers_the_locus_only() {
        let (w, h) = (170, 180);
        let mut dib = build_horseshoe(w, h).unwrap();
        let px = dib.pixels();
        let at = |x: f64, y: f64| {
            let col = ((x - X_MIN) / (X_MAX - X_MIN) * w as f64) as i32;
            let row = ((Y_MAX - y) / Y_MAX * h as f64) as i32;
            px[(row * w + col) as usize]
        };
        assert_eq!(at(0.3127, 0.329) >> 24, 0xFF, "D65 is inside");
        assert_eq!(at(0.7, 0.8) >> 24, 0, "outside the locus");
        assert_eq!(at(0.0, 0.01) >> 24, 0, "left of the locus");
        assert!(build_horseshoe(0, 10).is_none());
    }
}
