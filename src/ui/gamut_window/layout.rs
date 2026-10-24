//! Gamut window geometry (physical px): the plot on the left, the scrolling checklist and the info block
//! on the right.

use super::super::draw::{sc, Canvas, R};
use super::super::widgets::ScrollList;
use super::{Gamut, Row, CLIENT_H, CLIENT_W};

#[derive(Clone, Copy, Debug)]
pub(super) struct Geo {
    pub plot: R,
    /// The list container (surface fill, border, rounded corners).
    pub panel: R,
    /// The list viewport inside the container's padding: a whole number of rows tall.
    pub list: R,
    /// The info block (name, detail, coverage), anchored to the bottom of the right panel.
    pub info: R,
    pub head_l: R,
    pub head_r: R,
}

/// The list never shrinks below this many rows.
const MIN_LIST_ROWS: i32 = 3;
const PANEL_L: i32 = 598;
/// Clear space between the list container and the info block.
const INFO_GAP: i32 = 12;
/// Inner padding of the list container.
const LIST_PAD: i32 = 4;
/// Every list row (entry or header) has the same height, so scrolling and the viewport stay on whole rows.
pub(super) const ROW_H: i32 = 30;
/// Info block height (96 dpi) until the first paint measures it.
pub(super) const INFO_H_INITIAL: i32 = 110;

/// `info_h` is the physical height of the info block; the list container fills the rest of the right
/// column. Whole rows are visible; the sub-row remainder is bottom padding inside the container.
pub(super) fn geo(dpi: u32, info_h: i32) -> Geo {
    let s = |v: i32| sc(v, dpi);
    let pad = s(LIST_PAD);
    let row = s(ROW_H);
    let info_b = s(CLIENT_H - 16);
    let info_t = info_b - info_h;
    let top = s(40);
    let bottom = info_t - s(INFO_GAP);
    let rows = ((bottom - top - 2 * pad) / row).max(MIN_LIST_ROWS);
    let panel = R::new(
        s(PANEL_L),
        top,
        s(CLIENT_W - 20),
        (top + 2 * pad + rows * row).max(bottom),
    );
    Geo {
        plot: R::new(s(56), s(40), s(566), s(580)),
        head_l: R::new(s(56), s(6), s(566), s(38)),
        head_r: R::new(s(PANEL_L), s(6), s(CLIENT_W - 20), s(38)),
        panel,
        list: R::new(
            panel.l + pad,
            panel.t + pad,
            panel.r - pad,
            panel.t + pad + rows * row,
        ),
        info: R::new(s(PANEL_L), info_t, s(CLIENT_W - 20), info_b),
    }
}

impl Gamut {
    pub(super) fn geo(&self) -> Geo {
        geo(self.dpi, self.info_h)
    }

    pub(super) fn row_h(&self) -> i32 {
        sc(ROW_H, self.dpi)
    }

    pub(super) fn content_h(&self) -> i32 {
        self.rows.len() as i32 * self.row_h()
    }

    /// The checklist's scrolling geometry.
    pub(super) fn scroll_list(&self) -> ScrollList {
        ScrollList {
            viewport: self.geo().list,
            content_h: self.content_h(),
            row_h: self.row_h(),
            dpi: self.dpi,
        }
    }

    pub(super) fn clamp_scroll(&mut self) {
        self.scroll = self.scroll_list().clamp(self.scroll);
    }

    /// Height of the info block: the tallest text it can show, so the list does not resize on hover.
    pub(super) fn info_height(&self, c: &mut Canvas, width: i32) -> i32 {
        let dpi = self.dpi;
        let caption = self.fonts.caption;
        let block = |c: &mut Canvas, body: &str| c.measure_wrapped(width, body, caption) + sc(18, dpi);
        let mut tallest = 0;
        for i in 0..self.entries.len() {
            let (_, body) = self.row_info(i);
            tallest = tallest.max(block(c, &body));
            if let Some((_, body)) = self.coverage_info(i) {
                tallest = tallest.max(block(c, &body));
            }
        }
        tallest + sc(16, dpi)
    }

    /// Entry index at a client point (headers and the scrollbar strip have none).
    pub(super) fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let list = self.scroll_list();
        let viewport = list.viewport;
        if !viewport.contains(x, y) || x >= viewport.r - list.bar_width() {
            return None;
        }
        let i = usize::try_from((y - viewport.t + self.scroll) / list.row_h).ok()?;
        match self.rows.get(i)? {
            Row::Entry(e) => Some(*e),
            Row::Header(_) => None,
        }
    }

    /// Moves the keyboard cursor `dir` entries up or down the list and scrolls it into view.
    pub(super) fn move_selection(&mut self, dir: i32) {
        let order = self.entry_order();
        if order.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|s| order.iter().position(|&i| i == s))
            .map(|p| p as i32);
        let next = match current {
            None => 0,
            Some(c) => (c + dir).clamp(0, order.len() as i32 - 1),
        } as usize;
        self.selected = Some(order[next]);
        self.keyboard = true;
        self.ensure_visible(order[next]);
    }

    /// Scrolls so the row of `entry` is fully visible.
    pub(super) fn ensure_visible(&mut self, entry: usize) {
        let (row, viewport_h) = (self.row_h(), self.geo().list.h());
        if let Some(pos) = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Entry(i) if *i == entry))
        {
            let top = pos as i32 * row;
            if top < self.scroll {
                self.scroll = top;
            } else if top + row > self.scroll + viewport_h {
                self.scroll = top + row - viewport_h;
            }
        }
        self.clamp_scroll();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_whole_rows_and_leaves_room_for_the_info_block() {
        for dpi in [96, 120, 144, 192] {
            for info in [60, 110, 200] {
                let g = geo(dpi, sc(info, dpi));
                assert_eq!(g.list.h() % sc(ROW_H, dpi), 0, "dpi {dpi}");
                assert!(g.list.h() >= MIN_LIST_ROWS * sc(ROW_H, dpi));
                assert!(g.panel.b <= g.info.t || g.list.h() == MIN_LIST_ROWS * sc(ROW_H, dpi));
                assert!(g.plot.r < g.panel.l, "plot and list side by side");
                assert!(g.panel.contains(g.list.l, g.list.t));
                assert_eq!(g.info.b, sc(CLIENT_H - 16, dpi));
            }
        }
    }

    #[test]
    fn a_taller_info_block_shortens_the_list() {
        let short = geo(96, 60);
        let tall = geo(96, 200);
        assert!(tall.list.h() < short.list.h());
        assert_eq!(tall.plot, short.plot);
    }
}
