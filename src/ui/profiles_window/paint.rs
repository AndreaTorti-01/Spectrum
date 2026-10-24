//! Drawing the profiles window: heading, the list container with its rows and scrollbar, the status line.

use super::super::design;
use super::super::draw::{sc, scf, theme, Canvas, R, TA_CENTER, TA_LEFT};
use super::super::widgets::{draw_button, draw_scrollbar, BtnStyle, Ctx, WState};
use super::super::window;
use super::{geo, Profiles};
use windows_sys::Win32::Graphics::Gdi::HDC;

impl Profiles {
    /// `WM_PAINT`: draws into the back buffer and blits it.
    pub(super) fn paint(&mut self, hdc: HDC) {
        let Some(mut canvas) = window::client_canvas(self.hwnd, &mut self.canvas) else {
            return;
        };
        self.draw(&mut canvas);
        canvas.present(hdc, 0, 0);
        self.canvas = Some(canvas);
    }

    pub(super) fn draw(&mut self, c: &mut Canvas) {
        let th = theme();
        let dpi = self.dpi;
        let geo = geo(dpi);
        c.clear(th.bg);
        c.text(geo.head, "Manage profiles", self.fonts.subtitle, th.text, TA_LEFT);
        let radius = scf(design::RADIUS_CARD, dpi);
        c.fill_rrect(geo.panel, radius, th.surface, 1.0);
        if self.rows.is_empty() {
            c.text(
                geo.list,
                "No profiles found",
                self.fonts.body,
                th.text_dim,
                TA_CENTER,
            );
        }
        for i in 0..self.rows.len() {
            let r = self.row_rect(i);
            if r.t >= geo.list.t && r.b <= geo.list.b {
                self.draw_row(c, i, r);
            }
        }
        if let Some(bar) = self.scroll_list().scrollbar(self.scroll) {
            draw_scrollbar(c, &th, &bar, self.drag.is_some(), dpi);
        }
        c.stroke_rrect(geo.panel, radius, scf(1.0, dpi).floor().max(1.0), th.line, 1.0);
        if let Some(s) = &self.status {
            c.text(geo.status, s, self.fonts.caption, th.text_dim, TA_LEFT);
        }
    }

    /// Name with its type tag, file name, usage (and why a locked row can't be deleted), Delete button.
    fn draw_row(&self, c: &mut Canvas, i: usize, r: R) {
        let th = theme();
        let dpi = self.dpi;
        let row = &self.rows[i];
        if self.hover == Some(i) {
            c.fill_rrect(r.inset(sc(1, dpi)), scf(6.0, dpi), th.hover_fill(), 0.6);
        }
        let button = self.button_rect(i);
        let left = r.l + sc(12, dpi);
        let right = button.l - sc(12, dpi);
        let line = |k: i32| R::new(left, r.t + sc(k, dpi), right, r.t + sc(k + 18, dpi));
        // line 1: name and type tag
        let tag = row.kind.label();
        let tag_w = c.text_width(tag, self.fonts.caption) + sc(12, dpi);
        let name_max = (right - left - tag_w - sc(8, dpi)).max(sc(40, dpi));
        let name_w = c.text_width(&row.name, self.fonts.body).min(name_max);
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
        let usage = match row.deletable {
            Err(why) => format!("{} \u{b7} {}", row.used, why.hint()),
            Ok(()) => row.used.clone(),
        };
        c.text(line(42), &usage, self.fonts.caption, th.text_dim, TA_LEFT);
        let (label, style) = if self.confirm == Some(i) {
            ("Click to confirm", BtnStyle::DangerConfirm)
        } else {
            ("Delete", BtnStyle::Danger)
        };
        let enabled = row.deletable.is_ok();
        let over = button.contains(self.mouse.0, self.mouse.1);
        let state = WState {
            hover: enabled && self.hover == Some(i) && over,
            pressed: enabled && self.pressed == Some(i),
            focus: false,
            enabled,
        };
        let ctx = Ctx {
            th,
            fonts: &self.fonts,
            dpi,
        };
        draw_button(c, &ctx, button, label, state, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_shows_the_last_result() {
        let mut p = Profiles::new(crate::showcase::profiles(), true, 96);
        let mut before = Canvas::new(super::super::CLIENT_W, super::super::CLIENT_H).unwrap();
        p.draw(&mut before);
        p.status = Some("Deleted Display P3 calibrated".into());
        let mut after = Canvas::new(super::super::CLIENT_W, super::super::CLIENT_H).unwrap();
        p.draw(&mut after);
        let g = geo(96);
        let w = super::super::CLIENT_W as usize;
        let row = |c: &mut Canvas| c.pixels()[g.status.cy() as usize * w..][..w].to_vec();
        assert_ne!(row(&mut before), row(&mut after));
    }
}
