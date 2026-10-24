//! The retained widget list of a window: hit-testing and hover, pressed and focus state by id.

use super::super::draw::R;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    Toggle,
    Checkbox,
    Slider,
    Dropdown,
    /// Non-interactive-looking hit area drawn by the owner (monitor tiles).
    Area,
}

#[derive(Clone, Copy, Debug)]
pub struct Widget<I> {
    pub id: I,
    pub kind: Kind,
    pub rect: R,
    pub enabled: bool,
    pub focusable: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WState {
    pub hover: bool,
    pub pressed: bool,
    pub focus: bool,
    pub enabled: bool,
}

pub struct WidgetList<I: Copy + PartialEq> {
    items: Vec<Widget<I>>,
    pub hover: Option<I>,
    pub pressed: Option<I>,
    pub focus: Option<I>,
    pub focus_visible: bool,
}

impl<I: Copy + PartialEq> Default for WidgetList<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Copy + PartialEq> WidgetList<I> {
    pub fn new() -> Self {
        WidgetList {
            items: Vec::new(),
            hover: None,
            pressed: None,
            focus: None,
            focus_visible: false,
        }
    }

    /// Drops all widgets (interaction state is kept by id).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn add(&mut self, id: I, kind: Kind, rect: R, enabled: bool, focusable: bool) {
        self.items.push(Widget {
            id,
            kind,
            rect,
            enabled,
            focusable,
        });
    }

    pub fn get(&self, id: I) -> Option<&Widget<I>> {
        self.items.iter().find(|w| w.id == id)
    }

    /// Topmost widget at the point (later additions win).
    pub fn hit(&self, x: i32, y: i32) -> Option<I> {
        self.items
            .iter()
            .rev()
            .find(|w| w.rect.contains(x, y))
            .map(|w| w.id)
    }

    pub fn state(&self, id: I) -> WState {
        WState {
            hover: self.hover == Some(id),
            pressed: self.pressed == Some(id),
            focus: self.focus == Some(id) && self.focus_visible,
            enabled: self.get(id).map(|w| w.enabled).unwrap_or(false),
        }
    }

    /// Moves keyboard focus to the next/previous enabled focusable widget (wraps).
    pub fn focus_next(&mut self, backwards: bool) -> Option<I> {
        let order: Vec<I> = self
            .items
            .iter()
            .filter(|w| w.focusable && w.enabled)
            .map(|w| w.id)
            .collect();
        if order.is_empty() {
            self.focus = None;
            return None;
        }
        let cur = self.focus.and_then(|f| order.iter().position(|&i| i == f));
        let next = match (cur, backwards) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
        self.focus_visible = true;
        self.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_list_hit_and_focus() {
        let mut w: WidgetList<u8> = WidgetList::new();
        w.add(1, Kind::Button, R::xywh(0, 0, 10, 10), true, true);
        w.add(2, Kind::Button, R::xywh(20, 0, 10, 10), false, true);
        w.add(3, Kind::Slider, R::xywh(40, 0, 10, 10), true, true);
        assert_eq!(w.hit(5, 5), Some(1));
        assert_eq!(w.hit(25, 5), Some(2));
        assert_eq!(w.hit(500, 5), None);
        assert_eq!(w.focus_next(false), Some(1));
        assert_eq!(w.focus_next(false), Some(3)); // 2 is disabled
        assert_eq!(w.focus_next(false), Some(1));
        assert_eq!(w.focus_next(true), Some(3));
    }

    #[test]
    fn state_reflects_the_interaction_by_id() {
        let mut w: WidgetList<u8> = WidgetList::new();
        w.add(1, Kind::Toggle, R::xywh(0, 0, 10, 10), true, true);
        w.add(2, Kind::Area, R::xywh(0, 0, 10, 10), false, false);
        // Later additions win the hit test.
        assert_eq!(w.hit(5, 5), Some(2));
        w.hover = Some(1);
        w.pressed = Some(1);
        w.focus = Some(1);
        let s = w.state(1);
        assert!(s.hover && s.pressed && s.enabled);
        assert!(!s.focus, "focus is only drawn once it is visible");
        w.focus_visible = true;
        assert!(w.state(1).focus);
        assert!(!w.state(2).enabled);
        assert!(!w.state(9).enabled, "unknown ids are disabled");
        // Clearing drops the widgets but keeps the interaction state.
        w.clear();
        assert!(w.get(1).is_none());
        assert_eq!(w.hover, Some(1));
    }

    #[test]
    fn focus_wraps_and_handles_empty_lists() {
        let mut w: WidgetList<u8> = WidgetList::default();
        assert_eq!(w.focus_next(false), None);
        w.add(1, Kind::Button, R::xywh(0, 0, 1, 1), true, true);
        w.add(2, Kind::Checkbox, R::xywh(0, 0, 1, 1), true, false);
        w.add(3, Kind::Dropdown, R::xywh(0, 0, 1, 1), true, true);
        assert_eq!(
            w.focus_next(true),
            Some(3),
            "backwards from nothing starts at the end"
        );
        assert_eq!(w.focus_next(true), Some(1));
        assert_eq!(w.focus_next(true), Some(3), "wraps; 2 is not focusable");
        assert!(w.focus_visible);
    }
}
