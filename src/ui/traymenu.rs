//! Replica of the tray menu for off-screen screenshots (`--render traymenu`, the demo). The real menu is a
//! Windows popup menu (`tray::show_menu`), which only Windows can draw. The items come from the same
//! `MenuMonitor` list; metrics and colors were measured on captures of the real menu at 100% (Windows 11,
//! build 26300).

use super::draw::{make_font_px, sc, scf, sd_rrect, Canvas, R, TA_LEFT};
use crate::tray::MenuMonitor;
use windows_sys::Win32::Graphics::Gdi::{DeleteObject, CLEARTYPE_QUALITY, HFONT, HGDIOBJ};

// Metrics at 96 dpi.
const BORDER: i32 = 1;
const PAD: i32 = 2;
const ITEM_H: i32 = 22;
const SEP_H: i32 = 9;
/// Separator line offset inside its item.
const SEP_LINE: i32 = 4;
const SEP_INSET: i32 = 9;
const TEXT_X: i32 = 38;
const RIGHT: i32 = 60;
const GLYPH_X: f32 = 17.0;
const HIGHLIGHT_INSET: i32 = 6;
const RADIUS: f32 = 4.0;
const FONT_PX: i32 = 12;

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    None,
    Radio,
    Check,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Text {
        label: String,
        mark: Mark,
        bold: bool,
        disabled: bool,
        submenu: bool,
    },
    Separator,
}

fn text(label: &str, mark: Mark, bold: bool, disabled: bool, submenu: bool) -> Item {
    Item::Text {
        label: label.to_string(),
        mark,
        bold,
        disabled,
        submenu,
    }
}

/// The items `tray::show_menu` builds for `monitors` (top level only; a submenu shows as its label).
pub fn items(monitors: &[MenuMonitor]) -> Vec<Item> {
    let mut out = Vec::new();
    if monitors.len() == 1 {
        let m = &monitors[0];
        for (i, (label, _)) in m.profiles.iter().take(crate::tray::MAX_PER_MONITOR).enumerate() {
            let mark = if m.current == Some(i) {
                Mark::Radio
            } else {
                Mark::None
            };
            out.push(text(label, mark, false, false, false));
        }
        if !m.profiles.is_empty() {
            out.push(Item::Separator);
        }
        let mark = if m.hdr_on { Mark::Check } else { Mark::None };
        out.push(text("HDR", mark, false, !m.hdr_supported, false));
        out.push(Item::Separator);
    } else if monitors.len() > 1 {
        for (i, m) in monitors.iter().enumerate().take(crate::tray::MAX_PER_MONITOR) {
            let label = if m.label.starts_with(|c: char| c.is_ascii_digit()) {
                m.label.clone()
            } else {
                format!("{} \u{b7} {}", i + 1, m.label)
            };
            out.push(text(&label, Mark::None, false, false, true));
        }
        out.push(Item::Separator);
    }
    out.push(text("Open Spectrum", Mark::None, true, false, false));
    out.push(text("Exit", Mark::None, false, false, false));
    out
}

/// Moves the radio mark to item `idx` (the demo picking a profile).
pub fn set_radio(items: &mut [Item], idx: usize) {
    for (i, it) in items.iter_mut().enumerate() {
        if let Item::Text { mark, .. } = it {
            if *mark == Mark::Radio || i == idx {
                *mark = if i == idx { Mark::Radio } else { Mark::None };
            }
        }
    }
}

struct Fonts {
    normal: HFONT,
    bold: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Fonts {
        let f = |w| make_font_px("Segoe UI", -sc(FONT_PX, dpi), w, CLEARTYPE_QUALITY);
        Fonts {
            normal: f(400),
            bold: f(700),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: fonts created by `make_font_px`, not selected anywhere.
        unsafe {
            DeleteObject(self.normal as HGDIOBJ);
            DeleteObject(self.bold as HGDIOBJ);
        }
    }
}

/// Menu size and item rectangles (menu pixels) at `dpi`.
pub fn layout(items: &[Item], dpi: u32) -> (i32, i32, Vec<R>) {
    let fonts = Fonts::new(dpi);
    let widest = Canvas::new(1, 1)
        .map(|mut c| {
            items
                .iter()
                .map(|it| match it {
                    Item::Text { label, bold, .. } => {
                        c.text_width(label, if *bold { fonts.bold } else { fonts.normal })
                    }
                    Item::Separator => 0,
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let w = sc(TEXT_X, dpi) + widest + sc(RIGHT, dpi);
    let mut y = sc(BORDER + PAD, dpi);
    let rects = items
        .iter()
        .map(|it| {
            let h = sc(if *it == Item::Separator { SEP_H } else { ITEM_H }, dpi);
            let r = R::new(sc(BORDER, dpi), y, w - sc(BORDER, dpi), y + h);
            y += h;
            r
        })
        .collect();
    (w, y + sc(PAD + BORDER, dpi), rects)
}

struct Colors {
    /// Around the items and behind separators.
    bg: u32,
    /// Behind text items (a shade lighter in dark mode).
    item: u32,
    border: u32,
    separator: u32,
    text: u32,
    disabled: u32,
    highlight: u32,
}

fn colors(dark: bool) -> Colors {
    if dark {
        Colors {
            bg: 0x2B2B2B,
            item: 0x2C2C2C,
            border: 0x3E3E3E,
            separator: 0x3E3E3E,
            text: 0xFFFFFF,
            disabled: 0x787878,
            highlight: 0x353535,
        }
    } else {
        Colors {
            bg: 0xF9F9F9,
            item: 0xF9F9F9,
            border: 0xE5E5E5,
            separator: 0xD7D7D7,
            text: 0x000000,
            disabled: 0xA0A0A0,
            highlight: 0xF0F0F0,
        }
    }
}

/// The menu with item `highlight` under the pointer. Pixels outside the rounded corners get `backdrop`.
pub fn render(
    items: &[Item],
    dark: bool,
    dpi: u32,
    highlight: Option<usize>,
    backdrop: u32,
) -> Option<Canvas> {
    let col = colors(dark);
    let fonts = Fonts::new(dpi);
    let (w, h, rects) = layout(items, dpi);
    let mut c = Canvas::new(w, h)?;
    c.clear(col.bg);
    for (i, (it, r)) in items.iter().zip(&rects).enumerate() {
        match it {
            Item::Separator => {
                let y = r.t + sc(SEP_LINE, dpi);
                c.fill_rect(
                    R::new(sc(SEP_INSET, dpi), y, w - sc(SEP_INSET, dpi), y + sc(1, dpi)),
                    col.separator,
                );
            }
            Item::Text {
                label,
                mark,
                bold,
                disabled,
                submenu,
            } => {
                let hr = R::new(sc(HIGHLIGHT_INSET, dpi), r.t, w - sc(HIGHLIGHT_INSET, dpi), r.b);
                c.fill_rect(hr, col.item);
                if highlight == Some(i) && !disabled {
                    c.fill_rrect(hr, scf(RADIUS, dpi), col.highlight, 1.0);
                }
                let fg = if *disabled { col.disabled } else { col.text };
                let (gx, gy) = (scf(GLYPH_X, dpi), r.cy());
                match mark {
                    Mark::Radio => c.fill_circle(gx, gy, scf(1.85, dpi), fg, 1.0),
                    Mark::Check => c.polyline(
                        &[
                            (gx - scf(4.0, dpi), gy + scf(0.5, dpi)),
                            (gx - scf(1.5, dpi), gy + scf(2.7, dpi)),
                            (gx + scf(2.9, dpi), gy - scf(2.0, dpi)),
                        ],
                        false,
                        scf(1.1, dpi),
                        fg,
                        1.0,
                    ),
                    Mark::None => {}
                }
                let font = if *bold { fonts.bold } else { fonts.normal };
                c.text(R::new(sc(TEXT_X, dpi), r.t, r.r, r.b), label, font, fg, TA_LEFT);
                if *submenu {
                    let (ax, ay) = (r.r as f32 - scf(14.0, dpi), r.cy());
                    let a = scf(3.0, dpi);
                    c.polyline(
                        &[(ax - a / 2.0, ay - a), (ax + a / 2.0, ay), (ax - a / 2.0, ay + a)],
                        false,
                        scf(1.0, dpi),
                        fg,
                        1.0,
                    );
                }
            }
        }
    }

    // Rounded corners and the border.
    let (wf, hf, rad) = (w as f32, h as f32, scf(RADIUS, dpi));
    let edge = rad.ceil() as i32 + 1;
    let px = c.pixels();
    for y in 0..h {
        for x in 0..w {
            if x >= edge && y >= edge && x < w - edge && y < h - edge {
                continue;
            }
            let d = sd_rrect(x as f32 + 0.5, y as f32 + 0.5, 0.0, 0.0, wf, hf, rad);
            let p = &mut px[(y * w + x) as usize];
            super::draw::blend_px(p, col.border, 1.0 - (-0.5 - d).clamp(0.0, 1.0));
            super::draw::blend_px(p, backdrop, 1.0 - (0.5 - d).clamp(0.0, 1.0));
        }
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn showcase_menu() -> Vec<MenuMonitor> {
        let st = crate::showcase::state();
        let m = &st.monitors[0];
        vec![MenuMonitor::from_ui(m)]
    }

    #[test]
    fn single_monitor_items_mirror_the_tray_menu() {
        let items = items(&showcase_menu());
        let labels: Vec<String> = items
            .iter()
            .map(|i| match i {
                Item::Text { label, .. } => label.clone(),
                Item::Separator => "-".into(),
            })
            .collect();
        assert_eq!(labels.first().map(String::as_str), Some("QD-OLED 27 (EDID)"));
        assert_eq!(
            &labels[labels.len() - 5..],
            ["-", "HDR", "-", "Open Spectrum", "Exit"]
        );
        assert!(matches!(
            &items[0],
            Item::Text {
                mark: Mark::Radio,
                ..
            }
        ));
    }

    #[test]
    fn set_radio_moves_the_dot() {
        let mut items = items(&showcase_menu());
        set_radio(&mut items, 2);
        let radios: Vec<usize> = (0..items.len())
            .filter(|&i| {
                matches!(
                    &items[i],
                    Item::Text {
                        mark: Mark::Radio,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(radios, vec![2]);
    }
}
