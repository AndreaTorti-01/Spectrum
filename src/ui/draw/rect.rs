//! [`R`], the integer rectangle every layout is built from.

use windows_sys::Win32::Foundation::RECT;

/// Left, top, right, bottom in pixels; right and bottom are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct R {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl R {
    pub fn new(l: i32, t: i32, r: i32, b: i32) -> R {
        R { l, t, r, b }
    }
    pub fn xywh(x: i32, y: i32, w: i32, h: i32) -> R {
        R {
            l: x,
            t: y,
            r: x + w,
            b: y + h,
        }
    }
    pub fn w(&self) -> i32 {
        self.r - self.l
    }
    pub fn h(&self) -> i32 {
        self.b - self.t
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    pub fn inset(&self, d: i32) -> R {
        R {
            l: self.l + d,
            t: self.t + d,
            r: self.r - d,
            b: self.b - d,
        }
    }
    pub fn offset(&self, dx: i32, dy: i32) -> R {
        R {
            l: self.l + dx,
            t: self.t + dy,
            r: self.r + dx,
            b: self.b + dy,
        }
    }
    pub fn cx(&self) -> f32 {
        (self.l + self.r) as f32 / 2.0
    }
    pub fn cy(&self) -> f32 {
        (self.t + self.b) as f32 / 2.0
    }
    pub(super) fn to_rect(self) -> RECT {
        RECT {
            left: self.l,
            top: self.t,
            right: self.r,
            bottom: self.b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_geometry() {
        let r = R::xywh(10, 20, 30, 40);
        assert_eq!(r, R::new(10, 20, 40, 60));
        assert_eq!((r.w(), r.h()), (30, 40));
        assert_eq!((r.cx(), r.cy()), (25.0, 40.0));
        assert!(r.contains(10, 20) && r.contains(39, 59));
        assert!(!r.contains(40, 20) && !r.contains(10, 60));
        assert_eq!(r.inset(5), R::new(15, 25, 35, 55));
        assert_eq!(r.inset(-2), R::new(8, 18, 42, 62));
        assert_eq!(r.offset(-10, 5), R::new(0, 25, 30, 65));
        let w = r.to_rect();
        assert_eq!((w.left, w.top, w.right, w.bottom), (10, 20, 40, 60));
    }
}
