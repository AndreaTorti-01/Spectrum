//! Spectrum: a tiny Windows tray utility that builds and applies ICC / HDR (MHC2) color profiles from EDID.
//! The library holds every module so `cargo test` covers them; `main.rs` is a thin CLI shell.

pub mod app;
pub mod color;
pub mod config;
pub mod debug;
pub mod edid;
pub mod icc_read;
pub mod icc_write;
pub mod icon;
pub mod icon_render;
pub mod installer;
pub mod monitors;
pub mod tray;
pub mod ui;
pub mod util;
pub mod wcs;
