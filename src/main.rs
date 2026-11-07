// FastViewer - a tiny, fast image viewer for Windows.
// Window appears first; decoding (Windows Imaging Component) runs on a worker thread; drawing is Direct2D.
#![windows_subsystem = "windows"]
#![allow(static_mut_refs, non_snake_case, clippy::too_many_arguments)]

mod config;
mod decode;
mod shell;
mod ui;

use config::*;
use decode::*;
use ui::*;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc;
use windows::core::*;
use windows_numerics::{Matrix3x2, Vector2};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
const WM_LOADED: u32 = WM_APP + 1;
const WM_FILES: u32 = WM_APP + 2;
const WM_THUMB: u32 = WM_APP + 3;
const WM_ANIM: u32 = WM_APP + 5;
const WM_TEST_MODS: u32 = WM_APP + 9;
const WM_TEST_DROP: u32 = WM_APP + 10;
const WM_TEST_DUMP: u32 = WM_APP + 11;

pub fn post(msg: u32, ptr: isize) {
    // The worker may finish before the window exists; wait for it.
    let mut n = 0;
    loop {
        let h = HWND_RAW.load(Ordering::SeqCst);
        if h != 0 {
            unsafe { let _ = PostMessageW(Some(HWND(h as *mut c_void)), msg, WPARAM(0), LPARAM(ptr)); }
            return;
        }
        n += 1;
        if n > 5000 { return; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

struct Img {
    w: u32,
    h: u32,
    px: Vec<u8>,
    meta: Vec<(String, String)>,
    bmp: Option<ID2D1Bitmap>,
    chan: Option<(u8, ID2D1Bitmap)>,
    hist: Option<Box<[[u32; 256]; 4]>>,
    anim: bool,
    frame: (u32, u32),
}
impl Img {
    fn from_raw(r: Raw) -> Img {
        Img { w: r.w, h: r.h, px: r.px, meta: r.meta, bmp: None, chan: None, hist: None, anim: r.anim, frame: (0, 0) }
    }
}

struct Thumb { w: u32, h: u32, px: Vec<u8>, bmp: Option<ID2D1Bitmap> }

struct App {
    hwnd: HWND,
    ui: f32,
    cfg: Cfg,
    binds: Vec<(u32, u8, String)>,
    d2d: Option<ID2D1Factory>,
    dw: Option<IDWriteFactory>,
    fmt: Option<IDWriteTextFormat>,
    fmt_s: Option<IDWriteTextFormat>,
    rt: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    checker: Option<ID2D1BitmapBrush>,
    // images
    cur: Option<Img>,
    cur_path: PathBuf,
    err: Option<String>,
    loading: bool,
    files: Vec<PathBuf>,
    idx: usize,
    cache: Vec<(PathBuf, Img)>,
    pending: HashSet<PathBuf>,
    want_gen: u64,
    dir_gen: u64,
    last_dir: i32,
    // view
    zoom: f32,
    pan: (f32, f32),
    fit: bool,
    rot: u8,
    flip: bool,
    bgmode: u8,
    smooth: u8,
    channel: u8,
    // ui
    show_info: bool,
    show_film: bool,
    show_hud: bool,
    show_pixel: bool,
    show_hist: bool,
    show_help: bool,
    fs: bool,
    placement: WINDOWPLACEMENT,
    mouse: (i32, i32),
    drag: Option<(i32, i32)>,
    measuring: bool,
    measure: Option<((f32, f32), (f32, f32))>,
    // thumbs
    thumbs: HashMap<usize, Thumb>,
    thumb_req: HashSet<usize>,
    thumb_tx: Option<mpsc::Sender<(usize, PathBuf)>>,
    logged: bool,
    u: Ui,
}

static mut APP: *mut App = std::ptr::null_mut();
fn app() -> &'static mut App { unsafe { &mut *APP } }

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Some(a) = args.first() {
            if a == "--register" { shell::register(); return; }
            if a == "--unregister" { shell::unregister(); return; }
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(|a| { let p = PathBuf::from(a); if p.is_relative() { cwd.join(p) } else { p } }).collect();
        let start = paths.iter().find(|p| !p.is_dir()).cloned();

        // Kick off the first decode before anything else; it runs while the window is being built.
        if let Some(p) = &start {
            spawn_decode(p.clone(), 1, false);
        }
        if paths.len() == 1 && start.is_some() { spawn_list(paths[0].clone(), 1); }
        else if !paths.is_empty() { spawn_list_multi(paths.clone(), 1); }

        let cfg = Cfg::load();
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let inst = GetModuleHandleW(None).unwrap();
        let bg = cfg.color("bg");
        let brush = CreateSolidBrush(COLORREF(
            ((bg.b * 255.0) as u32) << 16 | ((bg.g * 255.0) as u32) << 8 | (bg.r * 255.0) as u32,
        ));
        let cls = w!("FastViewerWnd");
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: brush,
            lpszClassName: cls,
            hIcon: LoadIconW(Some(inst.into()), PCWSTR(1 as *const u16)).or_else(|_| LoadIconW(None, IDI_APPLICATION)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let sdpi = GetDpiForSystem() as f32 / 96.0;
        let (ww, hh) = (cfg.num("width") * sdpi, cfg.num("height") * sdpi);
        let sw = GetSystemMetrics(SM_CXSCREEN) as f32;
        let sh = GetSystemMetrics(SM_CYSCREEN) as f32;
        let (ww, hh) = (ww.min(sw * 0.95), hh.min(sh * 0.9));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0), cls, w!("FastViewer"), WS_OVERLAPPEDWINDOW,
            ((sw - ww) / 2.0) as i32, ((sh - hh) / 2.0) as i32, ww as i32, hh as i32,
            None, None, Some(inst.into()), None,
        ).unwrap();
        HWND_RAW.store(hwnd.0 as isize, Ordering::SeqCst);
        let dark = BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const c_void, 4);
        DragAcceptFiles(hwnd, true);
        if testing() {
            // test runs must not steal focus or sit on top of the user's windows
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        } else {
            let _ = ShowWindow(hwnd, if cfg.flag("maximized") { SW_SHOWMAXIMIZED } else { SW_SHOW });
        }

        shell::log_timing("window shown");
        let ui0 = GetDpiForWindow(hwnd) as f32 / 96.0;
        spawn_gpu(hwnd, cfg.text("font"), cfg.num("font_size") * ui0);
        let binds = cfg.bindings();
        let a = Box::new(App {
            hwnd, ui: ui0, d2d: None, dw: None, fmt: None, fmt_s: None, rt: None, brush: None, checker: None,
            cur: None, cur_path: start.clone().unwrap_or_default(), err: None, loading: start.is_some(),
            files: vec![], idx: 0, cache: vec![], pending: HashSet::new(), want_gen: 1, dir_gen: 1, last_dir: 1,
            zoom: 1.0, pan: (0.0, 0.0), fit: true, rot: 0, flip: false,
            bgmode: 0, smooth: 0, channel: 0,
            show_info: cfg.flag("show_info"), show_film: cfg.flag("show_gallery"), show_hud: cfg.flag("show_hud"),
            show_pixel: false, show_hist: false, show_help: false, fs: false, placement: WINDOWPLACEMENT::default(),
            mouse: (0, 0), drag: None, measuring: false, measure: None,
            thumbs: HashMap::new(), thumb_req: HashSet::new(), thumb_tx: None, logged: false,
            u: Ui::from_cfg(&cfg),
            cfg, binds,
        });
        APP = Box::into_raw(a);
        if app().cfg.flag("fullscreen") { app().toggle_fs(); }
        update_title();
        let _ = UpdateWindow(hwnd);
        if start.is_none() { app().invalidate(); }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn update_title() {
    let a = app();
    let name = a.cur_path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let t = if name.is_empty() { "FastViewer".to_string() } else if a.files.len() > 1 {
        format!("{} ({}/{}) - FastViewer", name, a.idx + 1, a.files.len())
    } else { format!("{} - FastViewer", name) };
    unsafe { let _ = SetWindowTextW(a.hwnd, PCWSTR(wide(&t).as_ptr())); }
}

impl App {
    fn invalidate(&self) { unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); } }

    fn make_formats(&mut self) {
        let Some(dw) = self.dw.clone() else { return };
        unsafe {
            let name = wide(&self.cfg.text("font"));
            let sz = self.cfg.num("font_size") * self.ui;
            let mk = |s: f32| dw.CreateTextFormat(PCWSTR(name.as_ptr()), None, DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, s, w!("en-us")).ok();
            self.fmt = mk(sz);
            self.fmt_s = mk(sz * 0.85);
        }
    }

    // ---------- rendering resources ----------
    fn ensure_rt(&mut self) -> bool {
        if self.rt.is_some() { return true; }
        let Some(d2d) = self.d2d.clone() else { return false };
        let Some(rt) = create_rt(&d2d, self.hwnd) else { return false };
        self.adopt_rt(rt);
        true
    }

    fn adopt_rt(&mut self, rt: ID2D1HwndRenderTarget) {
        unsafe {
            self.brush = rt.CreateSolidColorBrush(&clr(1.0, 1.0, 1.0, 1.0), None).ok();
            // checkerboard tile: 2x2 pixels
            let a = self.cfg.color("checker_a");
            let b = self.cfg.color("checker_b");
            let to8 = |c: D2D1_COLOR_F| [(c.b * 255.0) as u8, (c.g * 255.0) as u8, (c.r * 255.0) as u8, 255u8];
            let (pa, pb) = (to8(a), to8(b));
            let mut tile = Vec::with_capacity(16);
            for p in [pa, pb, pb, pa] { tile.extend_from_slice(&p); }
            if let Some(bm) = make_bitmap(&rt, 2, 2, &tile) {
                let bp = D2D1_BITMAP_BRUSH_PROPERTIES {
                    extendModeX: D2D1_EXTEND_MODE_WRAP, extendModeY: D2D1_EXTEND_MODE_WRAP,
                    interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                };
                self.checker = rt.CreateBitmapBrush(&bm, Some(&bp), None).ok();
            }
            self.rt = Some(rt);
            shell::log_timing("render target ready");
        }
    }

    fn drop_device(&mut self) {
        self.rt = None; self.brush = None; self.checker = None;
        if let Some(c) = self.cur.as_mut() { c.bmp = None; c.chan = None; }
        for (_, i) in self.cache.iter_mut() { i.bmp = None; i.chan = None; }
        for t in self.thumbs.values_mut() { t.bmp = None; }
    }

    // ---------- geometry ----------
    fn client(&self) -> (f32, f32) {
        let mut rc = RECT::default();
        unsafe { let _ = GetClientRect(self.hwnd, &mut rc); }
        ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32)
    }
    /// view rectangle (x, y, w, h) - the area available to the image
    fn view(&self) -> (f32, f32, f32, f32) {
        let v = self.layout().view;
        (v.x, v.y, v.w, v.h)
    }
    fn img_dims(&self) -> (f32, f32) {
        match &self.cur { Some(i) => (i.w as f32, i.h as f32), None => (1.0, 1.0) }
    }
    fn fit_zoom(&self) -> f32 {
        let (iw, ih) = self.img_dims();
        let (rw, rh) = if self.rot % 2 == 1 { (ih, iw) } else { (iw, ih) };
        let (_, _, vw, vh) = self.view();
        let z = (vw / rw).min(vh / rh);
        if self.cfg.flag("fit_upscale") { z } else { z.min(1.0) }
    }
    fn apply_fit(&mut self) {
        if self.fit { self.zoom = self.fit_zoom(); self.pan = (0.0, 0.0); }
    }
    /// linear part (a,b,c,d): x' = a x + c y ; y' = b x + d y
    fn lin(&self, z: f32) -> (f32, f32, f32, f32) {
        let (s, c) = match self.rot { 0 => (0.0, 1.0), 1 => (1.0, 0.0), 2 => (0.0, -1.0), _ => (-1.0, 0.0) };
        let f = if self.flip { -1.0 } else { 1.0 };
        (z * c * f, z * s * f, -z * s, z * c)
    }
    fn to_screen(&self, p: (f32, f32), z: f32, pan: (f32, f32)) -> (f32, f32) {
        let (vx, vy, vw, vh) = self.view();
        let (iw, ih) = self.img_dims();
        let (a, b, c, d) = self.lin(z);
        let (qx, qy) = (p.0 - iw / 2.0, p.1 - ih / 2.0);
        (vx + vw / 2.0 + pan.0 + a * qx + c * qy, vy + vh / 2.0 + pan.1 + b * qx + d * qy)
    }
    fn to_image(&self, s: (f32, f32)) -> (f32, f32) {
        let (vx, vy, vw, vh) = self.view();
        let (iw, ih) = self.img_dims();
        let (a, b, c, d) = self.lin(self.zoom);
        let (x, y) = (s.0 - vx - vw / 2.0 - self.pan.0, s.1 - vy - vh / 2.0 - self.pan.1);
        let det = a * d - b * c;
        ((d * x - c * y) / det + iw / 2.0, (-b * x + a * y) / det + ih / 2.0)
    }
    fn zoom_at(&mut self, s: (f32, f32), nz: f32) {
        let nz = nz.clamp(0.02, 128.0);
        let p = self.to_image(s);
        let (vx, vy, vw, vh) = self.view();
        let (iw, ih) = self.img_dims();
        let (a, b, c, d) = self.lin(nz);
        let (qx, qy) = (p.0 - iw / 2.0, p.1 - ih / 2.0);
        self.pan = (s.0 - vx - vw / 2.0 - (a * qx + c * qy), s.1 - vy - vh / 2.0 - (b * qx + d * qy));
        self.zoom = nz;
        self.fit = false;
    }

    // ---------- navigation / loading ----------
    /// Open a dropped or passed list of files and folders. Any size: the gallery becomes exactly that list.
    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() { return; }
        if paths.len() == 1 && !paths[0].is_dir() { self.open_path(paths[0].clone(), true); return; }
        self.dir_gen += 1;
        self.files.clear();
        self.thumbs.clear();
        self.thumb_req.clear();
        self.thumb_tx = None;
        self.measure = None;
        self.cur_path = PathBuf::new();
        if let Some(first) = paths.iter().find(|p| !p.is_dir()).cloned() {
            self.cur_path = first.clone();
            self.request(first);
        }
        spawn_list_multi(paths, self.dir_gen);
    }
    fn open_path(&mut self, p: PathBuf, relist: bool) {
        self.cur_path = p.clone();
        self.measure = None;
        if relist {
            self.dir_gen += 1;
            self.files.clear();
            self.thumbs.clear();
            self.thumb_req.clear();
            self.thumb_tx = None;
            spawn_list(p.clone(), self.dir_gen);
        }
        self.request(p);
    }
    fn request(&mut self, p: PathBuf) {
        self.err = None;
        if let Some(pos) = self.cache.iter().position(|(cp, _)| *cp == p) {
            let (_, img) = self.cache.remove(pos);
            self.show(img);
        } else {
            self.want_gen += 1;
            self.loading = true;
            LATEST.store(self.want_gen, Ordering::SeqCst);
            self.pending.insert(p.clone());
            spawn_decode(p, self.want_gen, false);
            self.invalidate();
        }
        update_title();
    }
    fn go(&mut self, idx: isize) {
        let n = self.files.len() as isize;
        if n == 0 { return; }
        let i = if self.cfg.flag("wrap") { idx.rem_euclid(n) } else { idx.clamp(0, n - 1) } as usize;
        if i == self.idx && !self.cur_path.as_os_str().is_empty() && self.cur.is_some() { return; }
        self.last_dir = if (i as isize) < self.idx as isize { -1 } else { 1 };
        self.idx = i;
        let p = self.files[i].clone();
        self.cur_path = p.clone();
        self.measure = None;
        self.request(p);
        self.ensure_visible();
    }
    fn show(&mut self, img: Img) {
        let id = ANIM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        ANIM_PAUSED.store(false, Ordering::SeqCst);
        if img.anim { spawn_anim(self.cur_path.clone(), id); }
        self.cur = Some(img);
        self.loading = false;
        self.fit = true;
        self.rot = 0;
        self.flip = false;
        self.channel = 0;
        self.apply_fit();
        self.prefetch();
        update_title();
        self.invalidate();
    }
    fn prefetch(&mut self) {
        if self.files.len() < 2 { return; }
        let n = self.files.len() as isize;
        let mut want = vec![];
        for k in [self.last_dir as isize, -(self.last_dir as isize), 2 * self.last_dir as isize] {
            let i = (self.idx as isize + k).rem_euclid(n) as usize;
            want.push(self.files[i].clone());
        }
        // keep only the wanted ones in cache
        self.cache.retain(|(p, _)| want.contains(p));
        for p in want {
            if p == self.cur_path || self.pending.contains(&p) || self.cache.iter().any(|(c, _)| *c == p) { continue; }
            self.pending.insert(p.clone());
            spawn_decode(p, 0, true);
        }
    }

    fn on_loaded(&mut self, l: Box<Loaded>) {
        self.pending.remove(&l.path);
        let is_cur = l.path == self.cur_path;
        match l.res {
            Ok(raw) => {
                let img = Img::from_raw(raw);
                if is_cur && self.cur_is_stale_for(&l.path) { self.show(img); }
                else if !self.cache.iter().any(|(p, _)| *p == l.path) && self.cache.len() < 4 { self.cache.push((l.path, img)); }
            }
            Err(e) => {
                if is_cur && self.loading {
                    self.loading = false;
                    self.cur = None;
                    self.err = Some(e);
                    self.invalidate();
                }
            }
        }
    }
    /// true if we are still waiting on `p` (i.e. it is not already displayed)
    fn cur_is_stale_for(&self, _p: &Path) -> bool { self.loading || self.cur.is_none() }

    fn on_files(&mut self, gen: u64, files: Vec<PathBuf>) {
        if gen != self.dir_gen { return; }
        let me = self.cur_path.to_string_lossy().to_lowercase();
        self.idx = files.iter().position(|f| f.to_string_lossy().to_lowercase() == me).unwrap_or(0);
        self.files = files;
        if self.cur_path.as_os_str().is_empty() && !self.files.is_empty() {
            let p = self.files[0].clone();
            self.cur_path = p.clone();
            self.request(p);
        }
        self.start_thumbs();
        self.ensure_visible();
        self.prefetch();
        update_title();
        self.invalidate();
    }

    // ---------- commands ----------
    fn dispatch(&mut self, act: &str) {
        if self.ui_action(act) { self.invalidate(); return; }
        let mid = {
            let (vx, vy, vw, vh) = self.view();
            (vx + vw / 2.0, vy + vh / 2.0)
        };
        let step = self.cfg.num("zoom_step");
        match act {
            "next" => self.go(self.idx as isize + 1),
            "prev" => self.go(self.idx as isize - 1),
            "first" => self.go(0),
            "last" => self.go(self.files.len() as isize - 1),
            "zoom_in" => self.zoom_at(mid, self.zoom * step),
            "zoom_out" => self.zoom_at(mid, self.zoom / step),
            "fit" => { self.fit = true; self.apply_fit(); }
            "actual" => { if (self.zoom - 1.0).abs() < 0.001 && !self.fit { self.fit = true; self.apply_fit(); } else { self.zoom_at(mid, 1.0); } }
            "fill" => { let (iw, ih) = self.img_dims(); let (rw, rh) = if self.rot % 2 == 1 { (ih, iw) } else { (iw, ih) };
                        let (_, _, vw, vh) = self.view(); self.zoom_at(mid, (vw / rw).max(vh / rh)); self.pan = (0.0, 0.0); }
            "fullscreen" => self.toggle_fs(),
            "info" => { self.show_info = !self.show_info; self.apply_fit(); }
            "gallery" => { self.show_film = !self.show_film; if !self.show_film { self.u.browse = false; } self.apply_fit(); self.ensure_visible(); }
            "hud" => self.show_hud = !self.show_hud,
            "pixel" => self.show_pixel = !self.show_pixel,
            "histogram" => self.show_hist = !self.show_hist,
            "help" => self.show_help = !self.show_help,
            "animate" => { let p = ANIM_PAUSED.load(Ordering::SeqCst); ANIM_PAUSED.store(!p, Ordering::SeqCst); }
            "step" => { ANIM_PAUSED.store(true, Ordering::SeqCst); ANIM_STEP.store(true, Ordering::SeqCst); }
            "channel" => { self.channel = (self.channel + 1) % 6; }
            "rotate_cw" => { self.rot = (self.rot + 1) % 4; self.apply_fit(); }
            "rotate_ccw" => { self.rot = (self.rot + 3) % 4; self.apply_fit(); }
            "flip" => { self.flip = !self.flip; }
            "background" => self.bgmode = (self.bgmode + 1) % 4,
            "smooth" => self.smooth = (self.smooth + 1) % 3,
            "delete" => self.delete_current(),
            "copy_path" => shell::set_clipboard_text(self.hwnd, &self.cur_path.to_string_lossy()),
            "copy_image" => if let Some(i) = &self.cur { shell::set_clipboard_image(self.hwnd, i.w, i.h, &i.px) },
            "reveal" => shell::reveal(&self.cur_path),
            "open" => if let Some(p) = shell::open_dialog(self.hwnd) { self.open_path(p, true) },
            "config" => shell::open_config(),
            "quit" => {
                if self.measure.is_some() { self.measure = None; }
                else if self.show_help { self.show_help = false; }
                else if self.u.browse { self.u.browse = false; self.apply_fit(); }
                else if self.u.i_edit { self.u.i_edit = false; }
                else if self.fs { self.toggle_fs(); }
                else { unsafe { let _ = DestroyWindow(self.hwnd); } }
            }
            _ => {}
        }
        self.invalidate();
    }

    fn delete_current(&mut self) {
        if self.cur_path.as_os_str().is_empty() { return; }
        if !shell::recycle(self.hwnd, &self.cur_path) { return; }
        let removed = self.idx;
        if removed < self.files.len() { self.files.remove(removed); }
        self.thumbs.clear(); self.thumb_req.clear();
        self.cur = None;
        if self.files.is_empty() {
            self.cur_path = PathBuf::new(); self.loading = false; update_title(); return;
        }
        let i = removed.min(self.files.len() - 1);
        self.idx = i;
        let p = self.files[i].clone();
        self.cur_path = p.clone();
        self.request(p);
    }

    fn toggle_fs(&mut self) {
        unsafe {
            if !self.fs {
                self.placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                let _ = GetWindowPlacement(self.hwnd, &mut self.placement);
                let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as isize));
                let r = mi.rcMonitor;
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_NOOWNERZORDER);
                self.fs = true;
            } else {
                let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, style | WS_OVERLAPPEDWINDOW.0 as isize);
                let _ = SetWindowPlacement(self.hwnd, &self.placement);
                let _ = SetWindowPos(self.hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
                self.fs = false;
            }
        }
    }

    // ---------- painting ----------
    fn paint(&mut self, hdc: HDC) {
        if self.d2d.is_none() { unsafe { self.paint_gdi(hdc) }; return; }
        if !self.ensure_rt() { return; }
        let rt: ID2D1HwndRenderTarget = self.rt.clone().unwrap();
        let br = self.brush.clone().unwrap();
        unsafe {
            rt.BeginDraw();
            let bg = match self.bgmode { 1 => self.cfg.color("bg_light"), 3 => clr(0.0, 0.0, 0.0, 1.0), _ => self.cfg.color("bg") };
            rt.Clear(Some(&bg));
            if !self.u.browse {
                self.draw_image(&rt, &br);
                if self.cur.is_some() && !self.logged { self.logged = true; shell::log_timing("first image painted"); }
                self.draw_overlays(&rt, &br);
            }
            self.draw_gallery(&rt, &br);
            self.draw_info(&rt, &br);
            self.draw_toolbar(&rt, &br);
            if !self.u.browse { self.draw_hud(&rt, &br); }
            self.draw_tooltip(&rt, &br);
            if self.show_help { self.draw_help(&rt, &br); }
            if let Err(e) = rt.EndDraw(None, None) {
                if e.code() == HRESULT(0x8899000Cu32 as i32) { self.drop_device(); self.invalidate(); }
            }
        }
    }

    fn ensure_bitmaps(&mut self, rt: &ID2D1HwndRenderTarget) {
        let ch = self.channel;
        let Some(img) = self.cur.as_mut() else { return };
        if img.bmp.is_none() { img.bmp = make_bitmap(rt, img.w, img.h, &img.px); }
        if ch != 0 && img.chan.as_ref().map(|c| c.0) != Some(ch) {
            let g = channel_pixels(&img.px, ch);
            img.chan = make_bitmap(rt, img.w, img.h, &g).map(|b| (ch, b));
        }
    }

    unsafe fn draw_image(&mut self, rt: &ID2D1HwndRenderTarget, _br: &ID2D1SolidColorBrush) {
        self.ensure_bitmaps(rt);
        let (vx, vy, vw, vh) = self.view();
        let Some(img) = self.cur.as_ref() else { return };
        let bmp = if self.channel != 0 { img.chan.as_ref().map(|c| c.1.clone()).or(img.bmp.clone()) } else { img.bmp.clone() };
        let Some(bmp) = bmp else { return };
        rt.PushAxisAlignedClip(&rc(vx, vy, vx + vw, vy + vh), D2D1_ANTIALIAS_MODE_ALIASED);
        let (a, b, c, d) = self.lin(self.zoom);
        let (iw, ih) = (img.w as f32, img.h as f32);
        let t0 = self.to_screen((0.0, 0.0), self.zoom, self.pan);
        let _ = (a, b, c, d);
        let m = Matrix3x2 { M11: a, M12: b, M21: c, M22: d, M31: t0.0, M32: t0.1 };
        rt.SetTransform(&m);
        let dest = rc(0.0, 0.0, iw, ih);
        if self.bgmode == 2 {
            if let Some(ck) = &self.checker {
                // 8 screen px squares: tile is 2x2 px, so scale by 8/zoom
                let s = 8.0 / self.zoom;
                ck.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
                rt.FillRectangle(&dest, ck);
            }
        }
        let nearest = self.smooth == 2 || (self.smooth == 0 && self.zoom >= 3.0);
        let interp = if nearest { D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR } else { D2D1_BITMAP_INTERPOLATION_MODE_LINEAR };
        rt.DrawBitmap(&bmp, Some(&dest), 1.0, interp, None);
        rt.SetTransform(&Matrix3x2::identity());
        rt.PopAxisAlignedClip();
    }

    unsafe fn text(&self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush, s: &str, x: f32, y: f32, w: f32, col: D2D1_COLOR_F, small: bool) -> f32 {
        let fmt = if small { self.fmt_s.as_ref() } else { self.fmt.as_ref() };
        let Some(fmt) = fmt else { return 0.0 };
        let ws: Vec<u16> = s.encode_utf16().collect();
        let Some(dw) = &self.dw else { return 0.0 };
        let Ok(lay) = dw.CreateTextLayout(&ws, fmt, w, 4000.0) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = lay.GetMetrics(&mut m);
        br.SetColor(&col);
        rt.DrawTextLayout(Vector2 { X: x, Y: y }, &lay, br, D2D1_DRAW_TEXT_OPTIONS_CLIP);
        m.height
    }
    unsafe fn text_w(&self, s: &str, small: bool) -> (f32, f32) {
        let fmt = if small { self.fmt_s.as_ref() } else { self.fmt.as_ref() };
        let Some(fmt) = fmt else { return (0.0, 0.0) };
        let ws: Vec<u16> = s.encode_utf16().collect();
        let Some(dw) = &self.dw else { return (0.0, 0.0) };
        let Ok(lay) = dw.CreateTextLayout(&ws, fmt, 4000.0, 4000.0) else { return (0.0, 0.0) };
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = lay.GetMetrics(&mut m);
        (m.widthIncludingTrailingWhitespace, m.height)
    }
    unsafe fn plate(&self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush, r: D2D_RECT_F) {
        br.SetColor(&self.cfg.color_a("panel", 0.82));
        rt.FillRectangle(&r, br);
    }

    unsafe fn draw_hud(&mut self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let ui = self.ui;
        let fg = self.cfg.color("text");
        let dim = self.cfg.color("text_dim");
        let mut lines: Vec<String> = vec![];
        if self.cur_path.as_os_str().is_empty() {
            lines.push("Drop an image here, or press Ctrl+O to open. F1 for help.".into());
        } else if let Some(e) = &self.err {
            lines.push(format!("Cannot open: {}", e));
        } else if self.show_hud {
            let name = self.cur_path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let mut l = name;
            if self.files.len() > 1 { l += &format!("   {}/{}", self.idx + 1, self.files.len()); }
            if let Some(i) = &self.cur {
                l += &format!("   {}x{}   {:.0}%", i.w, i.h, self.zoom * 100.0);
                if i.anim { l += &format!("   frame {}/{}{}", i.frame.0 + 1, i.frame.1.max(1), if ANIM_PAUSED.load(Ordering::SeqCst) { " (paused)" } else { "" }); }
            }
            if self.loading { l += "   loading..."; }
            lines.push(l);
        } else if self.loading { lines.push("loading...".into()); }
        if self.show_pixel {
            if let Some(i) = &self.cur {
                let p = self.to_image((self.mouse.0 as f32, self.mouse.1 as f32));
                let (x, y) = (p.0.floor() as i64, p.1.floor() as i64);
                if x >= 0 && y >= 0 && (x as u32) < i.w && (y as u32) < i.h {
                    let o = ((y as u32 * i.w + x as u32) * 4) as usize;
                    let (b, g, r, a) = (i.px[o] as u32, i.px[o + 1] as u32, i.px[o + 2] as u32, i.px[o + 3] as u32);
                    let un = |c: u32| if a == 0 { 0 } else { (c * 255 / a).min(255) };
                    let (r, g, b) = (un(r), un(g), un(b));
                    lines.push(format!("x {}  y {}    #{:02X}{:02X}{:02X}    rgba({}, {}, {}, {})", x, y, r, g, b, r, g, b, a));
                } else { lines.push("outside image".into()); }
            }
        }
        if let Some((a, b)) = self.measure {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            lines.push(format!("measure  dx {:.1}  dy {:.1}  dist {:.2}px  angle {:.1} deg", dx, dy, (dx * dx + dy * dy).sqrt(), (-dy).atan2(dx).to_degrees()));
        }
        if self.channel != 0 {
            let n = ["", "Red", "Green", "Blue", "Alpha", "Luma"][self.channel as usize];
            lines.push(format!("channel: {}", n));
        }
        if lines.is_empty() { return; }
        let mut wmax: f32 = 0.0; let mut ht = 0.0;
        for l in &lines { let (w, h) = self.text_w(l, false); wmax = wmax.max(w); ht += h; }
        let vr = self.layout().view;
        let (pad, x0, y0) = (7.0 * ui, vr.x + 8.0 * ui, vr.y + 8.0 * ui);
        self.plate(rt, br, rc(x0, y0, x0 + wmax + pad * 2.0, y0 + ht + pad * 2.0));
        let mut y = y0 + pad;
        for (i, l) in lines.iter().enumerate() {
            let h = self.text(rt, br, l, x0 + pad, y, wmax + 4.0, if i == 0 { fg } else { dim }, false);
            y += h;
        }
    }

    unsafe fn draw_overlays(&mut self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let ui = self.ui;
        let (vx, vy, vw, vh) = self.view();
        rt.PushAxisAlignedClip(&rc(vx, vy, vx + vw, vy + vh), D2D1_ANTIALIAS_MODE_ALIASED);
        // measurement line
        if let Some((a, b)) = self.measure {
            let (sa, sb) = (self.to_screen(a, self.zoom, self.pan), self.to_screen(b, self.zoom, self.pan));
            br.SetColor(&self.cfg.color("accent"));
            rt.DrawLine(Vector2 { X: sa.0, Y: sa.1 }, Vector2 { X: sb.0, Y: sb.1 }, br, 1.5 * ui, None);
            for s in [sa, sb] { rt.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: s.0, Y: s.1 }, radiusX: 4.0 * ui, radiusY: 4.0 * ui }, br, 1.5 * ui, None); }
        }
        // loupe
        if self.show_pixel {
            if let Some(i) = &self.cur {
                let p = self.to_image((self.mouse.0 as f32, self.mouse.1 as f32));
                let (cx, cy) = (p.0.floor() as i64, p.1.floor() as i64);
                if p.0 >= 0.0 && p.1 >= 0.0 && p.0 < i.w as f32 && p.1 < i.h as f32 {
                    let n = 11i64;
                    let cell = 12.0 * ui;
                    let size = cell * n as f32;
                    let mut lx = self.mouse.0 as f32 + 24.0 * ui;
                    let mut ly = self.mouse.1 as f32 + 24.0 * ui;
                    if lx + size > vx + vw { lx = self.mouse.0 as f32 - 24.0 * ui - size; }
                    if ly + size > vy + vh { ly = self.mouse.1 as f32 - 24.0 * ui - size; }
                    br.SetColor(&clr(0.0, 0.0, 0.0, 1.0));
                    rt.FillRectangle(&rc(lx - 2.0, ly - 2.0, lx + size + 2.0, ly + size + 2.0), br);
                    for j in 0..n {
                        for k in 0..n {
                            let (px, py) = (cx + k - n / 2, cy + j - n / 2);
                            if px < 0 || py < 0 || px >= i.w as i64 || py >= i.h as i64 { continue; }
                            let o = ((py as u32 * i.w + px as u32) * 4) as usize;
                            let a = i.px[o + 3] as f32 / 255.0;
                            let un = |c: u8| if a == 0.0 { 0.0 } else { (c as f32 / 255.0 / a).min(1.0) };
                            br.SetColor(&clr(un(i.px[o + 2]), un(i.px[o + 1]), un(i.px[o]), 1.0));
                            let (x, y) = (lx + k as f32 * cell, ly + j as f32 * cell);
                            rt.FillRectangle(&rc(x, y, x + cell, y + cell), br);
                        }
                    }
                    let (x, y) = (lx + (n / 2) as f32 * cell, ly + (n / 2) as f32 * cell);
                    br.SetColor(&clr(1.0, 1.0, 1.0, 1.0));
                    rt.DrawRectangle(&rc(x, y, x + cell, y + cell), br, 1.5 * ui, None);
                    br.SetColor(&clr(0.0, 0.0, 0.0, 1.0));
                    rt.DrawRectangle(&rc(x - 1.5, y - 1.5, x + cell + 1.5, y + cell + 1.5), br, 1.0, None);
                }
            }
        }
        // histogram
        if self.show_hist {
            if let Some(i) = self.cur.as_mut() {
                if i.hist.is_none() { i.hist = Some(histogram(&i.px)); }
            }
            if let Some(h) = self.cur.as_ref().and_then(|i| i.hist.as_ref()) {
                let (hw, hh) = (256.0 * ui * 1.0, 110.0 * ui);
                let (x0, y0) = (vx + 10.0 * ui, vy + vh - hh - 10.0 * ui);
                self.plate(rt, br, rc(x0 - 6.0 * ui, y0 - 6.0 * ui, x0 + hw + 6.0 * ui, y0 + hh + 6.0 * ui));
                let mut mx = 1u32;
                for c in 0..4 { for b in 1..255 { mx = mx.max(h[c][b]); } }
                let cols = [clr(1.0, 0.25, 0.25, 0.55), clr(0.25, 1.0, 0.25, 0.55), clr(0.35, 0.5, 1.0, 0.65), clr(1.0, 1.0, 1.0, 0.25)];
                for c in [3usize, 0, 1, 2] {
                    br.SetColor(&cols[c]);
                    for b in 0..256 {
                        let v = ((h[c][b] as f32 / mx as f32).min(1.0)).sqrt() * hh;
                        let x = x0 + b as f32 * hw / 256.0;
                        rt.FillRectangle(&rc(x, y0 + hh - v, x + hw / 256.0 + 0.5, y0 + hh), br);
                    }
                }
            }
        }
        rt.PopAxisAlignedClip();
    }

    unsafe fn draw_help(&mut self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let ui = self.ui;
        let (cw, ch) = self.client();
        let (fg, dim) = (self.cfg.color("text"), self.cfg.color("text_dim"));
        let rows: Vec<(String, &str)> = ACTIONS.iter().map(|(a, d)| (self.cfg.keys_for(a), *d)).collect();
        let lh = 19.0 * ui;
        // as many columns as needed to fit the window height
        let per_col = (((ch - 130.0 * ui) / lh).floor() as usize).max(8);
        let cols = ((rows.len() + per_col - 1) / per_col).max(1);
        let colw = 430.0 * ui;
        let (pw, ph) = (colw * cols as f32, rows.len().min(per_col) as f32 * lh + 90.0 * ui);
        let (x0, y0) = (((cw - pw) / 2.0).max(4.0), ((ch - ph) / 2.0).max(8.0));
        br.SetColor(&self.cfg.color_a("panel", 0.97));
        rt.FillRectangle(&rc(x0, y0, x0 + pw, y0 + ph), br);
        self.text(rt, br, "Shortcuts   (edit fastviewer.ini to change - F2)", x0 + 16.0 * ui, y0 + 12.0 * ui, pw, fg, false);
        for (i, (k, d)) in rows.into_iter().enumerate() {
            let (c, r) = (i / per_col, i % per_col);
            let (x, y) = (x0 + c as f32 * colw, y0 + 44.0 * ui + r as f32 * lh);
            self.text(rt, br, &k, x + 16.0 * ui, y, 160.0 * ui, fg, true);
            self.text(rt, br, d, x + 180.0 * ui, y, colw - 190.0 * ui, dim, true);
        }
        self.text(rt, br, "Mouse: wheel zoom - drag pan - double-click 100%/fit - Shift+drag measure - drag panel edges to resize", x0 + 16.0 * ui, y0 + ph - 28.0 * ui, pw - 20.0 * ui, dim, true);
    }
}

fn mods_now() -> u8 {
    let o = MODS.load(Ordering::SeqCst);
    if o >= 0 { return o as u8; }
    unsafe {
        let down = |vk: VIRTUAL_KEY| (GetKeyState(vk.0 as i32) as u16 & 0x8000) != 0;
        (down(VK_CONTROL) as u8) | ((down(VK_SHIFT) as u8) << 1) | ((down(VK_MENU) as u8) << 2)
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if APP.is_null() { return DefWindowProcW(hwnd, msg, wp, lp); }
        let a = app();
        let (mx, my) = ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                a.paint(ps.hdc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                if let Some(rt) = &a.rt {
                    let _ = rt.Resize(&D2D_SIZE_U { width: (lp.0 & 0xFFFF).max(1) as u32, height: ((lp.0 >> 16) & 0xFFFF).max(1) as u32 });
                }
                a.apply_fit();
                a.invalidate();
                LRESULT(0)
            }
            WM_DPICHANGED => {
                a.ui = (wp.0 & 0xFFFF) as f32 / 96.0;
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                a.make_formats();
                LRESULT(0)
            }
            WM_ANIM => {
                let b = Box::from_raw(lp.0 as *mut (u64, u32, u32, Vec<u8>));
                if b.0 == ANIM_GEN.load(Ordering::SeqCst) {
                    if let Some(img) = a.cur.as_mut() {
                        if b.3.len() == img.px.len() {
                            img.px = b.3;
                            img.frame = (b.1, b.2);
                            img.hist = None;
                            img.chan = None;
                            if let Some(bm) = &img.bmp { let _ = bm.CopyFromMemory(None, img.px.as_ptr() as *const c_void, img.w * 4); }
                            a.invalidate();
                        }
                    }
                }
                LRESULT(0)
            }
            WM_TEST_MODS if testing() => { MODS.store(wp.0 as i32, Ordering::SeqCst); LRESULT(0) }
            WM_TEST_DROP if testing() => { test_drop(hwnd); LRESULT(0) }
            WM_TEST_DUMP if testing() => { a.dump_state(); LRESULT(0) }
            WM_GPU => { a.on_gpu(*Box::from_raw(lp.0 as *mut Gpu)); LRESULT(0) }
            WM_LOADED => { shell::log_timing("WM_LOADED received"); a.on_loaded(Box::from_raw(lp.0 as *mut Loaded)); LRESULT(0) }
            WM_FILES => {
                let b = Box::from_raw(lp.0 as *mut (u64, Vec<PathBuf>));
                a.on_files(b.0, b.1);
                LRESULT(0)
            }
            WM_THUMB => {
                let b = Box::from_raw(lp.0 as *mut (u64, usize, Raw));
                if b.0 == a.dir_gen {
                    let r = b.2;
                    a.thumbs.insert(b.1, Thumb { w: r.w, h: r.h, px: r.px, bmp: None });
                    if a.show_film { a.invalidate(); }
                    if a.u.g_scroll < 0.0 { a.u.g_scroll = 0.0; }
                }
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let m = mods_now();
                let vk = wp.0 as u32;
                if a.u.browse && m == 0 && (vk == 0x26 || vk == 0x28) { a.browse_move(if vk == 0x26 { -1 } else { 1 }); a.invalidate(); return LRESULT(0); }
                let found = a.binds.iter().find(|(k, bm, _)| {
                    *k == vk && (*bm == m || (m & !2 == *bm && (vk == 0xBB || vk == 0xBD || vk == 0x6B || vk == 0x6D)))
                }).map(|b| b.2.clone());
                if let Some(act) = found { a.dispatch(&act); return LRESULT(0); }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
                let mut pt = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut pt);
                let ctrl = mods_now() & 1 != 0;
                if a.ui_wheel(pt.x as f32, pt.y as f32, delta, ctrl) { return LRESULT(0); }
                if (a.cfg.text("wheel") == "navigate") != ctrl {
                    a.go(a.idx as isize - delta.signum() as isize);
                } else {
                    let nz = a.zoom * a.cfg.num("zoom_step").powf(delta);
                    a.zoom_at((pt.x as f32, pt.y as f32), nz);
                    a.invalidate();
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if a.ui_mouse_down(mx as f32, my as f32, false) { return LRESULT(0); }
                SetCapture(hwnd);
                if mods_now() & 2 != 0 {
                    let p = a.to_image((mx as f32, my as f32));
                    a.measure = Some((p, p));
                    a.measuring = true;
                } else {
                    a.measure = None;
                    a.drag = Some((mx, my));
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if a.ui_mouse_up() { return LRESULT(0); }
                let _ = ReleaseCapture();
                a.drag = None; a.measuring = false;
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                if a.ui_mouse_down(mx as f32, my as f32, true) { return LRESULT(0); }
                a.dispatch("actual");
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if (lp.0 & 0xFFFF) as u32 == HTCLIENT {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                    let c = a.cursor_for(pt.x as f32, pt.y as f32);
                    SetCursor(Some(LoadCursorW(None, c).unwrap_or_default()));
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MBUTTONDOWN => { a.dispatch("fit"); LRESULT(0) }
            WM_MOUSEMOVE => {
                a.mouse = (mx, my);
                if a.ui_mouse_move(mx as f32, my as f32) { return LRESULT(0); }
                if let Some((lx, ly)) = a.drag {
                    a.pan.0 += (mx - lx) as f32;
                    a.pan.1 += (my - ly) as f32;
                    a.drag = Some((mx, my));
                    a.fit = false;
                    a.invalidate();
                } else if a.measuring {
                    let p = a.to_image((mx as f32, my as f32));
                    if let Some((s, _)) = a.measure { a.measure = Some((s, p)); }
                    a.invalidate();
                } else if a.show_pixel { a.invalidate(); }
                LRESULT(0)
            }
            WM_DROPFILES => {
                let hd = HDROP(wp.0 as *mut c_void);
                let paths = read_drop(hd);
                DragFinish(hd);
                a.open_paths(paths);
                LRESULT(0)
            }
            WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

pub fn make_bitmap(rt: &ID2D1HwndRenderTarget, w: u32, h: u32, px: &[u8]) -> Option<ID2D1Bitmap> {
    unsafe {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0, dpiY: 96.0,
        };
        rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(px.as_ptr() as *const c_void), w * 4, &props).ok()
    }
}

pub fn rc(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F { D2D_RECT_F { left: l, top: t, right: r, bottom: b } }
pub fn clr(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F { D2D1_COLOR_F { r, g, b, a } }
pub fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn channel_pixels(px: &[u8], mode: u8) -> Vec<u8> {
    let mut out = vec![255u8; px.len()];
    for (i, o) in px.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let (b, g, r, a) = (i[0] as u32, i[1] as u32, i[2] as u32, i[3] as u32);
        let un = |c: u32| if a == 0 { 0 } else { (c * 255 / a).min(255) };
        let v = match mode {
            1 => un(r), 2 => un(g), 3 => un(b), 4 => a,
            _ => (54 * un(r) + 183 * un(g) + 19 * un(b)) >> 8,
        } as u8;
        o[0] = v; o[1] = v; o[2] = v;
    }
    out
}

fn histogram(px: &[u8]) -> Box<[[u32; 256]; 4]> {
    let mut h = Box::new([[0u32; 256]; 4]);
    for i in px.chunks_exact(4) {
        let (b, g, r, a) = (i[0] as u32, i[1] as u32, i[2] as u32, i[3] as u32);
        if a == 0 { continue; }
        let un = |c: u32| (c * 255 / a).min(255) as usize;
        let (r, g, b) = (un(r), un(g), un(b));
        h[0][r] += 1; h[1][g] += 1; h[2][b] += 1;
        h[3][((54 * r + 183 * g + 19 * b) >> 8).min(255)] += 1;
    }
    h
}

// ---------- background GPU initialisation ----------
// Creating the Direct2D render target initialises the GPU driver (~250 ms on some systems).
// It runs on a worker thread so the window can show the first image (via GDI) in the meantime.
struct Gpu { d2d: ID2D1Factory, dw: IDWriteFactory, rt: Option<ID2D1HwndRenderTarget> }
unsafe impl Send for Gpu {}
const WM_GPU: u32 = WM_APP + 4;

pub fn create_rt(d2d: &ID2D1Factory, hwnd: HWND) -> Option<ID2D1HwndRenderTarget> {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let rtp = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_UNKNOWN, alphaMode: D2D1_ALPHA_MODE_UNKNOWN },
            dpiX: 96.0, dpiY: 96.0, usage: D2D1_RENDER_TARGET_USAGE_NONE, minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U { width: (rc.right - rc.left).max(1) as u32, height: (rc.bottom - rc.top).max(1) as u32 },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        d2d.CreateHwndRenderTarget(&rtp, &hp).ok()
    }
}

fn spawn_gpu(hwnd: HWND, font: String, size: f32) {
    let h = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let hwnd = HWND(h as *mut c_void);
        let d2d: Option<ID2D1Factory> = D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None).ok();
        let dw: Option<IDWriteFactory> = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok();
        let (Some(d2d), Some(dw)) = (d2d, dw) else { return };
        // warm up font loading so the first text draw is instant
        if let Ok(f) = dw.CreateTextFormat(PCWSTR(wide(&font).as_ptr()), None, DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us")) {
            let _ = dw.CreateTextLayout(&[65u16, 103], &f, 100.0, 100.0);
        }
        shell::log_timing("gpu: factories ready");
        let rt = create_rt(&d2d, hwnd);
        shell::log_timing("gpu: render target created");
        post(WM_GPU, Box::into_raw(Box::new(Gpu { d2d, dw, rt })) as isize);
    });
}

fn colorref(c: D2D1_COLOR_F) -> COLORREF {
    COLORREF(((c.b * 255.0) as u32) << 16 | ((c.g * 255.0) as u32) << 8 | (c.r * 255.0) as u32)
}

impl App {
    /// Fast first frame while the GPU path is still starting up. Fit view only, no overlays.
    unsafe fn paint_gdi(&mut self, hdc: HDC) {
        let (cw, ch) = self.client();
        let b = CreateSolidBrush(colorref(self.cfg.color("bg")));
        FillRect(hdc, &RECT { left: 0, top: 0, right: cw as i32, bottom: ch as i32 }, b);
        let _ = DeleteObject(b.into());
        // panels (toolbar / gallery / metadata) as plain fills until Direct2D takes over
        let pb = CreateSolidBrush(colorref(self.cfg.color("panel")));
        let lay = self.layout();
        for p in [lay.bar, lay.gal, lay.info].into_iter().flatten() {
            FillRect(hdc, &RECT { left: p.x as i32, top: p.y as i32, right: (p.x + p.w) as i32, bottom: (p.y + p.h) as i32 }, pb);
        }
        let _ = DeleteObject(pb.into());
        if self.u.browse { return; }
        if let Some(i) = &self.cur {
            if self.rot == 0 && !self.flip {
                let tl = self.to_screen((0.0, 0.0), self.zoom, self.pan);
                let bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: 40, biWidth: i.w as i32, biHeight: -(i.h as i32), biPlanes: 1, biBitCount: 32,
                        biCompression: BI_RGB.0, ..Default::default()
                    },
                    ..Default::default()
                };
                SetStretchBltMode(hdc, COLORONCOLOR);
                StretchDIBits(hdc, tl.0 as i32, tl.1 as i32, (i.w as f32 * self.zoom) as i32, (i.h as f32 * self.zoom) as i32,
                    0, 0, i.w as i32, i.h as i32, Some(i.px.as_ptr() as *const c_void), &bi, DIB_RGB_COLORS, SRCCOPY);
            }
            if !self.logged { self.logged = true; shell::log_timing("first image painted (GDI)"); }
        }
    }

    fn on_gpu(&mut self, g: Gpu) {
        self.d2d = Some(g.d2d);
        self.dw = Some(g.dw);
        self.make_formats();
        match g.rt {
            Some(rt) => {
                let (cw, ch) = self.client();
                unsafe { let _ = rt.Resize(&D2D_SIZE_U { width: cw.max(1.0) as u32, height: ch.max(1.0) as u32 }); }
                self.adopt_rt(rt);
            }
            None => {}
        }
        self.logged = false; // log the first D2D frame too
        self.invalidate();
    }
}


// ---------- test hooks (only active when FASTVIEWER_TEST is set) ----------
static MODS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
fn testing() -> bool {
    static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::env::var_os("FASTVIEWER_TEST").is_some())
}
/// Sends this window a genuine WM_DROPFILES built from the paths listed (one per line) in the file named by %FASTVIEWER_TEST_DROP%.
fn test_drop(hwnd: HWND) {
    use windows::Win32::System::Memory::*;
    let Some(list) = std::env::var("FASTVIEWER_TEST_DROP").ok().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    let mut wide_list: Vec<u16> = vec![];
    for l in list.trim_start_matches('\u{feff}').lines().filter(|l| !l.trim().is_empty()) { wide_list.extend(l.trim().encode_utf16()); wide_list.push(0); }
    wide_list.push(0);
    unsafe {
        let hdr = 20usize;
        let Ok(h) = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, hdr + wide_list.len() * 2) else { return };
        let p = GlobalLock(h) as *mut u8;
        *(p as *mut u32) = hdr as u32; // pFiles
        *(p.add(16) as *mut u32) = 1; // fWide
        std::ptr::copy_nonoverlapping(wide_list.as_ptr() as *const u8, p.add(hdr), wide_list.len() * 2);
        let _ = GlobalUnlock(h);
        let _ = PostMessageW(Some(hwnd), WM_DROPFILES, WPARAM(h.0 as usize), LPARAM(0));
    }
}

impl App {
    /// Test hook: writes the viewer's internal state as key=value lines to %FASTVIEWER_TEST_STATE%.
    fn dump_state(&mut self) {
        use std::fmt::Write;
        let Some(path) = std::env::var_os("FASTVIEWER_TEST_STATE") else { return };
        let mut s = String::new();
        let _ = writeln!(s, "file={}", self.cur_path.display());
        let _ = writeln!(s, "idx={}\ncount={}\nui={}", self.idx, self.files.len(), self.ui);
        let _ = writeln!(s, "zoom={:.4}\nfit={}\npan={:.1},{:.1}", self.zoom, self.fit, self.pan.0, self.pan.1);
        let _ = writeln!(s, "rot={}\nflip={}\nbg={}\nsmooth={}\nchannel={}", self.rot, self.flip, self.bgmode, self.smooth, self.channel);
        let _ = writeln!(s, "info={}\nfilm={}\nhud={}\npixel={}\nhist={}\nhelp={}\nfs={}", self.show_info, self.show_film, self.show_hud, self.show_pixel, self.show_hist, self.show_help, self.fs);
        let _ = writeln!(s, "loading={}\nerr={}\ngpu={}", self.loading, self.err.clone().unwrap_or_default(), self.rt.is_some());
        let _ = writeln!(s, "thumbs={}\ncache={}", self.thumbs.len(), self.cache.len());
        let _ = writeln!(s, "paused={}", ANIM_PAUSED.load(Ordering::SeqCst));
        if let Some(i) = &self.cur {
            let _ = writeln!(s, "w={}\nh={}\nanim={}\nframe={}/{}", i.w, i.h, i.anim, i.frame.0, i.frame.1);
            let p = self.to_image((self.mouse.0 as f32, self.mouse.1 as f32));
            let (x, y) = (p.0.floor() as i64, p.1.floor() as i64);
            if x >= 0 && y >= 0 && (x as u32) < i.w && (y as u32) < i.h {
                let o = ((y as u32 * i.w + x as u32) * 4) as usize;
                let _ = writeln!(s, "px={},{},{},{},{},{}", x, y, i.px[o + 2], i.px[o + 1], i.px[o], i.px[o + 3]);
            }
            // quick signature of the pixel buffer so tests can see frames change
            let mut h: u64 = 1469598103934665603;
            for b in i.px.iter().step_by(37) { h = (h ^ *b as u64).wrapping_mul(1099511628211); }
            let _ = writeln!(s, "pxhash={:x}", h);
            for (k, v) in &i.meta { let _ = writeln!(s, "meta.{}={}", k, v.replace('\n', " ")); }
        }
        if let Some((a, b)) = self.measure { let _ = writeln!(s, "measure={:.1},{:.1}->{:.1},{:.1}", a.0, a.1, b.0, b.1); }
        let (cw, ch) = self.client();
        let _ = writeln!(s, "client={}x{}", cw, ch);
        self.dump_ui(&mut s);
        let _ = std::fs::write(path, s);
    }
}

/// Reads every path from an HDROP in one linear pass. (DragQueryFileW(i) rescans from the start for
/// each index, which makes very large drops O(n^2).)
unsafe fn read_drop(hd: HDROP) -> Vec<PathBuf> {
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    let h = HGLOBAL(hd.0);
    let base = GlobalLock(h) as *const u8;
    let mut out = vec![];
    if !base.is_null() {
        let off = *(base as *const u32) as usize;
        let wide_names = *(base.add(16) as *const u32) != 0;
        let total = windows::Win32::System::Memory::GlobalSize(h);
        if wide_names {
            let units = (total.saturating_sub(off)) / 2;
            let s = std::slice::from_raw_parts(base.add(off) as *const u16, units);
            let mut i = 0;
            while i < s.len() && s[i] != 0 {
                let start = i;
                while i < s.len() && s[i] != 0 { i += 1; }
                out.push(PathBuf::from(String::from_utf16_lossy(&s[start..i])));
                i += 1;
            }
        } else {
            let s = std::slice::from_raw_parts(base.add(off), total.saturating_sub(off));
            let mut i = 0;
            while i < s.len() && s[i] != 0 {
                let start = i;
                while i < s.len() && s[i] != 0 { i += 1; }
                out.push(PathBuf::from(String::from_utf8_lossy(&s[start..i]).to_string()));
                i += 1;
            }
        }
        let _ = GlobalUnlock(h);
    }
    out
}
