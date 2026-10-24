//! Editing in the main window: slider values (raw while dragging, rounded when applied), activating
//! controls, and the actions they queue.

use super::super::widgets::{
    self, black_scale, full_scale, peak_scale, round_black, round_nits, round_sdr_white_nits, sat_scale,
    slider_t_at, white_scale, Scale, PEAK_MAX_NITS, PEAK_MIN_NITS,
};
use super::proc::{APPLY_INTERVAL_MS, TIMER_APPLY};
use super::{Id, Main, Pending};
use crate::app::UiAction;
use crate::icc_write::HdrParams;
use windows_sys::Win32::UI::WindowsAndMessaging::SetTimer;

impl Main {
    /// Pull HDR slider values from the snapshot unless the user is editing.
    pub(super) fn sync_hdr(&mut self) {
        if self.drag.is_some() || self.hdr_dirty || self.sdr_white_dirty {
            return;
        }
        if let Some(m) = self.mon() {
            let (h, white) = (m.hdr, m.sdr_white_nits);
            if let Some(n) = white {
                self.sdr_white = round_sdr_white_nits(n);
            }
            self.black = h.min_nits.clamp(0.0, widgets::BLACK_MAX_NITS);
            self.peak = h.peak_nits.clamp(PEAK_MIN_NITS, PEAK_MAX_NITS);
            self.full = h.full_frame_nits.clamp(PEAK_MIN_NITS, self.peak);
            self.sat = (h.saturation * 100.0).clamp(0.0, 100.0);
        }
    }

    pub(super) fn slider_scale(&self, id: Id) -> Scale {
        match id {
            Id::Black => black_scale(),
            Id::Peak => peak_scale(),
            Id::Full => full_scale(),
            Id::SdrWhite => white_scale(self.white_max()),
            _ => sat_scale(),
        }
    }

    pub(super) fn slider_value(&self, id: Id) -> f64 {
        match id {
            Id::Black => self.black,
            Id::Peak => self.peak,
            Id::Full => self.full,
            Id::SdrWhite => self.sdr_white,
            _ => self.sat,
        }
    }

    /// The slider's thumb position, 0..=1.
    pub(super) fn slider_t(&self, id: Id) -> f64 {
        self.slider_scale(id).to_t(self.slider_value(id))
    }

    pub(super) fn set_slider_t(&mut self, id: Id, t: f64) {
        let v = self.slider_scale(id).to_value(t);
        if id == Id::SdrWhite {
            self.sdr_white = round_sdr_white_nits(v).min(self.white_max());
            self.sdr_white_dirty = true;
            return;
        }
        match id {
            Id::Black => self.black = v,
            Id::Peak => (self.peak, self.full) = widgets::move_peak(v, self.full),
            Id::Full => self.full = widgets::move_full(v, self.peak),
            _ => self.sat = v,
        }
        self.hdr_dirty = true;
    }

    /// Peak as applied: 5-nit steps (10 above 1000).
    fn rounded_peak(&self) -> f64 {
        round_nits(self.peak, PEAK_MIN_NITS, PEAK_MAX_NITS)
    }

    /// Full-frame as applied: rounded like Peak and never above it.
    fn rounded_full(&self) -> f64 {
        round_nits(self.full, PEAK_MIN_NITS, self.rounded_peak())
    }

    /// The value column of a slider row.
    pub(super) fn slider_text(&self, id: Id) -> String {
        match id {
            Id::Black => format!("{:.2} nits", round_black(self.black)),
            Id::Peak => format!("{} nits", self.rounded_peak() as i64),
            Id::Full => format!("{} nits", self.rounded_full() as i64),
            Id::SdrWhite => format!("{} nits", self.sdr_white.round() as i64),
            _ => format!("{}%", self.sat.round() as i64),
        }
    }

    /// Top of the SDR content slider (the EDID cap unless the toggle is on).
    pub(super) fn white_max(&self) -> f64 {
        self.mon()
            .map(|m| m.sdr_max())
            .unwrap_or(crate::monitors::SDR_WHITE_MAX_NITS)
    }

    /// SDR content value of the monitor's peak, where the slider draws its tick mark.
    pub(super) fn white_target(&self) -> Option<f64> {
        widgets::sdr_peak_target(self.mon()?.effective_peak()?)
    }

    /// The HDR profile values the sliders show, rounded as applied.
    fn params(&self) -> HdrParams {
        HdrParams {
            min_nits: round_black(self.black),
            peak_nits: self.rounded_peak(),
            full_frame_nits: self.rounded_full(),
            saturation: self.sat.round().clamp(0.0, 100.0) / 100.0,
        }
    }

    fn send(&mut self, action: UiAction) {
        self.pending.push(Pending::Dispatch(action));
    }

    /// Queues whatever the sliders changed since the last commit.
    pub(super) fn commit(&mut self) {
        if self.sdr_white_dirty {
            self.sdr_white_dirty = false;
            self.send(UiAction::SetSdrWhite {
                mon: self.selected,
                nits: self.sdr_white,
            });
        }
        if !self.hdr_dirty {
            return;
        }
        self.hdr_dirty = false;
        let params = self.params();
        self.send(UiAction::ApplyHdr {
            mon: self.selected,
            params,
        });
    }

    pub(super) fn set_hover(&mut self, hover: Option<Id>) {
        if self.ui.hover != hover {
            self.ui.hover = hover;
            self.invalidate();
        }
    }

    /// A click (or Space / Enter) on `id`.
    pub(super) fn activate(&mut self, id: Id) {
        let mon = self.selected;
        match id {
            Id::Tile(i) => self.select_monitor(i),
            Id::Identify => self.send(UiAction::Identify),
            Id::Sdr => self.pending.push(Pending::OpenDropdown),
            Id::Read => self.send(UiAction::ReadEdid { mon }),
            Id::Compare => self.send(UiAction::OpenGamut),
            Id::Manage => self.send(UiAction::OpenProfiles),
            Id::HdrToggle => {
                let on = !self.mon().map(|m| m.hdr_on).unwrap_or(false);
                self.send(UiAction::SetHdrEnabled { mon, on });
            }
            Id::Reset => {
                self.hdr_dirty = false;
                self.sdr_white_dirty = false;
                self.send(UiAction::ResetHdr { mon });
            }
            Id::Advanced => {
                let on = !self.state.advanced;
                self.state.advanced = on;
                self.send(UiAction::SetAdvanced(on));
                self.pending.push(Pending::Resize);
                self.layout();
                self.invalidate();
            }
            Id::Autostart => self.send(UiAction::SetAutostart(!self.state.autostart)),
            Id::SdrCap => self.toggle_sdr_cap(),
            _ => {}
        }
    }

    /// Shows monitor `i`, committing the edits made on the previous one.
    fn select_monitor(&mut self, i: usize) {
        if i == self.selected {
            return;
        }
        self.commit();
        self.selected = i;
        self.hdr_dirty = false;
        self.sdr_white_dirty = false;
        self.sync_hdr();
        self.layout();
        self.invalidate();
    }

    /// "Allow clipping above the monitor peak brightness". Turning it off pulls SDR content down to the cap.
    fn toggle_sdr_cap(&mut self) {
        let mon = self.selected;
        // Pending slider edits go out first, so the snap-back below wins over them.
        self.commit();
        let on = !self.mon().is_some_and(|m| m.sdr_above_peak);
        if let Some(m) = self.state.monitors.get_mut(mon) {
            m.sdr_above_peak = on;
        }
        if let Some(cap) = self.mon().and_then(|m| m.sdr_cap()).filter(|_| !on) {
            self.sdr_white = self.sdr_white.min(cap as f64);
        }
        self.send(UiAction::SetSdrAbovePeak { mon, on });
        self.invalidate();
    }

    /// Moves slider `id` to the client x coordinate `x`.
    pub(super) fn slider_at_x(&mut self, id: Id, x: i32) {
        if let Some(w) = self.ui.get(id).copied() {
            let t = slider_t_at(w.rect, x, self.dpi);
            self.set_slider_t(id, t);
            self.invalidate();
        }
    }

    /// Commits the pending slider edit within `APPLY_INTERVAL_MS`. Unlike a debounce, a running timer is not
    /// re-armed, so a continuous drag still applies at a steady rate.
    pub(super) fn schedule_apply(&mut self) {
        if self.apply_armed || self.hwnd.is_null() {
            return;
        }
        self.apply_armed = true;
        // SAFETY: valid window handle.
        unsafe {
            SetTimer(self.hwnd, TIMER_APPLY, APPLY_INTERVAL_MS, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main() -> Main {
        let mut st = crate::showcase::state();
        st.advanced = true;
        Main::new(st, 96)
    }

    fn actions(main: &mut Main) -> Vec<String> {
        std::mem::take(&mut main.pending)
            .into_iter()
            .map(|p| match p {
                Pending::Dispatch(a) => format!("{a:?}"),
                Pending::Resize => "Resize".into(),
                Pending::OpenDropdown => "OpenDropdown".into(),
                Pending::Close => "Close".into(),
            })
            .collect()
    }

    #[test]
    fn slider_values_round_as_applied() {
        let mut m = main();
        m.black = 0.123;
        m.peak = 1234.0;
        m.full = 1500.0;
        m.sat = 49.6;
        assert_eq!(m.slider_text(Id::Black), "0.12 nits");
        assert_eq!(m.slider_text(Id::Peak), "1230 nits");
        assert_eq!(
            m.slider_text(Id::Full),
            "1230 nits",
            "full-frame stops at the peak"
        );
        assert_eq!(m.slider_text(Id::Sat), "50%");
        assert_eq!(m.slider_text(Id::SdrWhite), "240 nits");
        let p = m.params();
        assert_eq!(
            (p.min_nits, p.peak_nits, p.full_frame_nits, p.saturation),
            (0.12, 1230.0, 1230.0, 0.5)
        );
    }

    #[test]
    fn sdr_content_stays_under_the_cap() {
        let mut m = main();
        assert_eq!(m.white_max(), 452.0);
        m.set_slider_t(Id::SdrWhite, 1.0);
        assert_eq!(m.sdr_white, 452.0);
        assert!(m.sdr_white_dirty && !m.hdr_dirty);
        m.set_slider_t(Id::SdrWhite, 0.0);
        assert_eq!(m.sdr_white, 80.0);
        assert_eq!(m.white_target(), Some(448.0));
    }

    #[test]
    fn peak_drags_full_frame_down() {
        let mut m = main();
        m.set_slider_t(Id::Peak, 0.0);
        assert_eq!((m.peak, m.full), (100.0, 100.0));
        assert!(m.hdr_dirty);
        m.set_slider_t(Id::Full, 1.0);
        assert_eq!(m.full, 100.0, "full-frame can't pass the peak");
        assert!((m.slider_t(Id::Peak)).abs() < 1e-9);
    }

    #[test]
    fn commit_queues_the_changed_values_once() {
        let mut m = main();
        m.commit();
        assert!(actions(&mut m).is_empty());
        m.set_slider_t(Id::SdrWhite, 0.5);
        m.set_slider_t(Id::Sat, 0.5);
        m.commit();
        let sent = actions(&mut m);
        assert_eq!(sent.len(), 2);
        assert!(sent[0].starts_with("SetSdrWhite { mon: 0, nits: 2"), "{sent:?}");
        assert!(sent[1].starts_with("ApplyHdr { mon: 0,"), "{sent:?}");
        m.commit();
        assert!(actions(&mut m).is_empty());
    }

    #[test]
    fn edits_survive_a_refresh_until_committed() {
        let mut m = main();
        m.set_slider_t(Id::Peak, 1.0);
        m.sync_hdr();
        assert_eq!(m.peak, PEAK_MAX_NITS, "the snapshot doesn't overwrite an edit");
        m.commit();
        m.sync_hdr();
        assert_eq!(m.peak, 450.0);
    }

    #[test]
    fn activating_controls_queues_their_actions() {
        let mut m = main();
        m.activate(Id::Read);
        m.activate(Id::Sdr);
        m.activate(Id::HdrToggle);
        m.activate(Id::Autostart);
        assert_eq!(
            actions(&mut m),
            [
                "ReadEdid { mon: 0 }",
                "OpenDropdown",
                "SetHdrEnabled { mon: 0, on: false }",
                "SetAutostart(false)"
            ]
        );
        m.activate(Id::Advanced);
        assert!(!m.state.advanced && m.ui.get(Id::Peak).is_none());
        assert_eq!(actions(&mut m), ["SetAdvanced(false)", "Resize"]);
        m.activate(Id::Tile(0));
        assert!(actions(&mut m).is_empty(), "the shown monitor stays");
    }

    #[test]
    fn turning_clipping_off_pulls_sdr_content_to_the_cap() {
        let mut m = main();
        m.state.monitors[0].sdr_above_peak = true;
        m.sdr_white = 480.0;
        m.activate(Id::SdrCap);
        assert!(!m.state.monitors[0].sdr_above_peak);
        assert_eq!(m.sdr_white, 452.0);
        assert_eq!(actions(&mut m), ["SetSdrAbovePeak { mon: 0, on: false }"]);
    }

    #[test]
    fn reset_drops_unsent_edits() {
        let mut m = main();
        m.set_slider_t(Id::Black, 1.0);
        m.activate(Id::Reset);
        assert!(!m.hdr_dirty);
        assert_eq!(actions(&mut m), ["ResetHdr { mon: 0 }"]);
    }
}
