// Generates a Windows resource file (app icon + manifest) without needing rc.exe / the Windows SDK.
use std::io::Write;

fn tri(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s = |p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)| (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1);
    let (d1, d2, d3) = (s(p, a, b), s(p, b, c), s(p, c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// colour (r,g,b,a 0..255) of the icon design at normalised coordinates
fn design(x: f32, y: f32) -> [f32; 4] {
    // rounded square
    let (cx, cy, hw, r) = (0.5f32, 0.5f32, 0.46f32, 0.2f32);
    let (dx, dy) = ((x - cx).abs() - (hw - r), (y - cy).abs() - (hw - r));
    let d = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
    if d > 0.0 { return [0.0; 4]; }
    let t = ((x + y) / 2.0).clamp(0.0, 1.0);
    let mut c = [0x2b as f32 + (0x7a - 0x2b) as f32 * t, 0x6c as f32 + (0x3c - 0x6c) as f32 * t, 0xff as f32 + (0xe8 - 0xff) as f32 * t, 255.0];
    if (x - 0.7).hypot(y - 0.32) < 0.095 { c = [255.0, 208.0, 80.0, 255.0]; }
    if tri((x, y), (0.50, 0.80), (0.74, 0.50), (0.90, 0.80)) && y <= 0.80 { c = [210.0, 222.0, 255.0, 255.0]; }
    if tri((x, y), (0.12, 0.80), (0.40, 0.40), (0.68, 0.80)) && y <= 0.80 { c = [255.0, 255.0, 255.0, 255.0]; }
    c
}

fn icon_dib(n: usize) -> Vec<u8> {
    let ss = 4;
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0f32; 4];
            for sy in 0..ss { for sx in 0..ss {
                let c = design((x as f32 + (sx as f32 + 0.5) / ss as f32) / n as f32, (y as f32 + (sy as f32 + 0.5) / ss as f32) / n as f32);
                let a = c[3] / 255.0;
                acc[0] += c[0] * a; acc[1] += c[1] * a; acc[2] += c[2] * a; acc[3] += a;
            } }
            let cnt = (ss * ss) as f32;
            let a = acc[3] / cnt;
            let o = ((n - 1 - y) * n + x) * 4; // bottom-up
            if acc[3] > 0.0 {
                px[o] = (acc[2] / acc[3]) as u8; px[o + 1] = (acc[1] / acc[3]) as u8; px[o + 2] = (acc[0] / acc[3]) as u8;
            }
            px[o + 3] = (a * 255.0).round() as u8;
        }
    }
    let mut d = Vec::new();
    d.extend_from_slice(&40u32.to_le_bytes());
    d.extend_from_slice(&(n as u32).to_le_bytes());
    d.extend_from_slice(&((n * 2) as u32).to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&32u16.to_le_bytes());
    d.extend_from_slice(&[0u8; 24]);
    d.extend_from_slice(&px);
    d.extend(std::iter::repeat(0u8).take(((n + 31) / 32 * 4) * n)); // AND mask
    d
}

fn u16s(s: &str) -> Vec<u8> { s.encode_utf16().chain(Some(0)).flat_map(|c| c.to_le_bytes()).collect() }

/// one .res entry with numeric type/name ids
fn entry(out: &mut Vec<u8>, ty: u16, id: u16, lang: u16, data: &[u8]) {
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&[0xFF, 0xFF]); hdr.extend_from_slice(&ty.to_le_bytes());
    hdr.extend_from_slice(&[0xFF, 0xFF]); hdr.extend_from_slice(&id.to_le_bytes());
    hdr.extend_from_slice(&0u32.to_le_bytes()); // data version
    hdr.extend_from_slice(&0x1030u16.to_le_bytes()); // memory flags
    hdr.extend_from_slice(&lang.to_le_bytes());
    hdr.extend_from_slice(&0u32.to_le_bytes()); // version
    hdr.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    let header_size = 8 + hdr.len() as u32;
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(&hdr);
    out.extend_from_slice(data);
    while out.len() % 4 != 0 { out.push(0); }
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let sizes = [16usize, 24, 32, 48, 64, 128];
    let mut res = Vec::new();
    // mandatory empty first entry
    res.extend_from_slice(&[0, 0, 0, 0, 0x20, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    for (i, &n) in sizes.iter().enumerate() {
        let dib = icon_dib(n);
        entry(&mut res, 3, (i + 1) as u16, 0x0409, &dib);
        group.push(n as u8); group.push(n as u8); group.push(0); group.push(0);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        group.extend_from_slice(&((i + 1) as u16).to_le_bytes());
    }
    entry(&mut res, 14, 1, 0x0409, &group);
    entry(&mut res, 24, 1, 0x0409, MANIFEST.as_bytes());
    let _ = u16s; // (kept for named resources if ever needed)
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.res");
    std::fs::File::create(&out).unwrap().write_all(&res).unwrap();
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
