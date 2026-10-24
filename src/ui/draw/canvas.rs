//! The back buffer: a DIB section in a memory DC, with anti-aliased shapes rasterised from signed
//! distances and pixel-level blits. Text drawing is in `text.rs`.

#![allow(clippy::too_many_arguments, clippy::not_unsafe_ptr_arg_deref)]

use super::R;
use std::ffi::c_void;
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject, SetBkMode,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY, TRANSPARENT,
};

/// A top-down 32-bit DIB section. Pixels are `0x00RRGGBB` (or ARGB where the caller says so).
pub struct Dib {
    pub hbm: HBITMAP,
    pub bits: *mut u32,
    pub w: i32,
    pub h: i32,
}

impl Dib {
    pub fn new(w: i32, h: i32) -> Option<Dib> {
        if w <= 0 || h <= 0 {
            return None;
        }
        // SAFETY: standard DIB-section creation; bits pointer is valid while hbm lives.
        unsafe {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let hbm = CreateDIBSection(
                std::ptr::null_mut(),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if hbm.is_null() || bits.is_null() {
                return None;
            }
            Some(Dib {
                hbm,
                bits: bits as *mut u32,
                w,
                h,
            })
        }
    }

    pub fn pixels(&mut self) -> &mut [u32] {
        // SAFETY: the DIB owns w*h u32s at `bits`.
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: owned bitmap handle.
        unsafe {
            DeleteObject(self.hbm as HGDIOBJ);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------------------------

/// Shadow falloff for [`Canvas::shadow`]: (alpha at the edge, length in px at 96 dpi) beside, below and
/// above the rect.
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub side: (f32, f32),
    pub bottom: (f32, f32),
    pub top: (f32, f32),
}

/// Back buffer: a memory DC with a DIB selected. All painting happens here, then `present` blits it.
pub struct Canvas {
    pub dc: HDC,
    dib: Dib,
    old: HGDIOBJ,
    /// GDI drew text since the last flush (see `sync`).
    pub(super) dirty_gdi: bool,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Canvas> {
        let dib = Dib::new(w, h)?;
        // SAFETY: memory DC compatible with the screen.
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() {
                return None;
            }
            let old = SelectObject(dc, dib.hbm as HGDIOBJ);
            SetBkMode(dc, TRANSPARENT as i32);
            Some(Canvas {
                dc,
                dib,
                old,
                dirty_gdi: false,
            })
        }
    }

    pub fn width(&self) -> i32 {
        self.dib.w
    }
    pub fn height(&self) -> i32 {
        self.dib.h
    }

    /// Ensure GDI text drawn earlier is visible in the pixel memory.
    fn sync(&mut self) {
        if self.dirty_gdi {
            // SAFETY: no preconditions.
            unsafe {
                GdiFlush();
            }
            self.dirty_gdi = false;
        }
    }

    pub fn present(&mut self, dst: HDC, x: i32, y: i32) {
        self.sync();
        // SAFETY: both DCs valid.
        unsafe {
            BitBlt(dst, x, y, self.dib.w, self.dib.h, self.dc, 0, 0, SRCCOPY);
        }
    }

    /// The pixels (`0x00RRGGBB`, top-down), with any pending GDI text flushed in.
    pub fn pixels(&mut self) -> &mut [u32] {
        self.sync();
        self.dib.pixels()
    }

    /// The canvas as an opaque PNG file (off-screen screenshots).
    pub fn to_png(&mut self) -> Vec<u8> {
        let (w, h) = (self.dib.w as u32, self.dib.h as u32);
        let rgba: Vec<u8> = self
            .pixels()
            .iter()
            .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8, 0xFF])
            .collect();
        crate::icon_png::encode_rgba_wh(w, h, &rgba)
    }

    /// Copies `src` with its top-left corner at (x, y).
    pub fn draw_canvas(&mut self, x: i32, y: i32, src: &mut Canvas) {
        let (sw, sh) = (src.width(), src.height());
        let (cw, ch) = (self.dib.w, self.dib.h);
        let sp = src.pixels().to_vec();
        let px = self.pixels();
        for j in 0..sh {
            let dy = y + j;
            if dy < 0 || dy >= ch {
                continue;
            }
            for i in 0..sw {
                let dx = x + i;
                if dx >= 0 && dx < cw {
                    px[(dy * cw + dx) as usize] = sp[(j * sw + i) as usize];
                }
            }
        }
    }

    /// Soft shadow around `r`, like the one DWM draws under windows and menus: black whose alpha falls off
    /// as `a * exp(-d / len)` with the distance `d` from the rect, `(a, len)` blended by direction between
    /// the sides, the bottom (longer) and the top (shorter). Lengths are at 96 dpi. The rect is left alone.
    pub fn shadow(&mut self, r: R, dpi: u32, s: &Shadow) {
        let (l, t, rr, b) = (r.l as f32, r.t as f32, r.r as f32, r.b as f32);
        let k = dpi as f32 / 96.0;
        let reach = 6.0 * s.bottom.1.max(s.side.1).max(s.top.1) * k;
        let s = *s;
        self.paint_sdf(
            (l - reach, t - reach, rr + reach, b + reach),
            0,
            1.0,
            move |x, y| {
                let dx = (l - x).max(x - rr).max(0.0);
                let dy = (t - y).max(y - b).max(0.0);
                let d = (dx * dx + dy * dy).sqrt();
                if d <= 0.0 {
                    return 0.0;
                }
                let v = dy / d;
                let end = if y > b { s.bottom } else { s.top };
                let a = s.side.0 + (end.0 - s.side.0) * v;
                let len = (s.side.1 + (end.1 - s.side.1) * v) * k;
                a * (-(d / len)).exp()
            },
        );
    }

    /// Anti-aliased filled polygon (even-odd).
    pub fn fill_polygon(&mut self, pts: &[(f32, f32)], color: u32, alpha: f32) {
        if pts.len() < 3 {
            return;
        }
        let bbox = pts.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |b, p| {
            (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))
        });
        let pts = pts.to_vec();
        self.paint_sdf(bbox, color, alpha, move |x, y| {
            let mut d = f32::MAX;
            let mut inside = false;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                let (ex, ey) = (b.0 - a.0, b.1 - a.1);
                let t = (((x - a.0) * ex + (y - a.1) * ey) / (ex * ex + ey * ey).max(1e-6)).clamp(0.0, 1.0);
                d = d.min(((x - a.0 - t * ex).powi(2) + (y - a.1 - t * ey).powi(2)).sqrt());
                if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * ex {
                    inside = !inside;
                }
            }
            (0.5 + if inside { d } else { -d }).clamp(0.0, 1.0)
        });
    }

    pub fn clear(&mut self, color: u32) {
        self.sync();
        self.dib.pixels().fill(color & 0xFFFFFF);
    }

    pub fn fill_rect(&mut self, r: R, color: u32) {
        self.sync();
        let (w, h) = (self.dib.w, self.dib.h);
        let (l, t, rr, b) = (r.l.max(0), r.t.max(0), r.r.min(w), r.b.min(h));
        if l >= rr || t >= b {
            return;
        }
        let px = self.dib.pixels();
        for y in t..b {
            px[(y * w + l) as usize..(y * w + rr) as usize].fill(color & 0xFFFFFF);
        }
    }

    pub fn blend_rect(&mut self, r: R, color: u32, alpha: f32) {
        let (w, h) = (self.dib.w, self.dib.h);
        self.sync();
        let px = self.dib.pixels();
        for y in r.t.max(0)..r.b.min(h) {
            for x in r.l.max(0)..r.r.min(w) {
                blend_px(&mut px[(y * w + x) as usize], color, alpha);
            }
        }
    }

    /// Generic signed-distance painter: `cov(x, y)` returns coverage 0..1 for the pixel center.
    fn paint_sdf(
        &mut self,
        bbox: (f32, f32, f32, f32),
        color: u32,
        alpha: f32,
        cov: impl Fn(f32, f32) -> f32,
    ) {
        self.sync();
        let (w, h) = (self.dib.w, self.dib.h);
        let x0 = (bbox.0.floor() as i32 - 1).max(0);
        let y0 = (bbox.1.floor() as i32 - 1).max(0);
        let x1 = (bbox.2.ceil() as i32 + 1).min(w);
        let y1 = (bbox.3.ceil() as i32 + 1).min(h);
        let px = self.dib.pixels();
        for y in y0..y1 {
            for x in x0..x1 {
                let c = cov(x as f32 + 0.5, y as f32 + 0.5);
                if c > 0.0 {
                    blend_px(&mut px[(y * w + x) as usize], color, c * alpha);
                }
            }
        }
    }

    pub fn fill_rrect(&mut self, r: R, radius: f32, color: u32, alpha: f32) {
        self.fill_rrect_f(
            r.l as f32, r.t as f32, r.r as f32, r.b as f32, radius, color, alpha,
        );
    }

    pub fn fill_rrect_f(&mut self, l: f32, t: f32, r: f32, b: f32, radius: f32, color: u32, alpha: f32) {
        self.paint_sdf((l, t, r, b), color, alpha, move |x, y| {
            (0.5 - sd_rrect(x, y, l, t, r, b, radius)).clamp(0.0, 1.0)
        });
    }

    /// Border drawn inside the rect.
    pub fn stroke_rrect(&mut self, r: R, radius: f32, width: f32, color: u32, alpha: f32) {
        self.stroke_rrect_f(
            r.l as f32, r.t as f32, r.r as f32, r.b as f32, radius, width, color, alpha,
        );
    }

    pub fn stroke_rrect_f(
        &mut self,
        l: f32,
        t: f32,
        r: f32,
        b: f32,
        radius: f32,
        width: f32,
        color: u32,
        alpha: f32,
    ) {
        self.paint_sdf((l, t, r, b), color, alpha, move |x, y| {
            let d = sd_rrect(x, y, l, t, r, b, radius);
            (0.5 - ((d + width / 2.0).abs() - width / 2.0)).clamp(0.0, 1.0)
        });
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, rad: f32, color: u32, alpha: f32) {
        self.paint_sdf(
            (cx - rad, cy - rad, cx + rad, cy + rad),
            color,
            alpha,
            move |x, y| (0.5 - (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad)).clamp(0.0, 1.0),
        );
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, rad: f32, width: f32, color: u32, alpha: f32) {
        self.paint_sdf(
            (cx - rad, cy - rad, cx + rad, cy + rad),
            color,
            alpha,
            move |x, y| {
                let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad;
                (0.5 - ((d + width / 2.0).abs() - width / 2.0)).clamp(0.0, 1.0)
            },
        );
    }

    /// Anti-aliased line with round caps.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: u32, alpha: f32) {
        let hw = width / 2.0;
        let bbox = (x0.min(x1) - hw, y0.min(y1) - hw, x0.max(x1) + hw, y0.max(y1) + hw);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.paint_sdf(bbox, color, alpha, move |x, y| {
            let t = (((x - x0) * dx + (y - y0) * dy) / len2).clamp(0.0, 1.0);
            let (px, py) = (x0 + t * dx, y0 + t * dy);
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt() - hw;
            (0.5 - d).clamp(0.0, 1.0)
        });
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], closed: bool, width: f32, color: u32, alpha: f32) {
        for i in 0..pts.len().saturating_sub(1) {
            self.line(
                pts[i].0,
                pts[i].1,
                pts[i + 1].0,
                pts[i + 1].1,
                width,
                color,
                alpha,
            );
        }
        if closed && pts.len() > 2 {
            let (a, b) = (pts[pts.len() - 1], pts[0]);
            self.line(a.0, a.1, b.0, b.1, width, color, alpha);
        }
    }

    /// Composite premultiplied BGRA (top-down) at (x, y).
    pub fn blit_premul_bgra(&mut self, x: i32, y: i32, w: i32, h: i32, bgra: &[u8]) {
        self.sync();
        let (cw, ch) = (self.dib.w, self.dib.h);
        let px = self.dib.pixels();
        for j in 0..h {
            let dy = y + j;
            if dy < 0 || dy >= ch {
                continue;
            }
            for i in 0..w {
                let dx = x + i;
                if dx < 0 || dx >= cw {
                    continue;
                }
                let o = ((j * w + i) * 4) as usize;
                if o + 3 >= bgra.len() {
                    return;
                }
                let a = bgra[o + 3] as u32;
                if a == 0 {
                    continue;
                }
                let d = &mut px[(dy * cw + dx) as usize];
                let inv = 255 - a;
                let r = bgra[o + 2] as u32 + ((*d >> 16) & 0xFF) * inv / 255;
                let g = bgra[o + 1] as u32 + ((*d >> 8) & 0xFF) * inv / 255;
                let b = bgra[o] as u32 + (*d & 0xFF) * inv / 255;
                *d = (r.min(255) << 16) | (g.min(255) << 8) | b.min(255);
            }
        }
    }

    /// Rounds the canvas corners with `radius`: a 1 px `border` along the rounded outline, `backdrop`
    /// outside it. Only the edge strips are touched.
    pub fn round_corners(&mut self, radius: f32, border: u32, backdrop: u32) {
        let (w, h) = (self.dib.w, self.dib.h);
        let (wf, hf) = (w as f32, h as f32);
        let edge = radius.ceil() as i32 + 1;
        let px = self.pixels();
        for y in 0..h {
            for x in 0..w {
                if x >= edge && y >= edge && x < w - edge && y < h - edge {
                    continue;
                }
                // Coverage of the outer shape and of the shape inside the 1 px border.
                let d = sd_rrect(x as f32 + 0.5, y as f32 + 0.5, 0.0, 0.0, wf, hf, radius);
                let p = &mut px[(y * w + x) as usize];
                blend_px(p, border, 1.0 - (-0.5 - d).clamp(0.0, 1.0));
                blend_px(p, backdrop, 1.0 - (0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// Composite an ARGB source (alpha byte 0 = transparent, otherwise opaque) at (x, y).
    pub fn blit_masked(&mut self, x: i32, y: i32, src: &mut Dib) {
        self.sync();
        let (cw, ch) = (self.dib.w, self.dib.h);
        let (sw, sh) = (src.w, src.h);
        let sp = src.pixels();
        let px = self.dib.pixels();
        for j in 0..sh {
            let dy = y + j;
            if dy < 0 || dy >= ch {
                continue;
            }
            for i in 0..sw {
                let dx = x + i;
                if dx < 0 || dx >= cw {
                    continue;
                }
                let s = sp[(j * sw + i) as usize];
                if s >> 24 != 0 {
                    px[(dy * cw + dx) as usize] = s & 0xFFFFFF;
                }
            }
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: restore the original bitmap, then free the DC (the DIB drops after).
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteDC(self.dc);
        }
    }
}

/// Signed distance to a rounded rect (negative inside).
pub fn sd_rrect(x: f32, y: f32, l: f32, t: f32, r: f32, b: f32, radius: f32) -> f32 {
    let radius = radius.min((r - l) / 2.0).min((b - t) / 2.0).max(0.0);
    let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
    let (hx, hy) = ((r - l) / 2.0 - radius, (b - t) / 2.0 - radius);
    let (qx, qy) = ((x - cx).abs() - hx, (y - cy).abs() - hy);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

#[inline]
pub fn blend_px(d: &mut u32, c: u32, a: f32) {
    if a <= 0.0 {
        return;
    }
    let a = a.min(1.0);
    let ch = |s: u32| {
        let dv = ((*d >> s) & 0xFF) as f32;
        let cv = ((c >> s) & 0xFF) as f32;
        ((dv + (cv - dv) * a) + 0.5) as u32 & 0xFF
    };
    *d = (ch(16) << 16) | (ch(8) << 8) | ch(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrect_sdf_signs() {
        assert!(sd_rrect(50.0, 50.0, 0.0, 0.0, 100.0, 100.0, 8.0) < 0.0);
        assert!(sd_rrect(150.0, 50.0, 0.0, 0.0, 100.0, 100.0, 8.0) > 0.0);
        // corner pixel is outside a rounded rect
        assert!(sd_rrect(0.5, 0.5, 0.0, 0.0, 100.0, 100.0, 12.0) > 0.0);
        // the distance to a flat edge is exact
        assert!((sd_rrect(110.0, 50.0, 0.0, 0.0, 100.0, 100.0, 8.0) - 10.0).abs() < 1e-4);
    }

    #[test]
    fn blending_covers_the_endpoints() {
        let mut p = 0x000000;
        blend_px(&mut p, 0xFFFFFF, 0.0);
        assert_eq!(p, 0x000000);
        blend_px(&mut p, 0xFF8000, 1.0);
        assert_eq!(p, 0xFF8000);
        let mut p = 0x000000;
        blend_px(&mut p, 0xFFFFFF, 0.5);
        assert_eq!(p, 0x808080);
        let mut p = 0x102030;
        blend_px(&mut p, 0x405060, 7.0);
        assert_eq!(p, 0x405060, "alpha above 1 clamps");
    }

    #[test]
    fn rect_fills_clip_to_the_canvas() {
        let mut c = Canvas::new(4, 3).unwrap();
        assert_eq!((c.width(), c.height()), (4, 3));
        c.clear(0xFF112233);
        assert!(c.pixels().iter().all(|&p| p == 0x112233), "alpha byte dropped");
        c.fill_rect(R::new(-5, 1, 2, 9), 0xABCDEF);
        let px = c.pixels().to_vec();
        assert_eq!(&px[0..4], [0x112233; 4]);
        assert_eq!(&px[4..8], [0xABCDEF, 0xABCDEF, 0x112233, 0x112233]);
        c.blend_rect(R::new(3, 0, 10, 1), 0xFFFFFF, 1.0);
        assert_eq!(c.pixels()[3], 0xFFFFFF);
    }

    #[test]
    fn canvases_copy_with_clipping() {
        let mut src = Canvas::new(2, 2).unwrap();
        src.clear(0x00FF00);
        let mut dst = Canvas::new(3, 3).unwrap();
        dst.clear(0);
        dst.draw_canvas(2, -1, &mut src);
        let px = dst.pixels().to_vec();
        assert_eq!(px, [0, 0, 0x00FF00, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn masked_blit_skips_transparent_pixels() {
        let mut dib = Dib::new(2, 1).unwrap();
        dib.pixels().copy_from_slice(&[0x00FF_FFFF, 0xFF12_3456]);
        let mut c = Canvas::new(2, 1).unwrap();
        c.clear(0x000001);
        c.blit_masked(0, 0, &mut dib);
        assert_eq!(c.pixels(), [0x000001, 0x123456]);
        assert!(Dib::new(0, 5).is_none());
    }

    #[test]
    fn shapes_are_anti_aliased() {
        let mut c = Canvas::new(20, 20).unwrap();
        c.clear(0x000000);
        c.fill_circle(10.0, 10.0, 5.25, 0xFFFFFF, 1.0);
        let px = c.pixels().to_vec();
        assert_eq!(px[10 * 20 + 10], 0xFFFFFF, "center");
        assert_eq!(px[0], 0x000000, "outside");
        let edge = px[10 * 20 + 15] & 0xFF;
        assert!(edge > 0 && edge < 0xFF, "edge pixel is partly covered: {edge:#x}");
    }
}
