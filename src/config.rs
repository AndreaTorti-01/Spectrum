//! Tiny INI persistence for per-monitor choices in `%APPDATA%\Spectrum\spectrum.ini`.

use crate::icc_write::HdrParams;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorCfg {
    pub name: String,
    pub edid_profile: Option<String>,
    pub sdr: Option<String>,
    pub hdr: Option<HdrParams>,
    pub hdr_file: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub monitors: BTreeMap<String, MonitorCfg>,
}

impl Config {
    pub fn load() -> Config {
        todo!("config::Config::load")
    }

    pub fn save(&self) {
        todo!("config::Config::save")
    }

    pub fn monitor_mut(&mut self, _key: &str) -> &mut MonitorCfg {
        todo!("config::Config::monitor_mut")
    }

    pub fn parse(_text: &str) -> Config {
        todo!("config::Config::parse")
    }

    pub fn serialize(&self) -> String {
        todo!("config::Config::serialize")
    }
}
