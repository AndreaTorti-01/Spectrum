//! A Windows 11 window frame drawn in Spectrum's own code, for off-screen screenshots (`--render`): 1 px
//! border, rounded corners, caption with the app icon, the title and the minimize, maximize (disabled, the
//! windows are not resizable) and close glyphs. Colors and positions were measured on captures of the real
//! frame at 100% (Windows 11, build 26300).

use super::draw::{make_font_px, sc, scf, sd_rrect, Canvas, R, TA_LEFT};
use crate::icon_render::render_bgra;
use windows_sys::Win32::Graphics::Gdi::{DeleteObject, ANTIALIASED_QUALITY, HGDIOBJ};

/// Caption height below the top border, at 96 dpi.
const CAPTION_H: i32 = 25;
const BUTTON_W: i32 = 46;
const GLYPH: f32 = 10.0;
const CORNER: f32 = 8.0;

struct Colors {
    caption: u32,
    border: u32,
    text: u32,
}

fn colors(dark: bool, active: bool) -> Colors {
    match (dark, active) {
        (false, true) => Colors {
            caption: 0xEFF4F9,
            border: 0xA2A2A2,
            text: 0x000000,
        },
        (false, false) => Colors {
            caption: 0xF3F3F3,
            border: 0xA2A2A2,
            text: 0x929292,
        },
        (true, true) => Colors {
            caption: 0x1A2229,
            border: 0x3B3B3B,
            text: 0xFFFFFF,
        },
        (true, false) => Colors {
            caption: 0x202020,
            border: 0x3B3B3B,
            text: 0x797979,
        },
    }
}

/// Size of the frame around a `w` x `h` client area: (width, height, client x, client y).
pub fn outer(w: i32, h: i32, dpi: u32) -> (i32, i32, i32, i32) {
    let cap = sc(CAPTION_H, dpi);
    (w + 2, h + cap + 2, 1, 1 + cap)
}

/// `client` inside a window frame titled `title`. Pixels outside the rounded corners get `backdrop`.
pub fn framed(
    client: &mut Canvas,
    title: &str,
    dark: bool,
    active: bool,
    backdrop: u32,
    dpi: u32,
) -> Option<Canvas> {
    let col = colors(dark, active);
    let (w, h, cx, cy) = outer(client.width(), client.height(), dpi);
    let mut c = Canvas::new(w, h)?;
    c.clear(col.caption);
    c.draw_canvas(cx, cy, client);

    // Icon, title.
    let cap = R::new(1, 1, w - 1, cy);
    let isz = sc(16, dpi);
    let ix = sc(4, dpi);
    c.blit_premul_bgra(
        ix,
        cap.t + (cap.h() - isz) / 2,
        isz,
        isz,
        &render_bgra(isz as u32),
    );
    let font = make_font_px("Segoe UI", -sc(12, dpi), 400, ANTIALIASED_QUALITY);
    c.text(
        R::new(ix + isz + sc(6, dpi), cap.t, cap.r - 3 * sc(BUTTON_W, dpi), cap.b),
        title,
        font,
        col.text,
        TA_LEFT,
    );
    // SAFETY: font created above and no longer selected.
    unsafe { DeleteObject(font as HGDIOBJ) };

    // Caption buttons, right to left: close, maximize (disabled), minimize.
    let bw = sc(BUTTON_W, dpi);
    let g = scf(GLYPH, dpi);
    let mid = |i: i32| ((cap.r - bw * i - bw / 2) as f32, (cap.t + cap.h() / 2) as f32);
    let (x, y) = mid(0);
    let (x0, y0) = ((x - g / 2.0).round(), (y - g / 2.0).round());
    let lw = scf(1.0, dpi);
    c.line(
        x0 + 0.5,
        y0 + 0.5,
        x0 + g - 0.5,
        y0 + g - 0.5,
        lw * 1.1,
        col.text,
        0.92,
    );
    c.line(
        x0 + g - 0.5,
        y0 + 0.5,
        x0 + 0.5,
        y0 + g - 0.5,
        lw * 1.1,
        col.text,
        0.92,
    );
    let (x, y) = mid(1);
    let (x0, y0) = ((x - g / 2.0).round(), (y - g / 2.0).round());
    c.stroke_rrect_f(x0, y0, x0 + g, y0 + g, lw, lw, col.text, 0.2);
    let (x, y) = mid(2);
    let x0 = (x - g / 2.0).round() + scf(1.0, dpi).round();
    c.fill_rrect_f(x0, y.round(), x0 + g, y.round() + lw, 0.0, col.text, 1.0);

    // Rounded corners and the border.
    let (wf, hf, rad) = (w as f32, h as f32, scf(CORNER, dpi));
    let r = rad.ceil() as i32 + 1;
    let px = c.pixels();
    for y in 0..h {
        for x in 0..w {
            let edge = x < r || y < r || x >= w - r || y >= h - r;
            if !edge {
                continue;
            }
            // Coverage of the outer shape and of the shape inside the 1 px border.
            let d = sd_rrect(x as f32 + 0.5, y as f32 + 0.5, 0.0, 0.0, wf, hf, rad);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (-0.5 - d).clamp(0.0, 1.0);
            let p = &mut px[(y * w + x) as usize];
            let mut v = *p;
            super::draw::blend_px(&mut v, col.border, 1.0 - inner);
            super::draw::blend_px(&mut v, backdrop, 1.0 - outer);
            *p = v;
        }
    }
    Some(c)
}
