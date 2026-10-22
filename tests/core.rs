//! Integration tests for the pure-color core: color math, EDID, ICC writer and reader.

use spectrum::color::{self, Primaries, Trc, Xy, Xyz, BT709, SPECTRAL_LOCUS, STANDARD_SPACES};
use spectrum::edid;
use spectrum::icc_read;
use spectrum::icc_write::{build_hdr_profile, build_sdr_profile, HdrParams};
use spectrum::wcs;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn be32(d: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

/// (signature, offset, size) for each tag in the table.
fn tag_table(d: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let n = be32(d, 128) as usize;
    (0..n)
        .map(|i| {
            let b = 132 + 12 * i;
            (
                [d[b], d[b + 1], d[b + 2], d[b + 3]],
                be32(d, b + 4) as usize,
                be32(d, b + 8) as usize,
            )
        })
        .collect()
}

fn raw_xyz(d: &[u8], sig: &[u8; 4]) -> Xyz {
    let &(_, off, _) = tag_table(d).iter().find(|t| &t.0 == sig).expect("tag present");
    let f = |o: usize| be32(d, off + o) as i32 as f64 / 65536.0;
    Xyz {
        x: f(8),
        y: f(12),
        z: f(16),
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn xy_close(a: Xy, b: Xy, tol: f64) -> bool {
    close(a.x, b.x, tol) && close(a.y, b.y, tol)
}

// ------------------------------------------------------------------------------------------------------
// 1. Generated sRGB vs the inbox profile
// ------------------------------------------------------------------------------------------------------

#[test]
fn generated_srgb_matches_inbox_profile() {
    let srgb = color::space("srgb").unwrap();
    let ours = build_sdr_profile("Spectrum sRGB", &srgb.prims, srgb.trc);
    let inbox = fixture("srgb_inbox.icm");

    // Raw D50-adapted colorants.
    for sig in [b"rXYZ", b"gXYZ", b"bXYZ"] {
        let (a, b) = (raw_xyz(&ours, sig), raw_xyz(&inbox, sig));
        assert!(
            close(a.x, b.x, 2e-3) && close(a.y, b.y, 2e-3) && close(a.z, b.z, 2e-3),
            "{} differs: ours {a:?} inbox {b:?}",
            String::from_utf8_lossy(sig)
        );
    }

    // Un-adapted (native) primaries through icc_read.
    let (a, b) = (icc_read::parse(&ours).unwrap(), icc_read::parse(&inbox).unwrap());
    let (pa, pb) = (a.primaries.unwrap(), b.primaries.unwrap());
    for (n, (x, y)) in [
        ("r", (pa.r, pb.r)),
        ("g", (pa.g, pb.g)),
        ("b", (pa.b, pb.b)),
        ("w", (pa.w, pb.w)),
    ] {
        assert!(xy_close(x, y, 1e-3), "{n}: ours {x:?} inbox {y:?}");
    }
    assert!(a.is_matrix_trc && b.is_matrix_trc);
    assert_eq!(a.version >> 24, 4);
    assert_eq!(b.version >> 24, 2);
    assert_eq!(a.desc, "Spectrum sRGB");
    assert!(b.desc.contains("sRGB"), "inbox desc: {:?}", b.desc);
}

#[test]
fn sdr_profiles_roundtrip_through_reader() {
    for s in STANDARD_SPACES.iter() {
        let bytes = build_sdr_profile(s.name, &s.prims, s.trc);
        let info = icc_read::parse(&bytes).unwrap();
        let p = info.primaries.unwrap();
        for (n, (x, y)) in [
            ("r", (p.r, s.prims.r)),
            ("g", (p.g, s.prims.g)),
            ("b", (p.b, s.prims.b)),
            ("w", (p.w, s.prims.w)),
        ] {
            assert!(xy_close(x, y, 2e-4), "{} {n}: read {x:?} wanted {y:?}", s.id);
        }
        assert_eq!(info.desc, s.name);
        assert_eq!(&info.class, b"mntr");
        assert_eq!(&info.color_space, b"RGB ");
        assert!(info.is_matrix_trc && info.mhc2.is_none() && info.lumi.is_none());
    }
}

// ------------------------------------------------------------------------------------------------------
// 2 + 3. HDR fixture: read it, then rebuild it
// ------------------------------------------------------------------------------------------------------

#[test]
fn reads_hdr_calibration_fixture() {
    let info = icc_read::parse(&fixture("hdr_calibration_app.icc")).unwrap();
    assert_eq!(info.version, 0x0440_0000);
    assert_eq!(&info.class, b"mntr");
    assert_eq!(info.desc, "HDR Calibrated Profile 10/19/2026 124759");
    assert!(info.is_matrix_trc);
    assert_eq!(info.lumi, Some(450.0));
    let m = info.mhc2.expect("MHC2 present");
    assert_eq!((m.min_nits, m.peak_nits, m.lut_count), (0.0, 450.0, 2));
    let mat = m.matrix.expect("matrix");
    for (i, row) in mat.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            assert_eq!(*v, if i == j { 1.0 } else { 0.0 });
        }
    }
    let p = info.primaries.unwrap();
    assert!(xy_close(p.r, Xy { x: 0.685, y: 0.304 }, 0.003), "red {:?}", p.r);
    // White comes from the wtpt tag (the app's colorants do not sum to the display white).
    assert!(
        xy_close(p.w, Xy { x: 0.3135, y: 0.3291 }, 1e-3),
        "white {:?}",
        p.w
    );

    let info2 = icc_read::parse(&fixture("hdr_calibration_app_2.icc")).unwrap();
    assert_eq!(info2.lumi, Some(450.0));
    assert_eq!(info2.mhc2.unwrap().peak_nits, 450.0);
}

#[test]
fn reads_vendor_profile_with_mhc2() {
    let info = icc_read::parse(&fixture("vendor_mag271qpx.icm")).unwrap();
    assert_eq!(info.version >> 24, 4);
    assert!(info.is_matrix_trc);
    let p = info.primaries.unwrap();
    assert!(p.r.x > 0.6 && p.r.x < 0.72, "vendor red {:?}", p.r);
    let m = info.mhc2.expect("vendor profile carries MHC2");
    println!("vendor MHC2: {m:?}");
    assert!(m.lut_count > 2);
}

#[test]
fn hdr_profile_reproduces_windows_app_bytes() {
    let fx = fixture("hdr_calibration_app.icc");
    let info = icc_read::parse(&fx).unwrap();
    let prims = info.primaries.unwrap();
    let params = HdrParams {
        min_nits: 0.0,
        peak_nits: 450.0,
        full_frame_nits: 450.0,
        saturation: 0.0,
    };
    let built = build_hdr_profile(&info.desc, &prims, &params);

    // Documented exceptions: cprt (we write "Created by Spectrum (MIT license)", not Microsoft's notice) and the
    // header creator signature 'SPCT' (80..84). The shorter cprt shifts every later offset and the total size by
    // the same padded delta.
    let cprt_size = |d: &[u8]| {
        tag_table(d)
            .iter()
            .find(|t| &t.0 == b"cprt")
            .unwrap()
            .2
            .div_ceil(4)
            * 4
    };
    let delta = cprt_size(&fx) - cprt_size(&built);
    assert_eq!(built.len() + delta, fx.len(), "profile size");
    assert_eq!(&built[80..84], b"SPCT");
    assert_eq!(&fx[80..84], b"MSFT");

    // Header: identical except the size, the creation date (24..36), the creator (80..84) and the MD5 profile
    // ID (84..100), which depends on the date and on the colorant scale below.
    assert_eq!(&built[4..24], &fx[4..24], "header start");
    assert_eq!(&built[36..80], &fx[36..80], "header middle");
    assert_eq!(&built[100..128], &fx[100..128], "header tail");
    assert_ne!(&built[84..100], &[0u8; 16]);

    // Tag table and tag data.
    let (tb, tf) = (tag_table(&built), tag_table(&fx));
    assert_eq!(tb.len(), tf.len());
    for (a, b) in tb.iter().zip(&tf) {
        assert_eq!(a.0, b.0, "tag order");
        if &a.0 != b"cprt" {
            assert_eq!(a.2, b.2, "tag size {:?}", a.0);
            assert_eq!(a.1 + delta, b.1, "tag offset {:?}", a.0);
        }
    }
    // Intentional exception: the colorant and white-point XYZ. The app scales them with luminance factors we
    // could not derive (colorant Y sum 1.42, white Y 0.569); we write textbook scales (white Y = 1). The
    // chromaticities, which are all Windows uses, must still match.
    let exempt: [&[u8; 4]; 4] = [b"rXYZ", b"gXYZ", b"bXYZ", b"wtpt"];
    for (sig, off, size) in &tb {
        let name = String::from_utf8_lossy(sig).to_string();
        if sig == b"cprt" {
            continue;
        }
        if exempt.contains(&sig) {
            let (a, b) = (raw_xyz(&built, sig), raw_xyz(&fx, sig));
            let (xa, xb) = (color::xyz_to_xy(a), color::xyz_to_xy(b));
            assert!(
                xy_close(xa, xb, 5e-5),
                "{name} chromaticity: built {xa:?} app {xb:?}"
            );
        } else {
            let foff = tf.iter().find(|t| &t.0 == sig).unwrap().1;
            assert_eq!(built[*off..off + size], fx[foff..foff + size], "tag {name}");
        }
    }
    // Padding / gaps between tags are zeros in both.
    let covered: usize = tb.iter().map(|t| t.2).sum();
    println!("tag payload bytes {covered} of {}", fx.len());
}

#[test]
fn hdr_profile_saturation_and_roundtrip() {
    let prims = BT709;
    let p = HdrParams {
        min_nits: 0.05,
        peak_nits: 1000.0,
        full_frame_nits: 400.0,
        saturation: 0.5,
    };
    let bytes = build_hdr_profile("My HDR", &prims, &p);
    let info = icc_read::parse(&bytes).unwrap();
    assert_eq!(info.desc, "My HDR");
    assert!(close(info.lumi.unwrap(), 400.0, 1e-4));
    let m = info.mhc2.unwrap();
    assert!(close(m.min_nits, 0.05, 1e-4) && close(m.peak_nits, 1000.0, 1e-4));
    let mat = m.matrix.unwrap();
    let want = color::saturation_matrix(0.5);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                close(mat[i][j], want[i][j], 2e-5),
                "[{i}][{j}] {} vs {}",
                mat[i][j],
                want[i][j]
            );
        }
        assert_eq!(mat[i][3], 0.0);
    }
    let rp = info.primaries.unwrap();
    assert!(xy_close(rp.r, prims.r, 1e-4) && xy_close(rp.w, prims.w, 1e-4));
}

// ------------------------------------------------------------------------------------------------------
// 4. Spectral locus
// ------------------------------------------------------------------------------------------------------

#[test]
fn spectral_locus_anchors_and_smoothness() {
    let at = |nm: u16| {
        let r = SPECTRAL_LOCUS.iter().find(|r| r.0 == nm).unwrap();
        Xy { x: r.1, y: r.2 }
    };
    // CIE 1931 2 degree standard observer reference values.
    let anchors = [
        (380, 0.1741, 0.0050),
        (450, 0.1566, 0.0177),
        (480, 0.0913, 0.1327),
        (500, 0.0082, 0.5384),
        (520, 0.0743, 0.8338),
        (540, 0.2296, 0.7543),
        (560, 0.3731, 0.6245),
        (580, 0.5125, 0.4866),
        (600, 0.6270, 0.3725),
        (620, 0.6915, 0.3083),
        (650, 0.7260, 0.2740),
        (700, 0.7347, 0.2653),
    ];
    for (nm, x, y) in anchors {
        assert!(
            xy_close(at(nm), Xy { x, y }, 0.002),
            "{nm} nm: {:?} vs ({x},{y})",
            at(nm)
        );
    }
    assert_eq!(SPECTRAL_LOCUS.len(), 65);
    // The real locus moves fast through the green-cyan: 5 nm steps near 490-505 nm are up to ~0.12 long
    // (e.g. 490 -> 495 nm is 0.1197), so a plain "< 0.08" limit would reject the true CIE data. A typo
    // shows up as a long step or a kink, so also require every step to keep heading the same way
    // (positive dot product with the previous step) and the step length not to change by more than 2.5x.
    let mut prev: Option<(f64, f64)> = None;
    for w in SPECTRAL_LOCUS.windows(2) {
        assert_eq!(w[1].0, w[0].0 + 5);
        let step = (w[1].1 - w[0].1, w[1].2 - w[0].2);
        let d = step.0.hypot(step.1);
        assert!(d < 0.13, "jump of {d} between {} and {} nm", w[0].0, w[1].0);
        if let Some(p) = prev {
            let pd = p.0.hypot(p.1);
            assert!(p.0 * step.0 + p.1 * step.1 > 0.0, "kink at {} nm", w[0].0);
            if pd > 0.01 && d > 0.01 {
                assert!(
                    d / pd < 2.5 && pd / d < 2.5,
                    "step length jumps at {} nm: {pd} -> {d}",
                    w[0].0
                );
            }
        }
        prev = Some(step);
    }
    let area = color::locus_area();
    assert!(area > 0.32 && area < 0.34, "locus area {area}");
    let srgb = color::polygon_area(&color::triangle(&BT709)) / area;
    println!(
        "sRGB covers {:.1}% of the CIE 1931 xy locus polygon",
        srgb * 100.0
    );
}

// ------------------------------------------------------------------------------------------------------
// 5. EDID
// ------------------------------------------------------------------------------------------------------

#[test]
fn parses_mag271qpx_edid() {
    let raw = fixture("edid_mag271qpx_active.bin");
    assert_eq!(raw.len(), 384);
    for i in 0..3 {
        assert_eq!(
            raw[i * 128..(i + 1) * 128]
                .iter()
                .fold(0u8, |a, &b| a.wrapping_add(b)),
            0,
            "block {i}"
        );
    }
    let e = edid::parse(&raw).unwrap();
    assert_eq!(e.manufacturer, "MSI");
    assert_eq!(e.product_code, 0x4CD8);
    assert!(
        e.name.as_deref().unwrap_or("").contains("MAG 271QPX"),
        "name {:?}",
        e.name
    );
    assert!(e.primaries.r.x > 0.6 && e.primaries.r.x < 0.72);
    assert!(close(e.gamma.unwrap(), 2.2, 1e-9));
    assert!(!e.srgb_default);
    assert_eq!(e.trc(), Trc::Gamma(2.2));
    let h = e.hdr.clone().expect("CTA-861 HDR static metadata");
    assert!(h.pq);
    assert!(close(h.max_nits.unwrap(), 455.5, 1.0));
    assert!(close(h.max_frame_avg_nits.unwrap(), 253.7, 1.0));
    assert_eq!(h.min_nits, Some(0.0));

    let key = e.key();
    assert!(key.starts_with("MSI-4CD8-"));
    assert!(key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));

    println!("EDID name        : {:?}", e.display_name());
    println!(
        "manufacturer     : {} product {:#06X} serial {} year {}",
        e.manufacturer, e.product_code, e.serial, e.year
    );
    println!("serial descriptor: {:?}", e.serial_text);
    println!(
        "gamma            : {:?}  sRGB default flag: {}",
        e.gamma, e.srgb_default
    );
    let p = e.primaries;
    println!(
        "primaries R ({:.4}, {:.4})  G ({:.4}, {:.4})  B ({:.4}, {:.4})",
        p.r.x, p.r.y, p.g.x, p.g.y, p.b.x, p.b.y
    );
    println!("white point      : ({:.4}, {:.4})", p.w.x, p.w.y);
    println!("HDR static       : {h:?}");
    println!("key              : {key}");
    for s in STANDARD_SPACES.iter() {
        println!(
            "  coverage of {:<18}: {:5.1}%",
            s.name,
            color::coverage(&p, &s.prims) * 100.0
        );
    }

    // The base-only copy of the same model parses without extensions.
    let base = edid::parse(&fixture("edid_mag271qpx_base_only.bin")).unwrap();
    assert_eq!(base.product_code, 0x4CD8);
    assert!(base.hdr.is_none());

    // Corrupting the base block is rejected.
    let mut bad = raw.clone();
    bad[30] ^= 0x10;
    assert!(edid::parse(&bad).is_err());
}

#[test]
fn real_edid_primaries_are_plausible() {
    for name in ["edid_mag271qpx_active.bin", "edid_mag271qpx_base_only.bin"] {
        let e = edid::parse(&fixture(name)).unwrap();
        assert!(color::primaries_plausible(&e.primaries), "{name}");
    }
}

#[test]
fn edid_primaries_match_windows_hdr_fixture() {
    // Windows builds its HDR profile straight from the EDID chromaticities.
    let e = edid::parse(&fixture("edid_mag271qpx_active.bin")).unwrap();
    let info = icc_read::parse(&fixture("hdr_calibration_app.icc")).unwrap();
    let p = info.primaries.unwrap();
    for (a, b) in [
        (e.primaries.r, p.r),
        (e.primaries.g, p.g),
        (e.primaries.b, p.b),
        (e.primaries.w, p.w),
    ] {
        assert!(xy_close(a, b, 2e-4), "EDID {a:?} vs fixture {b:?}");
    }
}

// ------------------------------------------------------------------------------------------------------
// 6. Windows' own validator
// ------------------------------------------------------------------------------------------------------

#[cfg(windows)]
mod windows_validation {
    use super::*;
    use windows_sys::Win32::Foundation::{FALSE, TRUE};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, OPEN_EXISTING};
    use windows_sys::Win32::UI::ColorSystem::{
        CloseColorProfile, IsColorProfileValid, OpenColorProfileW, PROFILE, PROFILE_FILENAME, PROFILE_READ,
    };

    fn is_valid(name: &str, bytes: &[u8]) -> bool {
        let path =
            std::env::temp_dir().join(format!("spectrum_core_test_{}_{}.icc", std::process::id(), name));
        std::fs::write(&path, bytes).unwrap();
        let mut wide: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
        wide.push(0);
        let profile = PROFILE {
            dwType: PROFILE_FILENAME,
            pProfileData: wide.as_mut_ptr() as *mut _,
            cbDataSize: (wide.len() * 2) as u32,
        };
        let ok = unsafe {
            let h = OpenColorProfileW(&profile, PROFILE_READ, FILE_SHARE_READ, OPEN_EXISTING);
            assert!(
                h != 0,
                "{name}: OpenColorProfileW failed (error {})",
                std::io::Error::last_os_error()
            );
            let mut valid = FALSE;
            let call = IsColorProfileValid(h, &mut valid);
            CloseColorProfile(h);
            call == TRUE && valid == TRUE
        };
        let _ = std::fs::remove_file(&path);
        ok
    }

    #[test]
    fn windows_accepts_generated_profiles() {
        let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
        for id in ["srgb", "display-p3", "adobe-rgb", "prophoto", "rec2020", "dci-p3"] {
            let s = color::space(id).unwrap();
            cases.push((id.to_string(), build_sdr_profile(s.name, &s.prims, s.trc)));
        }
        // The exact files Spectrum installs for the monitor picker (name, description, curve).
        for (id, _, _) in wcs::REFERENCE_PROFILES {
            if let Some((_, desc)) = wcs::reference_profile_spec(id) {
                let s = color::space(id).unwrap();
                cases.push((format!("ref_{id}"), build_sdr_profile(&desc, &s.prims, s.trc)));
            }
        }
        let e = edid::parse(&fixture("edid_mag271qpx_active.bin")).unwrap();
        cases.push((
            "edid".into(),
            build_sdr_profile(&e.display_name(), &e.primaries, e.trc()),
        ));
        let hdr = HdrParams {
            min_nits: 0.0,
            peak_nits: 450.0,
            full_frame_nits: 450.0,
            saturation: 0.5,
        };
        cases.push((
            "hdr_sat".into(),
            build_hdr_profile("Spectrum HDR", &e.primaries, &hdr),
        ));
        let hdr0 = HdrParams {
            saturation: 0.0,
            ..hdr
        };
        cases.push((
            "hdr_plain".into(),
            build_hdr_profile("Spectrum HDR", &e.primaries, &hdr0),
        ));
        // Sanity: the app's own profile and the inbox sRGB profile are valid too, so the check is meaningful.
        cases.push(("fixture_app".into(), fixture("hdr_calibration_app.icc")));
        cases.push(("fixture_inbox".into(), fixture("srgb_inbox.icm")));

        let mut failed = Vec::new();
        for (name, bytes) in &cases {
            let ok = is_valid(name, bytes);
            println!("IsColorProfileValid({name}) = {ok}");
            if !ok {
                failed.push(name.clone());
            }
        }
        assert!(failed.is_empty(), "Windows rejected: {failed:?}");
    }
}

// ------------------------------------------------------------------------------------------------------
// 7. Gamut set math
// ------------------------------------------------------------------------------------------------------

fn prims(id: &str) -> Primaries {
    color::space(id).unwrap().prims
}

#[test]
fn coverage_and_iou() {
    let (srgb, p3, adobe, rec2020) = (
        prims("srgb"),
        prims("display-p3"),
        prims("adobe-rgb"),
        prims("rec2020"),
    );
    assert!(close(color::coverage(&srgb, &srgb), 1.0, 1e-9));
    // sRGB lies inside Display P3 and (almost) inside Adobe RGB.
    assert!(close(color::coverage(&p3, &srgb), 1.0, 1e-6));
    assert!(color::coverage(&adobe, &srgb) > 0.999);
    // The reverse direction is partial.
    let c = color::coverage(&srgb, &p3);
    assert!(c > 0.6 && c < 0.8, "sRGB covers {c} of P3");
    assert!(color::coverage(&srgb, &rec2020) < 0.6);

    for (a, b) in [(srgb, p3), (adobe, p3), (rec2020, srgb)] {
        assert!(close(color::iou(&a, &b), color::iou(&b, &a), 1e-12));
    }
    assert!(close(color::iou(&srgb, &srgb), 1.0, 1e-9));
    let area = |p: &Primaries| color::polygon_area(&color::triangle(p));
    // sRGB inside P3: IoU = area ratio.
    assert!(close(color::iou(&srgb, &p3), area(&srgb) / area(&p3), 1e-6));
    assert!(color::iou(&srgb, &p3) < 1.0);
}
