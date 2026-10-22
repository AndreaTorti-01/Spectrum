#![windows_subsystem = "windows"]
//! UI preview harness: opens Spectrum's windows against a realistic mock data provider so they can be
//! looked at (and screenshotted) without the app/monitor/WCS layers.
//!
//! Usage: `uipreview <main|gamut|identify|tray|icon> [--theme dark|light] [--hdr on|off|none]
//!         [--mons 1|2] [--secs N] [--out file.bmp]`

use spectrum::app::{HdrProfile, UiMonitor, UiState};
use spectrum::edid::HdrStatic;
use spectrum::icc_write::HdrParams;
use spectrum::monitors::Rect;
use spectrum::tray::{MenuMonitor, TrayCmd};
use spectrum::ui::{self, draw};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, FindWindowW, GetMessageW, GetSystemMetrics, SetCursorPos, SetTimer,
    TranslateMessage, MSG, SM_CXSCREEN, SM_CYSCREEN, WS_POPUP,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn mock_state(args: &[String]) -> UiState {
    let hdr_mode = arg(args, "--hdr").unwrap_or_else(|| "on".into());
    let mons = arg(args, "--mons")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(2);
    let opts = vec![
        (
            "MAG 271QPX E2 (from EDID)".to_string(),
            "Spectrum MAG.icm".to_string(),
        ),
        ("sRGB".to_string(), "sRGB Color Space Profile.icm".to_string()),
        ("Display P3".to_string(), "Spectrum Display P3.icm".to_string()),
        (
            "Adobe RGB".to_string(),
            "Spectrum Adobe RGB compatible.icm".to_string(),
        ),
        ("Vendor sRGB mode".to_string(), "VENDOR_sRGB_mode.icm".to_string()),
    ];
    let m1 = UiMonitor {
        num: 1,
        rect: Rect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        },
        accent: 0xFF5A3C,
        name: "MAG 271QPX E2".into(),
        width: 2560,
        height: 1440,
        hdr_supported: hdr_mode != "none",
        hdr_on: hdr_mode == "on",
        sdr_options: opts.clone(),
        sdr_current: Some(0),
        has_edid: true,
        hdr: HdrParams {
            min_nits: 0.0,
            peak_nits: 1000.0,
            full_frame_nits: 400.0,
            saturation: 0.0,
        },
        hdr_custom_active: true,
        sdr_white_nits: Some(240.0),
        sdr_above_peak: false,
        hdr_profile: HdrProfile::Spectrum,
        reported: None,
        edid_hdr: Some(HdrStatic {
            max_nits: Some(455.0),
            max_frame_avg_nits: Some(254.0),
            min_nits: Some(0.0),
            pq: true,
        }),
    };
    let m2 = UiMonitor {
        num: 2,
        rect: Rect {
            left: 2560,
            top: 180,
            right: 4480,
            bottom: 1260,
        },
        accent: 0x2EC4B6,
        name: "DELL U2415".into(),
        width: 1920,
        height: 1080,
        hdr_supported: false,
        hdr_on: false,
        sdr_options: opts[1..].to_vec(),
        sdr_current: Some(0),
        has_edid: true,
        hdr: HdrParams {
            min_nits: 0.0,
            peak_nits: 1000.0,
            full_frame_nits: 400.0,
            saturation: 0.0,
        },
        hdr_custom_active: false,
        sdr_white_nits: Some(120.0),
        sdr_above_peak: false,
        hdr_profile: HdrProfile::None,
        reported: None,
        edid_hdr: None,
    };
    UiState {
        monitors: if mons == 1 { vec![m1] } else { vec![m1, m2] },
        autostart: true,
        advanced: true,
        status: Some("Applied MAG 271QPX E2 (from EDID)".into()),
    }
}

fn pump(secs: u32, mut done: impl FnMut() -> bool) {
    // SAFETY: plain message loop on this thread.
    unsafe {
        if secs > 0 {
            SetTimer(std::ptr::null_mut(), 0, secs * 1000, None);
        }
        let mut msg: MSG = std::mem::zeroed();
        let started = std::time::Instant::now();
        loop {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if r <= 0 {
                eprintln!("GetMessage returned {r} msg {:#x}", msg.message);
                break;
            }
            if msg.message == 0x0113 /* WM_TIMER */ && msg.hwnd.is_null() {
                if started.elapsed().as_secs() < secs as u64 {
                    continue;
                }
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if done() {
                break;
            }
        }
    }
}

fn gamut_open() -> bool {
    // SAFETY: class lookup only.
    unsafe { !FindWindowW(wide("SpectrumGamutWindow").as_ptr(), std::ptr::null()).is_null() }
}

fn write_bmp(path: &str, w: usize, h: usize, rgb: &[u8]) {
    let row = (w * 3).div_ceil(4) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + row * h) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        for x in 0..w {
            let o = (y * w + x) * 3;
            out.extend_from_slice(&[rgb[o + 2], rgb[o + 1], rgb[o]]);
        }
        out.resize(out.len() + row - w * 3, 0);
    }
    std::fs::write(path, out).unwrap();
}

/// Icon sheet: sizes 16/24/32/48 shown 1:1 and enlarged (nearest) on a dark and a light background.
fn icon_sheet(path: &str) {
    let sizes = [(16usize, 12usize), (20, 10), (24, 8), (32, 6), (48, 4)];
    let tile_w = 48 * 4 + 20;
    let (w, h) = (tile_w * sizes.len() + 20, 2 * (48 * 4 + 60));
    let mut img = vec![0u8; w * h * 3];
    let bgs = [[0x20u8, 0x20, 0x20], [0xF3u8, 0xF3, 0xF3]];
    for (row, bg) in bgs.iter().enumerate() {
        let y0 = row * (h / 2);
        for y in y0..y0 + h / 2 {
            for x in 0..w {
                img[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(bg);
            }
        }
        for (i, &(s, zoom)) in sizes.iter().enumerate() {
            let px = spectrum::icon_render::render_bgra(s as u32);
            let ox = 20 + i * tile_w;
            let put = |img: &mut Vec<u8>, x: usize, y: usize, p: &[u8]| {
                let a = p[3] as u32;
                let o = (y * w + x) * 3;
                for c in 0..3 {
                    let src = p[2 - c] as u32;
                    img[o + c] = (src + img[o + c] as u32 * (255 - a) / 255).min(255) as u8;
                }
            };
            // 1:1 sample
            for y in 0..s {
                for x in 0..s {
                    let p = &px[(y * s + x) * 4..(y * s + x) * 4 + 4];
                    put(&mut img, ox + x, y0 + 10 + y, p);
                }
            }
            // enlarged
            for y in 0..s * zoom {
                for x in 0..s * zoom {
                    let (sx, sy) = (x / zoom, y / zoom);
                    let p = &px[(sy * s + sx) * 4..(sy * s + sx) * 4 + 4];
                    let (dx, dy) = (ox + x, y0 + 60 + y);
                    if dx < w && dy < y0 + h / 2 {
                        put(&mut img, dx, dy, p);
                    }
                }
            }
        }
    }
    write_bmp(path, w, h, &img);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().cloned().unwrap_or_else(|| "main".into());
    // SAFETY: process-wide DPI mode, set before any window exists.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    match arg(&args, "--theme").as_deref() {
        Some("dark") => draw::set_forced_theme(Some(true)),
        Some("light") => draw::set_forced_theme(Some(false)),
        _ => {}
    }
    let secs = arg(&args, "--secs").and_then(|v| v.parse().ok()).unwrap_or(0u32);
    match mode.as_str() {
        "icon" => {
            icon_sheet(&arg(&args, "--out").unwrap_or_else(|| "icons.bmp".into()));
        }
        "main" => {
            ui::set_mock(Some(mock_state(&args)));
            ui::main_window::open();
            pump(secs, || !ui::main_window::is_open());
        }
        "gamut" => {
            ui::set_mock(Some(mock_state(&args)));
            ui::gamut_window::open(Some("Spectrum MAG 271QPX E2 EDID.icm".into()), 0);
            pump(secs, || !gamut_open());
        }
        "identify" => {
            let st = mock_state(&args);
            // SAFETY: plain metric queries.
            let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
            let r = Rect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
            ui::overlay::identify_all(&[(r, 1, st.monitors[0].accent)]);
            pump(secs.max(4), || false);
        }
        "tray" => {
            // SAFETY: a throwaway popup window as the menu owner.
            let hwnd: HWND = unsafe {
                SetCursorPos(600, 400);
                CreateWindowExW(
                    0,
                    wide("STATIC").as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                )
            };
            let st = mock_state(&args);
            let menu: Vec<MenuMonitor> = st
                .monitors
                .iter()
                .map(|m| MenuMonitor {
                    label: format!("{} \u{b7} {}", m.num, m.name),
                    profiles: m.sdr_options.clone(),
                    current: m.sdr_current,
                    hdr_supported: m.hdr_supported,
                    hdr_on: m.hdr_on,
                })
                .collect();
            let cmd: Option<TrayCmd> = spectrum::tray::show_menu(hwnd, &menu);
            println!("cmd: {cmd:?}");
        }
        _ => eprintln!("unknown mode {mode}"),
    }
}
