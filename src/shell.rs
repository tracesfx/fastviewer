//! Shell integration: file associations, clipboard, recycle bin, dialogs.
use crate::config::*;
use crate::decode::EXTS;
use crate::wide;
use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const PROGID: &str = "FastViewer.Image";

unsafe fn set(sub: &str, name: Option<&str>, val: &str) {
    let s = wide(sub);
    let n = name.map(wide);
    let v = wide(val);
    let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(s.as_ptr()), n.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null()),
        REG_SZ.0, Some(v.as_ptr() as *const _), (v.len() * 2) as u32);
}

fn msg(text: &str) {
    if std::env::args().any(|a| a == "--quiet") { return; }
    unsafe { MessageBoxW(None, PCWSTR(wide(text).as_ptr()), w!("FastViewer"), MB_OK); }
}

/// Registers FastViewer for the current user (no admin needed) so it shows up in "Open with" and Default apps.
pub fn register() {
    unsafe {
        let exe = std::env::current_exe().unwrap_or_default().to_string_lossy().to_string();
        let base = format!("Software\\Classes\\{}", PROGID);
        set(&base, None, "Image (FastViewer)");
        set(&base, Some("MultiSelectModel"), "Player");
        set(&format!("{}\\DefaultIcon", base), None, &format!("{},0", exe));
        set(&format!("{}\\shell\\open\\command", base), None, &format!("\"{}\" \"%1\"", exe));
        let cap = "Software\\FastViewer\\Capabilities";
        set(cap, Some("ApplicationName"), "FastViewer");
        set(cap, Some("ApplicationDescription"), "Ultra fast image viewer");
        set("Software\\RegisteredApplications", Some("FastViewer"), "Software\\FastViewer\\Capabilities");
        for e in EXTS {
            set(&format!("{}\\FileAssociations", cap), Some(&format!(".{}", e)), PROGID);
            let key = wide(&format!("Software\\Classes\\.{}\\OpenWithProgids", e));
            let nm = wide(PROGID);
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(nm.as_ptr()), REG_NONE.0, None, 0);
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        msg("FastViewer is registered.\n\nWindows 11 lets only you choose the default app: right-click an image > Open with > Choose another app > FastViewer > Always.");
    }
}

pub fn unregister() {
    unsafe {
        for k in [format!("Software\\Classes\\{}", PROGID), "Software\\FastViewer".to_string()] {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide(&k).as_ptr()));
        }
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, w!("Software\\RegisteredApplications"), w!("FastViewer"));
        for e in EXTS {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(wide(&format!("Software\\Classes\\.{}\\OpenWithProgids", e)).as_ptr()), PCWSTR(wide(PROGID).as_ptr()));
        }
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        msg("FastViewer registration removed.");
    }
}

unsafe fn put_clip(hwnd: HWND, fmt: u32, bytes: &[u8]) {
    if OpenClipboard(Some(hwnd)).is_err() { return; }
    let _ = EmptyClipboard();
    if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
        let p = GlobalLock(h) as *mut u8;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(h);
            let _ = SetClipboardData(fmt, Some(HANDLE(h.0)));
        }
    }
    let _ = CloseClipboard();
}

pub fn set_clipboard_text(hwnd: HWND, s: &str) {
    let w = wide(s);
    let bytes: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    unsafe { put_clip(hwnd, 13, &bytes); }
}

/// CF_DIB (bottom-up 32bpp); input is premultiplied BGRA which is un-premultiplied here.
pub fn set_clipboard_image(hwnd: HWND, w: u32, h: u32, px: &[u8]) {
    let mut out = Vec::with_capacity(40 + px.len());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    for y in (0..h as usize).rev() {
        for p in px[y * w as usize * 4..(y + 1) * w as usize * 4].chunks_exact(4) {
            let a = p[3] as u32;
            let un = |c: u8| if a == 0 { 0 } else { (c as u32 * 255 / a).min(255) as u8 };
            out.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2]), p[3]]);
        }
    }
    unsafe { put_clip(hwnd, 8, &out); }
}

pub fn reveal(p: &Path) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
}

pub fn open_config() {
    let path = active_ini();
    if !path.exists() {
        if let Some(d) = path.parent() { let _ = std::fs::create_dir_all(d); }
        let _ = std::fs::write(&path, DEFAULT_INI);
    }
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide(&path.to_string_lossy()).as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

pub fn recycle(hwnd: HWND, p: &Path) -> bool {
    unsafe {
        let mut from = wide(&p.to_string_lossy());
        from.push(0);
        let mut op = SHFILEOPSTRUCTW {
            hwnd, wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()),
            fFlags: FOF_ALLOWUNDO.0 as u16, ..Default::default()
        };
        SHFileOperationW(&mut op) == 0 && !op.fAnyOperationsAborted.as_bool()
    }
}

pub fn open_dialog(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let mut buf = [0u16; 2048];
        let exts: Vec<String> = EXTS.iter().map(|e| format!("*.{}", e)).collect();
        let filter = format!("Images\0{}\0All files\0*.*\0\0", exts.join(";"));
        let f: Vec<u16> = filter.encode_utf16().collect();
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd, lpstrFilter: PCWSTR(f.as_ptr()),
            lpstrFile: PWSTR(buf.as_mut_ptr()), nMaxFile: buf.len() as u32,
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER, ..Default::default()
        };
        if GetOpenFileNameW(&mut ofn).as_bool() {
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            return Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])));
        }
        None
    }
}

/// Opt-in startup timing (set FASTVIEWER_TIMING=1): ms since process creation.
pub fn since_start() -> f64 {
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    unsafe {
        let (mut c, mut e, mut k, mut u) = Default::default();
        let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u);
        let now = GetSystemTimeAsFileTime();
        let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64;
        (t(now) - t(c)) / 10000.0
    }
}
pub fn log_timing(what: &str) {
    if std::env::var_os("FASTVIEWER_TIMING").is_some() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("fastviewer_timing.txt")) {
            let _ = writeln!(f, "{:7.1} ms  {}", since_start(), what);
        }
    }
}
