//! Image decoding + metadata via the Windows Imaging Component (no codecs bundled in the exe).
use crate::{post, wide};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use windows::core::*;
use windows::Win32::Foundation::{FILETIME, GENERIC_READ, SYSTEMTIME};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::*;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

const WM_LOADED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
const WM_FILES: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;
const MAX_SIDE: u32 = 16384; // GPU texture limit

/// Generation of the newest foreground request; older decodes bail out early.
pub static LATEST: AtomicU64 = AtomicU64::new(0);

pub struct Raw { pub w: u32, pub h: u32, pub px: Vec<u8>, pub meta: Vec<(String, String)>, pub anim: bool }
pub struct Loaded { pub path: PathBuf, pub res: std::result::Result<Raw, String> }

pub const EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "jfif", "webp", "gif", "bmp", "dib", "tif", "tiff", "ico",
    "heic", "heif", "avif", "jxr", "wdp", "hdp", "dds", "jxl",
];

pub fn wic_factory() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

pub fn spawn_decode(path: PathBuf, gen: u64, prefetch: bool) {
    std::thread::spawn(move || {
        unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
        crate::shell::log_timing("worker started");
        let res = (|| {
            let f = wic_factory().ok_or("Windows Imaging Component unavailable")?;
            crate::shell::log_timing("wic factory ready");
            decode_full(&f, &path, gen, prefetch).map_err(|e| e.message())
        })();
        crate::shell::log_timing("decode done");
        post(WM_LOADED, Box::into_raw(Box::new(Loaded { path, res })) as isize);
    });
}

fn is_image_ext(p: &Path) -> bool {
    p.extension().map(|e| EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

fn dir_images(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| is_image_ext(p)).collect()).unwrap_or_default()
}

/// List the images in the folder of `path`.
pub fn spawn_list(path: PathBuf, gen: u64) {
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    spawn_list_multi(vec![dir], gen);
}

/// Build the gallery from any mix of files and folders (drag & drop / command line lists, any size).
/// Folders are expanded (one level), explicitly listed files are always kept, duplicates removed, natural sort.
pub fn spawn_list_multi(paths: Vec<PathBuf>, gen: u64) {
    std::thread::spawn(move || {
        let mut seen = std::collections::HashSet::new();
        let mut files: Vec<PathBuf> = Vec::with_capacity(paths.len());
        for p in paths {
            let batch = if p.is_dir() { dir_images(&p) } else { vec![p] };
            for f in batch { if seen.insert(f.to_string_lossy().to_lowercase()) { files.push(f); } }
        }
        let keys: Vec<Vec<u16>> = files.iter().map(|p| wide(&p.to_string_lossy())).collect();
        let mut order: Vec<usize> = (0..files.len()).collect();
        order.sort_by(|&a, &b| unsafe {
            windows::Win32::UI::Shell::StrCmpLogicalW(PCWSTR(keys[a].as_ptr()), PCWSTR(keys[b].as_ptr())).cmp(&0)
        });
        let sorted: Vec<PathBuf> = order.iter().map(|&i| std::mem::take(&mut files[i])).collect();
        post(WM_FILES, Box::into_raw(Box::new((gen, sorted))) as isize);
    });
}

// ---------- animated GIF ----------
pub static ANIM_GEN: AtomicU64 = AtomicU64::new(0);
pub static ANIM_PAUSED: AtomicBool = AtomicBool::new(false);
pub static ANIM_STEP: AtomicBool = AtomicBool::new(false);
const WM_ANIM: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 5;

/// Composes GIF frames onto a canvas honouring offsets, transparency and disposal methods.
pub struct GifPlayer {
    f: IWICImagingFactory, dec: IWICBitmapDecoder,
    pub w: u32, pub h: u32, pub count: u32, pub last: u32,
    webp: bool, idx: u32, pub canvas: Vec<u8>, saved: Option<Vec<u8>>, prev_disp: u8, prev_rect: (i32, i32, i32, i32),
}

impl GifPlayer {
    pub fn new(f: &IWICImagingFactory, dec: IWICBitmapDecoder) -> Result<GifPlayer> {
        unsafe {
            let count = dec.GetFrameCount()?;
            let (mut w, mut h) = (0u32, 0u32);
            if let Ok(r) = dec.GetMetadataQueryReader() {
                w = query_u16(&r, "/logscrdesc/Width").unwrap_or(0) as u32;
                h = query_u16(&r, "/logscrdesc/Height").unwrap_or(0) as u32;
            }
            let webp = dec.GetContainerFormat().map(|g| g == GUID_ContainerFormatWebp).unwrap_or(false);
            if w == 0 || h == 0 { dec.GetFrame(0)?.GetSize(&mut w, &mut h)?; }
            Ok(GifPlayer { f: f.clone(), dec, w, h, count, last: 0, webp, idx: 0, canvas: vec![0; (w * h * 4) as usize],
                saved: None, prev_disp: 0, prev_rect: (0, 0, 0, 0) })
        }
    }
    pub fn from_path(f: &IWICImagingFactory, path: &Path) -> Result<GifPlayer> {
        unsafe {
            let wp = wide_os(path);
            let dec = f.CreateDecoderFromFilename(PCWSTR(wp.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            GifPlayer::new(f, dec)
        }
    }
    /// Composes the next frame into `canvas`; returns how long (ms) it should stay on screen.
    pub fn next(&mut self) -> Result<u32> {
        unsafe {
            let (w, h) = (self.w as i32, self.h as i32);
            if self.idx == 0 { self.canvas.iter_mut().for_each(|b| *b = 0); self.saved = None; self.prev_disp = 0; }
            else {
                match self.prev_disp {
                    2 => { let (l, t, fw, fh) = self.prev_rect; self.clear_rect(l, t, fw, fh); }
                    3 => { if let Some(s) = self.saved.take() { self.canvas = s; } }
                    _ => {}
                }
            }
            let frame = self.dec.GetFrame(self.idx)?;
            let (mut fw, mut fh) = (0u32, 0u32);
            frame.GetSize(&mut fw, &mut fh)?;
            let (mut left, mut top, mut delay, mut disp) = (0i32, 0i32, 10u32, 0u8);
            if let Ok(r) = frame.GetMetadataQueryReader() {
                left = query_u16(&r, "/imgdesc/Left").unwrap_or(0) as i32;
                top = query_u16(&r, "/imgdesc/Top").unwrap_or(0) as i32;
                if let Some(d) = query_u16(&r, "/grctlext/Delay") { delay = d as u32; }
                disp = query_u16(&r, "/grctlext/Disposal").unwrap_or(0) as u8;
                if self.webp {
                    // WIC hands out fully composited WebP frames: just replace the canvas; duration is in ms
                    left = 0; top = 0; disp = 2;
                    delay = match query(&r, "/ANMF/FrameDuration") { Some((Val::U(n),)) => (n / 10).min(u32::MAX as u64) as u32, _ => 10 };
                }
            }
            if disp == 3 { self.saved = Some(self.canvas.clone()); }
            let conv = self.f.CreateFormatConverter()?;
            conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let mut buf = vec![0u8; (fw * fh * 4) as usize];
            conv.CopyPixels(std::ptr::null(), fw * 4, &mut buf)?;
            for y in 0..fh as i32 {
                let dy = top + y;
                if dy < 0 || dy >= h { continue; }
                for x in 0..fw as i32 {
                    let dx = left + x;
                    if dx < 0 || dx >= w { continue; }
                    let s = ((y as u32 * fw + x as u32) * 4) as usize;
                    let a = buf[s + 3] as u32;
                    if a == 0 { continue; }
                    let d = ((dy as u32 * self.w + dx as u32) * 4) as usize;
                    if a == 255 { self.canvas[d..d + 4].copy_from_slice(&buf[s..s + 4]); }
                    else { for k in 0..4 { self.canvas[d + k] = (buf[s + k] as u32 + self.canvas[d + k] as u32 * (255 - a) / 255).min(255) as u8; } }
                }
            }
            self.prev_disp = disp;
            self.prev_rect = (left, top, fw as i32, fh as i32);
            self.last = self.idx;
            self.idx = (self.idx + 1) % self.count;
            // browsers treat 0/1 (<=10 ms) as 100 ms
            Ok(if delay <= 1 { 100 } else { delay * 10 })
        }
    }
    fn clear_rect(&mut self, l: i32, t: i32, fw: i32, fh: i32) {
        for y in t.max(0)..(t + fh).min(self.h as i32) {
            for x in l.max(0)..(l + fw).min(self.w as i32) {
                let d = ((y as u32 * self.w + x as u32) * 4) as usize;
                self.canvas[d..d + 4].copy_from_slice(&[0; 4]);
            }
        }
    }
}

/// Plays frames 1.. of an already-displayed GIF, posting each composed canvas to the UI thread.
pub fn spawn_anim(path: PathBuf, id: u64) {
    std::thread::spawn(move || {
        unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
        let Some(f) = wic_factory() else { return };
        let Ok(mut p) = GifPlayer::from_path(&f, &path) else { return };
        let Ok(d0) = p.next() else { return };
        let mut due = std::time::Instant::now() + std::time::Duration::from_millis(d0 as u64);
        loop {
            // wait for the frame's display time; a paused animation waits for a step request
            loop {
                if ANIM_GEN.load(Ordering::SeqCst) != id { return; }
                if ANIM_PAUSED.load(Ordering::SeqCst) {
                    if ANIM_STEP.swap(false, Ordering::SeqCst) { break; }
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    due = std::time::Instant::now();
                    continue;
                }
                let now = std::time::Instant::now();
                if now >= due { break; }
                std::thread::sleep((due - now).min(std::time::Duration::from_millis(15)));
            }
            let Ok(d) = p.next() else { return };
            if ANIM_GEN.load(Ordering::SeqCst) != id { return; }
            post(WM_ANIM, Box::into_raw(Box::new((id, p.last, p.count, p.canvas.clone()))) as isize);
            due += std::time::Duration::from_millis(d as u64);
            let now = std::time::Instant::now();
            if due + std::time::Duration::from_millis(250) < now { due = now + std::time::Duration::from_millis(d as u64); }
        }
    });
}

struct Opened { src: IWICBitmapSource, dec: IWICBitmapDecoder, frame: IWICBitmapFrameDecode, ow: u32, oh: u32 }

fn query_u16(r: &IWICMetadataQueryReader, name: &str) -> Option<u16> {
    let v = query(r, name)?;
    Some(match v.0 { Val::U(n) => n as u16, _ => return None })
}

enum Val { S(String), U(u64), R(u32, u32), F(f64) }

fn query(r: &IWICMetadataQueryReader, name: &str) -> Option<(Val,)> {
    unsafe {
        let n = wide(name);
        let mut pv = PROPVARIANT::default();
        r.GetMetadataByName(PCWSTR(n.as_ptr()), &mut pv).ok()?;
        let inner = &pv.Anonymous.Anonymous;
        let vt = inner.vt.0 as u32;
        let u = &inner.Anonymous;
        let out = match vt {
            2 => Some(Val::U(u.uiVal as u64)),
            3 => Some(Val::U(u.lVal as u64)),
            17 => Some(Val::U(u.bVal as u64)),
            18 => Some(Val::U(u.uiVal as u64)),
            19 => Some(Val::U(u.ulVal as u64)),
            21 => { let v = u.uhVal; Some(Val::R(v as u32, (v >> 32) as u32)) }
            20 => { let v = u.uhVal; Some(Val::R(v as u32, (v >> 32) as u32)) }
            4 => Some(Val::F(u.fltVal as f64)),
            5 => Some(Val::F(u.dblVal)),
            30 => u.pszVal.to_string().ok().map(Val::S),
            31 => u.pwszVal.to_string().ok().map(Val::S),
            _ => None,
        };
        let _ = PropVariantClear(&mut pv);
        out.map(|v| (v,))
    }
}

fn qs(r: &IWICMetadataQueryReader, prefixes: &[&str], tail: &str) -> Option<Val> {
    for p in prefixes { if let Some(v) = query(r, &format!("{}{}", p, tail)) { return Some(v.0); } }
    None
}

fn orientation(frame: &IWICBitmapFrameDecode) -> u16 {
    unsafe {
        let Ok(r) = frame.GetMetadataQueryReader() else { return 1 };
        for p in ["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"] {
            if let Some(o) = query_u16(&r, p) { return o; }
        }
        1
    }
}

fn open(f: &IWICImagingFactory, path: &Path, max_side: u32, gen: u64, check: bool) -> Result<Opened> {
    unsafe {
        let wp = crate::decode::wide_os(path);
        let dec = f.CreateDecoderFromFilename(PCWSTR(wp.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let (mut ow, mut oh) = (0, 0);
        frame.GetSize(&mut ow, &mut oh)?;
        if check && LATEST.load(Ordering::SeqCst) > gen { return Err(Error::new(HRESULT(0x80004004u32 as i32), "cancelled")); }
        let conv = f.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut src: IWICBitmapSource = conv.cast()?;
        let o = orientation(&frame);
        let m = ow.max(oh);
        if m > max_side {
            let s = max_side as f64 / m as f64;
            let (nw, nh) = (((ow as f64 * s).round() as u32).max(1), ((oh as f64 * s).round() as u32).max(1));
            let sc = f.CreateBitmapScaler()?;
            sc.Initialize(&src, nw, nh, WICBitmapInterpolationModeFant)?;
            src = sc.cast()?;
        }
        if o > 1 {
            let opt = match o {
                2 => WICBitmapTransformFlipHorizontal,
                3 => WICBitmapTransformRotate180,
                4 => WICBitmapTransformFlipVertical,
                5 => WICBitmapTransformOptions(WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0),
                6 => WICBitmapTransformRotate90,
                7 => WICBitmapTransformOptions(WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0),
                8 => WICBitmapTransformRotate270,
                _ => WICBitmapTransformRotate0,
            };
            let r = f.CreateBitmapFlipRotator()?;
            r.Initialize(&src, opt)?;
            src = r.cast()?;
        }
        Ok(Opened { src, dec, frame, ow, oh })
    }
}

pub fn wide_os(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn copy_pixels(src: &IWICBitmapSource) -> Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let (mut w, mut h) = (0, 0);
        src.GetSize(&mut w, &mut h)?;
        let mut px = vec![0u8; (w as usize) * (h as usize) * 4];
        src.CopyPixels(std::ptr::null(), w * 4, &mut px)?;
        Ok((w, h, px))
    }
}

fn decode_full(f: &IWICImagingFactory, path: &Path, gen: u64, prefetch: bool) -> Result<Raw> {
    let o = open(f, path, MAX_SIDE, gen, !prefetch)?;
    unsafe {
        // animated GIF: compose frame 0 on the logical-screen canvas; the UI starts playback of the rest
        if o.dec.GetContainerFormat().map(|g| g == GUID_ContainerFormatGif || g == GUID_ContainerFormatWebp).unwrap_or(false) && o.dec.GetFrameCount().unwrap_or(1) > 1 {
            if let Ok(mut p) = GifPlayer::new(f, o.dec.clone()) {
                if p.next().is_ok() {
                    let (w, h) = (p.w, p.h);
                    let mut meta = gather_meta(f, path, &o, w, h);
                    meta.push(("Animation".into(), format!("{} frames", p.count)));
                    return Ok(Raw { w, h, px: std::mem::take(&mut p.canvas), meta, anim: true });
                }
            }
        }
    }
    let (w, h, px) = copy_pixels(&o.src)?;
    let meta = gather_meta(f, path, &o, w, h);
    Ok(Raw { w, h, px, meta, anim: false })
}

pub fn decode_thumb(f: &IWICImagingFactory, path: &Path, side: u32) -> Result<Raw> {
    // open() scales to `side` before rotating, so thumbnails stay small
    let o = open(f, path, side, 0, false)?;
    let (w, h, px) = copy_pixels(&o.src)?;
    Ok(Raw { w, h, px, meta: vec![], anim: false })
}

fn human(n: u64) -> String {
    let n = n as f64;
    if n < 1024.0 { format!("{} B", n) } else if n < 1048576.0 { format!("{:.1} KB", n / 1024.0) }
    else if n < 1073741824.0 { format!("{:.2} MB", n / 1048576.0) } else { format!("{:.2} GB", n / 1073741824.0) }
}

fn local_time(t: std::time::SystemTime) -> Option<String> {
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let ft100 = (secs + 11_644_473_600) * 10_000_000;
    let ft = FILETIME { dwLowDateTime: ft100 as u32, dwHighDateTime: (ft100 >> 32) as u32 };
    unsafe {
        let (mut utc, mut loc) = (SYSTEMTIME::default(), SYSTEMTIME::default());
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).ok()?;
        Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", loc.wYear, loc.wMonth, loc.wDay, loc.wHour, loc.wMinute, loc.wSecond))
    }
}

fn rat(v: &Val) -> Option<f64> {
    match v { Val::R(n, d) if *d != 0 => Some(*n as f64 / *d as f64), Val::F(x) => Some(*x), Val::U(x) => Some(*x as f64), _ => None }
}

fn gather_meta(f: &IWICImagingFactory, path: &Path, o: &Opened, w: u32, h: u32) -> Vec<(String, String)> {
    let mut m: Vec<(String, String)> = vec![];
    let mut add = |k: &str, v: String| { if !v.is_empty() { m.push((k.to_string(), v)); } };
    add("Name", path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    add("Folder", path.parent().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    if let Ok(md) = std::fs::metadata(path) {
        add("Size", format!("{} ({} bytes)", human(md.len()), md.len()));
        if let Ok(t) = md.modified() { if let Some(s) = local_time(t) { add("Modified", s); } }
    }
    let (ow, oh) = if w == 0 { (o.ow, o.oh) } else { (w, h) };
    add("Dimensions", format!("{} x {} px", ow, oh));
    add("Megapixels", format!("{:.2} MP", ow as f64 * oh as f64 / 1e6));
    let g = gcd(ow, oh).max(1);
    add("Aspect", format!("{}:{}  ({:.3})", ow / g, oh / g, ow as f64 / oh.max(1) as f64));
    unsafe {
        if let Ok(g) = o.dec.GetContainerFormat() {
            let name = if g == GUID_ContainerFormatPng { "PNG" } else if g == GUID_ContainerFormatJpeg { "JPEG" }
                else if g == GUID_ContainerFormatGif { "GIF" } else if g == GUID_ContainerFormatBmp { "BMP" }
                else if g == GUID_ContainerFormatTiff { "TIFF" } else if g == GUID_ContainerFormatIco { "ICO" }
                else if g == GUID_ContainerFormatWmp { "JPEG XR" } else if g == GUID_ContainerFormatHeif { "HEIF" }
                else if g == GUID_ContainerFormatWebp { "WebP" } else if g == GUID_ContainerFormatDds { "DDS" } else { "" };
            add("Format", name.to_string());
        }
        if let Ok(pf) = o.frame.GetPixelFormat() {
            if let Ok(ci) = f.CreateComponentInfo(&pf) {
                let mut buf = [0u16; 128];
                let mut n = 0u32;
                if ci.GetFriendlyName(&mut buf, &mut n).is_ok() {
                    add("Pixel format", String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1).min(127)]));
                }
            }
        }
        let (mut dx, mut dy) = (0.0, 0.0);
        if o.frame.GetResolution(&mut dx, &mut dy).is_ok() && dx > 0.0 { add("DPI", format!("{:.0} x {:.0}", dx, dy)); }
        if let Ok(n) = o.dec.GetFrameCount() { if n > 1 { add("Frames", format!("{} (first shown)", n)); } }

        if let Ok(r) = o.frame.GetMetadataQueryReader() {
            let p = ["/app1/ifd", "/ifd"];
            let pe = ["/app1/ifd/exif", "/ifd/exif"];
            let s = |v: Option<Val>| match v { Some(Val::S(s)) => s.trim().to_string(), _ => String::new() };
            add("Camera", { let mk = s(qs(&r, &p, "/{ushort=271}")); let md = s(qs(&r, &p, "/{ushort=272}"));
                if md.starts_with(&mk) { md } else { format!("{} {}", mk, md).trim().to_string() } });
            add("Lens", s(qs(&r, &pe, "/{ushort=42036}")));
            if let Some(v) = qs(&r, &pe, "/{ushort=33434}").as_ref().and_then(rat) {
                add("Exposure", if v > 0.0 && v < 1.0 { format!("1/{:.0} s", 1.0 / v) } else { format!("{} s", v) });
            }
            if let Some(v) = qs(&r, &pe, "/{ushort=33437}").as_ref().and_then(rat) { add("Aperture", format!("f/{:.1}", v)); }
            if let Some(v) = qs(&r, &pe, "/{ushort=34855}").as_ref().and_then(rat) { add("ISO", format!("{:.0}", v)); }
            if let Some(v) = qs(&r, &pe, "/{ushort=37386}").as_ref().and_then(rat) {
                let eq = qs(&r, &pe, "/{ushort=41989}").as_ref().and_then(rat);
                add("Focal length", match eq { Some(e) if e > 0.0 => format!("{:.0} mm ({:.0} mm eq.)", v, e), _ => format!("{:.0} mm", v) });
            }
            if let Some(v) = qs(&r, &pe, "/{ushort=36867}").or_else(|| qs(&r, &p, "/{ushort=306}")) { add("Taken", s(Some(v)).replacen(':', "-", 2)); }
            add("Software", s(qs(&r, &p, "/{ushort=305}")));
            add("Artist", s(qs(&r, &p, "/{ushort=315}")));
            add("Copyright", s(qs(&r, &p, "/{ushort=33432}")));
            // GPS
            let gp = ["/app1/ifd/gps", "/ifd/gps"];
            let lat = gps_coord(&r, &gp, 1, 2);
            let lon = gps_coord(&r, &gp, 3, 4);
            if let (Some(a), Some(b)) = (lat, lon) { add("GPS", format!("{:.6}, {:.6}", a, b)); }
            // PNG text chunks (e.g. generation parameters)
            for k in ["parameters", "prompt", "Description", "Comment", "Title", "Author", "Software", "Source"] {
                let v = s(query(&r, &format!("/tEXt/{{str={}}}", k)).map(|v| v.0));
                if !v.is_empty() { add(k, v.chars().take(1500).collect()); }
            }
        }
    }
    m
}

fn gps_coord(r: &IWICMetadataQueryReader, gp: &[&str], ref_id: u32, val_id: u32) -> Option<f64> {
    // GPS lat/long are vectors of 3 rationals; WIC exposes elements via [index]
    unsafe {
        for p in gp {
            let rf = query(r, &format!("{}/{{ushort={}}}", p, ref_id));
            let mut parts = [0.0f64; 3];
            let mut ok = true;
            for i in 0..3 {
                let n = wide(&format!("{}/{{ushort={}}}", p, val_id));
                let mut pv = PROPVARIANT::default();
                if r.GetMetadataByName(PCWSTR(n.as_ptr()), &mut pv).is_err() { ok = false; break; }
                let inner = &pv.Anonymous.Anonymous;
                // VT_VECTOR (0x1000) | VT_UI8 (21)
                if inner.vt.0 as u32 == (0x1000 | 21) {
                    let ca = &inner.Anonymous.cauh;
                    if (ca.cElems as usize) > i {
                        let v = *ca.pElems.add(i);
                        let (nu, de) = (v as u32, (v >> 32) as u32);
                        parts[i] = if de == 0 { 0.0 } else { nu as f64 / de as f64 };
                    } else { ok = false; }
                } else { ok = false; }
                let _ = PropVariantClear(&mut pv);
                if !ok { break; }
            }
            if ok {
                let mut d = parts[0] + parts[1] / 60.0 + parts[2] / 3600.0;
                if let Some((Val::S(s),)) = rf { if s.starts_with('S') || s.starts_with('W') { d = -d; } }
                return Some(d);
            }
        }
    }
    None
}

fn gcd(a: u32, b: u32) -> u32 { if b == 0 { a } else { gcd(b, a % b) } }
