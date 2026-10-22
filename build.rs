//! Embeds the Windows resources (version info, manifest, icon) without windres or any other tool: it
//! writes a COFF object holding a `.rsrc` section and hands it to the linker. GNU ld merges that section
//! into the exe exactly as it does for the output of windres.
//!
//! Dependency-free on purpose. The icon pixels come from `src/icon_render.rs`, shared with the app.

use std::path::PathBuf;

#[allow(dead_code)]
mod icon_render {
    include!("src/icon_render.rs");
}

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;
/// en-US; the version info translation below says the same.
const LANG: u16 = 0x0409;

const COMPANY: &str = "Spectrum contributors";
const DESCRIPTION: &str = "Spectrum: monitor color profiles";
const PRODUCT: &str = "Spectrum";
const EXE_NAME: &str = "Spectrum.exe";
const COPYRIGHT: &str = "MIT License";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon_render.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_os != "windows" || arch != "x86_64" {
        return;
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());

    let resources = build_resources(&version);
    let obj = coff_object(&resources);
    let path = out_dir.join("spectrum_resources.o");
    std::fs::write(&path, obj).expect("write resource object");
    println!(
        "cargo:rustc-link-arg-bins={}",
        path.to_string_lossy().replace('\\', "/")
    );
}

// ---- resources ---------------------------------------------------------------------------------

/// (resource type, resource id, data)
type Res = (u16, u16, Vec<u8>);

fn build_resources(version: &str) -> Vec<Res> {
    let mut v: Vec<Res> = Vec::new();
    // Icons: RT_ICON 1..n are the images, RT_GROUP_ICON 1 lists them.
    let mut group = Vec::new();
    push_u16(&mut group, 0);
    push_u16(&mut group, 1);
    push_u16(&mut group, icon_render::SIZES.len() as u16);
    for (i, &size) in icon_render::SIZES.iter().enumerate() {
        let id = (i + 1) as u16;
        let dib = icon_render::dib_entry(size);
        let dim = if size >= 256 { 0 } else { size as u8 };
        group.extend_from_slice(&[dim, dim, 0, 0]);
        push_u16(&mut group, 1); // planes
        push_u16(&mut group, 32); // bit count
        push_u32(&mut group, dib.len() as u32);
        push_u16(&mut group, id);
        v.push((RT_ICON, id, dib));
    }
    v.push((RT_GROUP_ICON, 1, group));
    v.push((RT_VERSION, 1, version_info(version)));
    v.push((RT_MANIFEST, 1, manifest(version).into_bytes()));
    v.sort_by_key(|r| (r.0, r.1));
    v
}

fn manifest(version: &str) -> String {
    let four = four_part(version);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Spectrum.Spectrum" version="{four}" processorArchitecture="*"/>
  <description>{DESCRIPTION}</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
"#
    )
}

/// "1.2.3-beta" gives [1, 2, 3, 0].
fn parts(version: &str) -> [u16; 4] {
    let mut out = [0u16; 4];
    let core = version.split(['-', '+']).next().unwrap_or("");
    for (i, p) in core.split('.').take(4).enumerate() {
        out[i] = p.parse().unwrap_or(0);
    }
    out
}

fn four_part(version: &str) -> String {
    let p = parts(version);
    format!("{}.{}.{}.{}", p[0], p[1], p[2], p[3])
}

// ---- VS_VERSIONINFO ----------------------------------------------------------------------------

fn utf16z(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.encode_utf16().chain(std::iter::once(0)) {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out
}

fn pad4(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

/// One version-info block: header, key, DWORD padding, value, DWORD padding, children.
fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    b.extend_from_slice(&utf16z(key));
    pad4(&mut b);
    b.extend_from_slice(value);
    for c in children {
        pad4(&mut b);
        b.extend_from_slice(c);
    }
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b[2..4].copy_from_slice(&value_len.to_le_bytes());
    b[4..6].copy_from_slice(&(text as u16).to_le_bytes());
    b
}

fn string_entry(key: &str, value: &str) -> Vec<u8> {
    let v = utf16z(value);
    block(key, &v, (v.len() / 2) as u16, true, &[])
}

fn version_info(version: &str) -> Vec<u8> {
    let p = parts(version);
    let ms = ((p[0] as u32) << 16) | p[1] as u32;
    let ls = ((p[2] as u32) << 16) | p[3] as u32;
    let mut fixed = Vec::new();
    for v in [
        0xFEEF_04BDu32, // signature
        0x0001_0000,    // struct version
        ms,
        ls,
        ms,
        ls,
        0x3F,        // flags mask
        0,           // flags
        0x0004_0004, // VOS_NT_WINDOWS32
        1,           // VFT_APP
        0,
        0,
        0,
    ] {
        push_u32(&mut fixed, v);
    }
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", COMPANY.to_string()),
        ("FileDescription", DESCRIPTION.to_string()),
        ("FileVersion", version.to_string()),
        ("InternalName", EXE_NAME.to_string()),
        ("LegalCopyright", COPYRIGHT.to_string()),
        ("OriginalFilename", EXE_NAME.to_string()),
        ("ProductName", PRODUCT.to_string()),
        ("ProductVersion", version.to_string()),
    ]
    .iter()
    .map(|(k, v)| string_entry(k, v))
    .collect();
    let table = block("040904B0", &[], 0, true, &strings);
    let string_file_info = block("StringFileInfo", &[], 0, true, &[table]);
    let translation = block("Translation", &0x04B0_0409u32.to_le_bytes(), 4, false, &[]);
    let var_file_info = block("VarFileInfo", &[], 0, true, &[translation]);
    block(
        "VS_VERSION_INFO",
        &fixed,
        fixed.len() as u16,
        false,
        &[string_file_info, var_file_info],
    )
}

// ---- resource directory tree -------------------------------------------------------------------

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn push_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn dir_header(v: &mut Vec<u8>, id_entries: usize) {
    push_u32(v, 0); // characteristics
    push_u32(v, 0); // time stamp
    push_u16(v, 0); // major
    push_u16(v, 0); // minor
    push_u16(v, 0); // named entries
    push_u16(v, id_entries as u16);
}

fn align(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

/// The `.rsrc` section content (type, name, language directories, data entries, data) and the offsets of
/// the `OffsetToData` fields, which need an RVA relocation.
fn section_data(res: &[Res]) -> (Vec<u8>, Vec<u32>) {
    // Distinct types in ascending order, each with its (sorted) resources.
    let mut types: Vec<u16> = res.iter().map(|r| r.0).collect();
    types.dedup();
    let per_type = |t: u16| res.iter().filter(move |r| r.0 == t);

    let root_size = 16 + 8 * types.len();
    let type_dirs_start = root_size;
    let mut type_dir_offs = Vec::new();
    let mut off = type_dirs_start;
    for &t in &types {
        type_dir_offs.push(off);
        off += 16 + 8 * per_type(t).count();
    }
    let name_dirs_start = off;
    let n = res.len();
    let lang_dirs_start = name_dirs_start; // one language directory per resource (16 + 8 each)
    let entries_start = lang_dirs_start + n * 24;
    let data_start = align(entries_start + n * 16, 8);

    let mut data_offs = Vec::new();
    let mut cur = data_start;
    for r in res {
        data_offs.push(cur);
        cur = align(cur + r.2.len(), 8);
    }

    let mut s: Vec<u8> = Vec::new();
    // root
    dir_header(&mut s, types.len());
    for (i, &t) in types.iter().enumerate() {
        push_u32(&mut s, t as u32);
        push_u32(&mut s, 0x8000_0000 | type_dir_offs[i] as u32);
    }
    // per-type directories (names)
    let mut idx = 0usize;
    for &t in &types {
        let items: Vec<&Res> = per_type(t).collect();
        dir_header(&mut s, items.len());
        for r in &items {
            push_u32(&mut s, r.1 as u32);
            push_u32(&mut s, 0x8000_0000 | (lang_dirs_start + idx * 24) as u32);
            idx += 1;
        }
    }
    // per-resource language directories
    for i in 0..n {
        dir_header(&mut s, 1);
        push_u32(&mut s, LANG as u32);
        push_u32(&mut s, (entries_start + i * 16) as u32);
    }
    // data entries
    let mut reloc_offsets = Vec::new();
    for (i, r) in res.iter().enumerate() {
        reloc_offsets.push(s.len() as u32);
        push_u32(&mut s, data_offs[i] as u32); // RVA = section RVA + this (added by the linker)
        push_u32(&mut s, r.2.len() as u32);
        push_u32(&mut s, 0); // code page
        push_u32(&mut s, 0);
    }
    s.resize(data_start, 0);
    for (i, r) in res.iter().enumerate() {
        debug_assert_eq!(s.len(), data_offs[i]);
        s.extend_from_slice(&r.2);
        let a = align(s.len(), 8);
        s.resize(a, 0);
    }
    (s, reloc_offsets)
}

// ---- COFF object -------------------------------------------------------------------------------

fn coff_object(res: &[Res]) -> Vec<u8> {
    let (data, relocs) = section_data(res);
    let raw_ptr = 20 + 40;
    let reloc_ptr = raw_ptr + data.len();
    let sym_ptr = reloc_ptr + relocs.len() * 10;

    let mut o: Vec<u8> = Vec::new();
    // IMAGE_FILE_HEADER
    push_u16(&mut o, 0x8664); // AMD64
    push_u16(&mut o, 1); // sections
    push_u32(&mut o, 0); // time stamp
    push_u32(&mut o, sym_ptr as u32);
    push_u32(&mut o, 2); // symbols: the section symbol and its aux record
    push_u16(&mut o, 0); // optional header
    push_u16(&mut o, 0); // characteristics
                         // IMAGE_SECTION_HEADER
    o.extend_from_slice(b".rsrc\0\0\0");
    push_u32(&mut o, 0); // virtual size
    push_u32(&mut o, 0); // virtual address
    push_u32(&mut o, data.len() as u32);
    push_u32(&mut o, raw_ptr as u32);
    push_u32(&mut o, reloc_ptr as u32);
    push_u32(&mut o, 0); // line numbers
    push_u16(&mut o, relocs.len() as u16);
    push_u16(&mut o, 0);
    push_u32(&mut o, 0x4030_0040); // initialized data, 4-byte aligned, readable
    assert_eq!(o.len(), raw_ptr);
    o.extend_from_slice(&data);
    // relocations: IMAGE_REL_AMD64_ADDR32NB against the section symbol (index 0)
    for off in &relocs {
        push_u32(&mut o, *off);
        push_u32(&mut o, 0);
        push_u16(&mut o, 3);
    }
    assert_eq!(o.len(), sym_ptr);
    // symbol: .rsrc, value 0, section 1, type 0, static, one aux record
    o.extend_from_slice(b".rsrc\0\0\0");
    push_u32(&mut o, 0);
    push_u16(&mut o, 1);
    push_u16(&mut o, 0);
    o.push(3); // IMAGE_SYM_CLASS_STATIC
    o.push(1); // aux records
               // aux: length, relocation count, line numbers, checksum, number, selection
    push_u32(&mut o, data.len() as u32);
    push_u16(&mut o, relocs.len() as u16);
    push_u16(&mut o, 0);
    push_u32(&mut o, 0);
    push_u16(&mut o, 0);
    o.push(0);
    o.extend_from_slice(&[0, 0, 0]);
    // string table (empty)
    push_u32(&mut o, 4);
    o
}
