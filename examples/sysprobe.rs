//! Dev-only probe for the system modules. `cargo run --example sysprobe` prints monitors, scope,
//! defaults and associations (read-only). `cargo run --example sysprobe -- roundtrip` additionally
//! installs a copy of the inbox sRGB profile as "Spectrum Test.icm", associates it (not default),
//! checks it appears, then removes it again. Never changes defaults or HDR.

use spectrum::{config, installer, monitors, util, wcs};

fn main() {
    let roundtrip = std::env::args().any(|a| a == "roundtrip");
    // edid::parse may still be todo!() in a partial checkout: probe without it, then try it guarded.
    let mons = monitors::enumerate_with(false);
    println!("color dir: {}", wcs::color_dir().display());
    println!("monitors: {}", mons.len());
    for m in &mons {
        println!("--- monitor {} ---", m.num);
        println!("  gdi_name      {}", m.gdi_name);
        println!("  rect          {:?}", m.rect);
        println!(
            "  adapter       {:?}  source {}  target {}",
            m.adapter, m.source_id, m.target_id
        );
        println!("  friendly      {}", m.friendly_name);
        println!("  device_path   {}", m.device_path);
        let raw = monitors::read_edid_registry(&m.device_path);
        println!("  edid bytes    {:?}", raw.as_ref().map(|r| r.len()));
        println!("  key (no edid) {}", m.key);
        println!(
            "  hdr           supported={} enabled={}",
            m.hdr_supported, m.hdr_enabled
        );
        println!("  color info    {}", monitors::color_info_debug(m));
        println!("  accent        {:#08x}", m.accent);
        println!("  user scope    current_user_active={:?}", wcs::is_user_scope(m));
        println!("  SDR default   {:?}", wcs::default_profile(m, false));
        println!("  HDR default   {:?}", wcs::default_profile(m, true));
        println!("  assoc (SDR)   {:?}", wcs::associated(m, false));
        println!("  assoc (HDR)   {:?}", wcs::associated(m, true));
        println!("  autostart     {}", installer::autostart_enabled());
    }
    println!("install_dir   {}", installer::install_dir().display());
    println!("running inst. {}", installer::is_running_installed());
    println!("backup_dir    {}", wcs::backup_dir().display());
    let cfg = config::Config::load();
    println!("config monitors: {}", cfg.monitors.len());
    let _ = util::app_data_dir();

    if let Some(m) = mons.first().filter(|_| roundtrip) {
        const TEST: &str = "Spectrum Test.icm";
        println!("=== round trip on monitor {} ===", m.num);
        if wcs::is_user_scope(m) != Some(true) {
            println!("user scope is not CURRENT_USER; skipping (would need ensure_user_scope)");
            return;
        }
        let before_default = wcs::default_profile(m, false);
        let before_list = wcs::associated(m, false);
        let bytes = wcs::read_profile(wcs::SRGB_INBOX).expect("inbox sRGB profile");
        println!("install: {:?}", wcs::install_profile(TEST, &bytes));
        println!(
            "exists: {}  list_installed has it: {}",
            wcs::profile_exists(TEST),
            wcs::list_installed().iter().any(|n| n == TEST)
        );
        println!("associate: {:?}", wcs::associate(m, TEST, false, false));
        let l = wcs::associated(m, false);
        println!(
            "associated now: {:?}  contains test: {}",
            l,
            l.iter().any(|n| n == TEST)
        );
        println!(
            "default unchanged: {}",
            wcs::default_profile(m, false) == before_default
        );
        println!("disassociate: {:?}", wcs::disassociate(m, TEST, false));
        let l = wcs::associated(m, false);
        println!(
            "associated after: {:?}  contains test: {}",
            l,
            l.iter().any(|n| n == TEST)
        );
        println!("delete: {:?}", wcs::delete_profile(TEST));
        println!(
            "exists after: {}  backup exists: {}",
            wcs::profile_exists(TEST),
            wcs::backup_dir().join(TEST).exists()
        );
        println!("list restored: {}", wcs::associated(m, false) == before_list);
    }
}
