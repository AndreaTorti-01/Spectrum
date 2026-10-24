//! Drawing the main window's client area, top to bottom. The same code paints the window and the
//! off-screen screenshots.

use super::super::design;
use super::super::draw::{mix, sc, scf, Canvas, Theme, R, TA_CENTER, TA_LEFT, TA_RIGHT};
use super::super::widgets::{
    self, draw_button, draw_checkbox, draw_dropdown, draw_expander, draw_slider, draw_slider_tick,
    draw_toggle, white_scale, BtnStyle, Ctx,
};
use super::super::window;
use super::layout::MARGIN;
use super::{Id, Main, CLIENT_W, SLIDERS};
use crate::app::UiMonitor;
use windows_sys::Win32::Graphics::Gdi::HDC;

/// Labels of the slider rows, in `SLIDERS` order.
const SLIDER_LABELS: [&str; 5] = [
    "Black level",
    "Peak brightness",
    "Full-frame",
    "Saturation",
    "SDR content",
];

impl Main {
    /// `WM_PAINT`: draws into the back buffer and blits it.
    pub(super) fn paint(&mut self, hdc: HDC) {
        let Some(mut canvas) = window::client_canvas(self.hwnd, &mut self.canvas) else {
            return;
        };
        self.draw(&mut canvas);
        canvas.present(hdc, 0, 0);
        self.canvas = Some(canvas);
    }

    pub(super) fn draw(&self, c: &mut Canvas) {
        let th = self.theme();
        let ctx = Ctx {
            th,
            fonts: &self.fonts,
            dpi: self.dpi,
        };
        c.clear(th.bg);
        self.draw_monitor_map(c, &ctx);
        self.draw_sdr_section(c, &ctx);
        self.draw_hdr_section(c, &ctx);
        self.draw_footer(c, &ctx);
    }

    /// A 1 px line across the content width at client row `y`.
    fn draw_divider(&self, c: &mut Canvas, th: &Theme, y: i32) {
        let dpi = self.dpi;
        c.fill_rect(
            R::new(sc(MARGIN, dpi), y, sc(CLIENT_W - MARGIN, dpi), y + 1),
            th.line,
        );
    }

    /// Monitor tiles with their numbers, Identify, and the info line of the shown monitor.
    fn draw_monitor_map(&self, c: &mut Canvas, ctx: &Ctx) {
        let (th, dpi, geo) = (&ctx.th, self.dpi, &self.geo);
        if self.state.monitors.is_empty() {
            c.text(
                geo.map,
                "No active displays found",
                self.fonts.body,
                th.text_dim,
                TA_LEFT,
            );
        }
        for (i, m) in self.state.monitors.iter().enumerate() {
            let Some(w) = self.ui.get(Id::Tile(i)) else {
                continue;
            };
            let r = w.rect;
            let hover = self.ui.state(Id::Tile(i)).hover;
            let radius = scf(8.0, dpi);
            let fill = if i == self.selected {
                mix(th.surface, m.accent, if hover { 0.28 } else { 0.18 })
            } else if hover {
                th.hover_fill()
            } else {
                th.surface
            };
            c.fill_rrect(r, radius, fill, 1.0);
            c.stroke_rrect(r, radius, scf(2.0, dpi), m.accent, 1.0);
            let font = if r.h() < sc(46, dpi) {
                self.fonts.subtitle
            } else {
                self.fonts.display
            };
            let number = th.with_accent(m.accent).accent_text();
            c.text(r, &m.num.to_string(), font, number, TA_CENTER);
        }
        draw_button(
            c,
            ctx,
            geo.identify,
            "Identify",
            self.ui.state(Id::Identify),
            BtnStyle::Secondary,
        );
        if let Some(m) = self.mon() {
            self.draw_info_line(c, th, m);
        }
    }

    /// Accent dot, then "name · width × height · HDR state".
    fn draw_info_line(&self, c: &mut Canvas, th: &Theme, m: &UiMonitor) {
        let (dpi, info) = (self.dpi, self.geo.info);
        c.fill_circle(
            info.l as f32 + scf(4.0, dpi),
            info.cy(),
            scf(4.0, dpi),
            m.accent,
            1.0,
        );
        let hdr = if !m.hdr_supported {
            "HDR not supported"
        } else if m.hdr_on {
            "HDR on"
        } else {
            "HDR off"
        };
        let line = format!("{} \u{b7} {} \u{d7} {} \u{b7} {}", m.name, m.width, m.height, hdr);
        c.text(
            R::new(info.l + sc(18, dpi), info.t, info.r, info.b),
            &line,
            self.fonts.body,
            th.text,
            TA_LEFT,
        );
    }

    /// SDR profile heading and picker, then Read from monitor, Compare gamuts and Manage profiles.
    fn draw_sdr_section(&self, c: &mut Canvas, ctx: &Ctx) {
        let (th, geo, ui) = (&ctx.th, &self.geo, &self.ui);
        c.text(geo.sdr_head, "SDR profile", self.fonts.subtitle, th.text, TA_LEFT);
        let current = self
            .mon()
            .and_then(|m| {
                m.sdr_current
                    .and_then(|i| m.sdr_options.get(i))
                    .map(|o| o.0.clone())
            })
            .unwrap_or_else(|| "Not set".to_string());
        draw_dropdown(
            c,
            ctx,
            geo.sdr,
            &current,
            ui.state(Id::Sdr),
            widgets::dropdown_is_open(),
        );
        for (id, rect, label) in [
            (Id::Read, geo.read, "Read from monitor"),
            (Id::Compare, geo.compare, "Compare gamuts\u{2026}"),
            (Id::Manage, geo.manage, "Manage profiles\u{2026}"),
        ] {
            draw_button(c, ctx, rect, label, ui.state(id), BtnStyle::Secondary);
        }
    }

    /// HDR heading with its hint and toggle, the slider rows, the clipping toggle, Advanced and restore.
    fn draw_hdr_section(&self, c: &mut Canvas, ctx: &Ctx) {
        let (th, geo, ui) = (&ctx.th, &self.geo, &self.ui);
        let mon = self.mon();
        self.draw_divider(c, th, geo.div1);
        c.text(geo.hdr_head, "HDR", self.fonts.subtitle, th.text, TA_LEFT);
        let hint = match mon {
            Some(m) if !m.hdr_supported => Some("Not supported by this display"),
            Some(m) if !m.hdr_on => Some("Turn on HDR to tune it"),
            _ => None,
        };
        let enabled = self.hdr_enabled();
        if let Some(h) = hint {
            let color = if enabled { design::warning(th) } else { th.text_dim };
            c.text(geo.hdr_hint, h, self.fonts.caption, color, TA_RIGHT);
        }
        draw_toggle(
            c,
            ctx,
            geo.toggle,
            mon.map(|m| m.hdr_on).unwrap_or(false),
            ui.state(Id::HdrToggle),
        );
        self.draw_slider_rows(c, ctx);
        if self.state.advanced && mon.is_some_and(|m| m.sdr_cap().is_some()) {
            let (label, toggle) = geo.cap;
            let label_color = if enabled {
                th.text
            } else {
                design::disabled_text(th)
            };
            c.text(
                label,
                "Allow clipping above the monitor peak brightness",
                self.fonts.body,
                label_color,
                TA_LEFT,
            );
            draw_toggle(
                c,
                ctx,
                toggle,
                mon.is_some_and(|m| m.sdr_above_peak),
                ui.state(Id::SdrCap),
            );
        }
        draw_expander(
            c,
            ctx,
            geo.advanced,
            "Advanced",
            self.state.advanced,
            ui.state(Id::Advanced),
        );
        if self.shape().restore {
            draw_button(
                c,
                ctx,
                geo.reset,
                "Restore recommended HDR settings",
                ui.state(Id::Reset),
                BtnStyle::Secondary,
            );
        }
    }

    /// Label, slider and value of every visible slider row. SDR content above the EDID cap shows in amber,
    /// with a tick where the monitor's peak is.
    fn draw_slider_rows(&self, c: &mut Canvas, ctx: &Ctx) {
        let th = &ctx.th;
        let enabled = self.hdr_enabled();
        let label_color = if enabled {
            th.text
        } else {
            design::disabled_text(th)
        };
        let dim_value = mix(th.text_dim, th.bg, 0.5);
        let above_cap = enabled && self.mon().is_some_and(|m| m.sdr_over_cap(self.sdr_white));
        for (i, id) in SLIDERS.iter().enumerate() {
            if i < 4 && !self.state.advanced {
                continue;
            }
            let (label, slider, value) = self.geo.rows[i];
            c.text(label, SLIDER_LABELS[i], self.fonts.body, label_color, TA_LEFT);
            let state = self.ui.state(*id);
            if *id == Id::SdrWhite {
                let max = self.white_max();
                if let Some(p) = self.white_target().filter(|p| *p <= max) {
                    draw_slider_tick(c, ctx, slider, white_scale(max).to_t(p), state.enabled);
                }
            }
            draw_slider(c, ctx, slider, self.slider_t(*id), state, self.drag == Some(*id));
            let value_color = if !enabled {
                dim_value
            } else if *id == Id::SdrWhite && above_cap {
                design::warning(th)
            } else {
                th.text
            };
            c.text(
                value,
                &self.slider_text(*id),
                self.fonts.body,
                value_color,
                TA_RIGHT,
            );
        }
    }

    /// Start with Windows, and the last action's error.
    fn draw_footer(&self, c: &mut Canvas, ctx: &Ctx) {
        let (th, geo) = (&ctx.th, &self.geo);
        self.draw_divider(c, th, geo.div2);
        draw_checkbox(
            c,
            ctx,
            geo.autostart,
            "Start with Windows",
            self.state.autostart,
            self.ui.state(Id::Autostart),
        );
        if let Some(e) = &self.state.error {
            c.text(geo.error, e, self.fonts.caption, design::warning(th), TA_LEFT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(main: &Main) -> Vec<u32> {
        let (w, h) = (sc(CLIENT_W, main.dpi), main.geo.h);
        let mut c = Canvas::new(w, h).unwrap();
        main.draw(&mut c);
        c.pixels().to_vec()
    }

    #[test]
    fn the_error_line_is_drawn_in_its_row() {
        let mut st = crate::showcase::state();
        st.error = Some("Couldn't apply HDR profile to monitor 1. Try again.".into());
        let main = Main::new(st, 96);
        assert!(main.shape().error);
        let (w, geo) = (sc(CLIENT_W, 96), main.geo);
        let px = pixels(&main);
        let bg = main.theme().bg;
        let row = &px[(geo.error.cy() as i32 * w) as usize..][..w as usize];
        assert!(row.iter().any(|&p| p != bg), "text in the error row");
    }

    #[test]
    fn drawing_depends_only_on_the_state() {
        let a = Main::new(crate::showcase::state(), 96);
        let b = Main::new(crate::showcase::state(), 96);
        assert_eq!(pixels(&a), pixels(&b));
        let mut st = crate::showcase::state();
        st.monitors[0].hdr_on = false;
        assert_ne!(pixels(&a), pixels(&Main::new(st, 96)));
    }
}
