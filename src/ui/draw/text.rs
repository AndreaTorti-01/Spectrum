//! GDI text on a [`Canvas`] (ClearType onto the DIB): single-line, wrapped, and measuring.

use super::{colorref, Canvas, R};
use windows_sys::Win32::Foundation::{RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    DrawTextW, GetTextExtentPoint32W, SelectObject, SetTextColor, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, HFONT, HGDIOBJ,
};

/// Horizontal alignment for [`Canvas::text`].
pub const TA_LEFT: u32 = 0;
pub const TA_CENTER: u32 = 1;
pub const TA_RIGHT: u32 = 2;

/// `DrawTextW` flags for single-line text in a rect with `align`.
fn single_line_flags(align: u32) -> u32 {
    DT_VCENTER
        | DT_SINGLELINE
        | DT_NOPREFIX
        | DT_END_ELLIPSIS
        | match align {
            TA_CENTER => DT_CENTER,
            TA_RIGHT => DT_RIGHT,
            _ => 0,
        }
}

impl Canvas {
    /// Single-line text in `r`, vertically centered, with ellipsis. `align` is TA_*.
    pub fn text(&mut self, r: R, s: &str, font: HFONT, color: u32, align: u32) {
        self.draw_text(r, s, font, color, single_line_flags(align));
    }

    /// Word-wrapped text from the top of `r`.
    pub fn text_wrapped(&mut self, r: R, s: &str, font: HFONT, color: u32) {
        self.draw_text(r, s, font, color, DT_WORDBREAK | DT_NOPREFIX);
    }

    /// Height of `s` wrapped at `width`.
    pub fn measure_wrapped(&mut self, width: i32, s: &str, font: HFONT) -> i32 {
        let text: Vec<u16> = s.encode_utf16().collect();
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 0,
        };
        // SAFETY: valid DC, font and buffers.
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            DrawTextW(
                self.dc,
                text.as_ptr(),
                text.len() as i32,
                &mut rc,
                DT_WORDBREAK | DT_CALCRECT | DT_NOPREFIX,
            );
            SelectObject(self.dc, old);
        }
        rc.bottom - rc.top
    }

    fn draw_text(&mut self, r: R, s: &str, font: HFONT, color: u32, flags: u32) {
        if s.is_empty() {
            return;
        }
        let text: Vec<u16> = s.encode_utf16().collect();
        let mut rc = r.to_rect();
        // SAFETY: valid DC, font and buffers.
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            SetTextColor(self.dc, colorref(color));
            DrawTextW(self.dc, text.as_ptr(), text.len() as i32, &mut rc, flags);
            SelectObject(self.dc, old);
        }
        self.dirty_gdi = true;
    }

    pub fn text_width(&mut self, s: &str, font: HFONT) -> i32 {
        let text: Vec<u16> = s.encode_utf16().collect();
        let mut size = SIZE { cx: 0, cy: 0 };
        // SAFETY: valid DC, font and buffers.
        unsafe {
            let old = SelectObject(self.dc, font as HGDIOBJ);
            GetTextExtentPoint32W(self.dc, text.as_ptr(), text.len() as i32, &mut size);
            SelectObject(self.dc, old);
        }
        size.cx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_flags_match_the_win32_values() {
        // DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS, plus the alignment.
        assert_eq!(single_line_flags(TA_LEFT), 0x8824);
        assert_eq!(single_line_flags(TA_CENTER), 0x8825);
        assert_eq!(single_line_flags(TA_RIGHT), 0x8826);
    }

    #[test]
    fn text_is_measured_and_drawn() {
        let fonts = super::super::Fonts::new(96);
        let mut c = Canvas::new(200, 40).unwrap();
        c.clear(0xFFFFFF);
        let w = c.text_width("Spectrum", fonts.body);
        assert!(w > 20 && w < 200, "{w}");
        assert_eq!(c.text_width("", fonts.body), 0);
        let one = c.measure_wrapped(1000, "a b c", fonts.body);
        let wrapped = c.measure_wrapped(1, "a b c", fonts.body);
        assert!(wrapped > one, "{wrapped} > {one}");
        c.text(R::new(0, 0, 200, 40), "Spectrum", fonts.body, 0x000000, TA_LEFT);
        assert!(
            c.pixels().iter().any(|&p| p != 0xFFFFFF),
            "text reached the pixels"
        );
    }
}
