//! A list that scrolls by whole rows, with a thin scrollbar at the right edge of its viewport (the gamut
//! and profiles windows).

use super::super::draw::{mix, sc, scf, Canvas, Theme, R};

/// Width reserved at the right edge of the viewport for the scrollbar, at 96 dpi.
const SCROLLBAR_W: i32 = 12;

/// The geometry of a scrolling list, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct ScrollList {
    /// The visible part of the list.
    pub viewport: R,
    /// Height of all rows together.
    pub content_h: i32,
    /// Scrolling snaps to multiples of this.
    pub row_h: i32,
    pub dpi: u32,
}

/// Where the scrollbar is drawn for a scroll offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scrollbar {
    pub track: R,
    pub thumb_top: i32,
    pub thumb_len: i32,
    pub max_scroll: i32,
}

impl Scrollbar {
    pub fn thumb(&self) -> R {
        R::new(
            self.track.l,
            self.thumb_top,
            self.track.r,
            self.thumb_top + self.thumb_len,
        )
    }
}

/// What a mouse press on the scrollbar does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// On the thumb: drag it, keeping this offset between the pointer and the thumb top.
    Thumb(i32),
    /// On the track: page to this scroll offset.
    Page(i32),
}

impl ScrollList {
    pub fn overflows(&self) -> bool {
        self.content_h > self.viewport.h()
    }

    /// Width the rows leave free for the scrollbar (0 when the list fits).
    pub fn bar_width(&self) -> i32 {
        if self.overflows() {
            sc(SCROLLBAR_W, self.dpi)
        } else {
            0
        }
    }

    /// `scroll` snapped to a whole row and kept inside the content.
    pub fn clamp(&self, scroll: i32) -> i32 {
        let max = (self.content_h - self.viewport.h()).max(0);
        let snapped = (scroll + self.row_h / 2) / self.row_h * self.row_h;
        snapped.clamp(0, max)
    }

    /// The scrollbar at `scroll`; `None` when the list fits.
    pub fn scrollbar(&self, scroll: i32) -> Option<Scrollbar> {
        let (list, content, dpi) = (self.viewport, self.content_h, self.dpi);
        if content <= list.h() {
            return None;
        }
        let max_scroll = content - list.h();
        let tx = list.r - sc(7, dpi);
        let track = R::new(tx, list.t + sc(2, dpi), tx + sc(4, dpi), list.b - sc(2, dpi));
        let len = ((track.h() as f32 * list.h() as f32 / content as f32).max(scf(24.0, dpi))) as i32;
        let top = track.t + ((track.h() - len) as f32 * scroll as f32 / max_scroll as f32) as i32;
        Some(Scrollbar {
            track,
            thumb_top: top,
            thumb_len: len,
            max_scroll,
        })
    }

    /// A press at (x, y) with the list at `scroll`: on the thumb, on the track (pages by the visible rows),
    /// or `None` outside the scrollbar's strip.
    pub fn press(&self, scroll: i32, x: i32, y: i32) -> Option<Press> {
        let bar = self.scrollbar(scroll)?;
        let list = self.viewport;
        let zone = R::new(list.r - sc(SCROLLBAR_W, self.dpi), list.t, list.r, list.b);
        if !zone.contains(x, y) {
            return None;
        }
        let (top, len) = (bar.thumb_top, bar.thumb_len);
        if y >= top && y < top + len {
            return Some(Press::Thumb(y - top));
        }
        let page = list.h() / self.row_h * self.row_h;
        Some(Press::Page(
            self.clamp(scroll + if y < top { -page } else { page }),
        ))
    }

    /// The scroll offset that puts the thumb top at `thumb_top`, snapped to whole rows.
    pub fn drag_to(&self, scroll: i32, thumb_top: i32) -> Option<i32> {
        let bar = self.scrollbar(scroll)?;
        let span = (bar.track.h() - bar.thumb_len).max(1);
        let f = ((thumb_top - bar.track.t) as f32 / span as f32).clamp(0.0, 1.0);
        Some(self.clamp((f * bar.max_scroll as f32).round() as i32))
    }
}

/// The track stays visible while the list overflows; the thumb is solid while dragged.
pub fn draw_scrollbar(c: &mut Canvas, th: &Theme, bar: &Scrollbar, dragging: bool, dpi: u32) {
    c.fill_rrect(bar.track, scf(2.0, dpi), mix(th.surface, th.text_dim, 0.22), 1.0);
    let alpha = if dragging { 1.0 } else { 0.85 };
    c.fill_rrect(bar.thumb(), scf(2.0, dpi), th.text_dim, alpha);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten 30 px rows in a 90 px viewport at 96 dpi.
    fn list() -> ScrollList {
        ScrollList {
            viewport: R::new(0, 100, 200, 190),
            content_h: 300,
            row_h: 30,
            dpi: 96,
        }
    }

    #[test]
    fn a_list_that_fits_has_no_scrollbar() {
        let fits = ScrollList {
            content_h: 90,
            ..list()
        };
        assert!(!fits.overflows());
        assert_eq!(fits.bar_width(), 0);
        assert_eq!(fits.scrollbar(0), None);
        assert_eq!(fits.press(0, 195, 120), None);
        assert_eq!(fits.clamp(50), 0);
    }

    #[test]
    fn scrolling_snaps_to_whole_rows() {
        let l = list();
        assert_eq!(l.clamp(14), 0);
        assert_eq!(l.clamp(15), 30);
        assert_eq!(l.clamp(-40), 0);
        assert_eq!(l.clamp(1000), 210);
    }

    #[test]
    fn thumb_moves_along_the_track() {
        let l = list();
        assert_eq!(l.bar_width(), 12);
        let top = l.scrollbar(0).unwrap();
        assert_eq!(top.track, R::new(193, 102, 197, 188));
        assert_eq!((top.thumb_top, top.thumb_len, top.max_scroll), (102, 25, 210));
        let bottom = l.scrollbar(210).unwrap();
        assert_eq!(bottom.thumb().b, top.track.b);
    }

    #[test]
    fn presses_drag_the_thumb_or_page() {
        let l = list();
        assert_eq!(l.press(0, 100, 110), None, "outside the scrollbar strip");
        assert_eq!(l.press(0, 195, 110), Some(Press::Thumb(8)));
        assert_eq!(l.press(0, 195, 180), Some(Press::Page(90)));
        assert_eq!(l.press(210, 195, 105), Some(Press::Page(120)));
        assert_eq!(l.drag_to(0, 102), Some(0));
        assert_eq!(l.drag_to(0, 1000), Some(210));
        assert_eq!(l.drag_to(0, 132), Some(90));
    }
}
