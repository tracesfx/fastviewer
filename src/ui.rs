//! Flexible UI: a docking layout engine, toolbar buttons, gallery (strip / grid, any edge, adjustable size),
//! and a metadata panel (any edge, per-field hiding).
use crate::config::*;
use crate::*;

// ---------------------------------------------------------------- geometry
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct R { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }
impl R {
    pub fn has(&self, px: f32, py: f32) -> bool { px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h }
    pub fn d(&self) -> D2D_RECT_F { rc(self.x, self.y, self.x + self.w, self.y + self.h) }
    pub fn right(&self) -> f32 { self.x + self.w }
    pub fn bottom(&self) -> f32 { self.y + self.h }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dock { Top, Bottom, Left, Right }
impl Dock {
    pub fn parse(s: &str, def: Dock) -> Dock {
        match s.trim().to_lowercase().as_str() { "top" => Dock::Top, "bottom" => Dock::Bottom, "left" => Dock::Left, "right" => Dock::Right, _ => def }
    }
    pub fn name(self) -> &'static str { match self { Dock::Top => "top", Dock::Bottom => "bottom", Dock::Left => "left", Dock::Right => "right" } }
    pub fn next(self) -> Dock { match self { Dock::Bottom => Dock::Left, Dock::Left => Dock::Top, Dock::Top => Dock::Right, Dock::Right => Dock::Bottom } }
    /// panel spans the window horizontally (top / bottom)
    pub fn horiz(self) -> bool { matches!(self, Dock::Top | Dock::Bottom) }
}

fn carve(rem: &mut R, dock: Dock, t: f32) -> R {
    match dock {
        Dock::Top => { let r = R { x: rem.x, y: rem.y, w: rem.w, h: t }; rem.y += t; rem.h -= t; r }
        Dock::Bottom => { let r = R { x: rem.x, y: rem.bottom() - t, w: rem.w, h: t }; rem.h -= t; r }
        Dock::Left => { let r = R { x: rem.x, y: rem.y, w: t, h: rem.h }; rem.x += t; rem.w -= t; r }
        Dock::Right => { let r = R { x: rem.right() - t, y: rem.y, w: t, h: rem.h }; rem.w -= t; r }
    }
}
fn clamp_t(t: f32, dim: f32, ui: f32) -> f32 { t.clamp(36.0 * ui, (dim - 120.0 * ui).max(36.0 * ui)) }

pub struct Layout { pub bar: Option<R>, pub gal: Option<R>, pub info: Option<R>, pub view: R, pub btns: Vec<(String, R)> }

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit { None, View, Btn(usize), BarBg, Thumb(usize), GalBg, SplitGallery, SplitInfo, InfoRow(usize), InfoBg }
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Split { Gallery, Info }

pub struct Ui {
    pub toolbar: bool, pub tb_dock: Dock, pub tb_list: Vec<String>,
    pub g_dock: Dock, pub g_grid: bool, pub g_size: f32, pub g_names: bool, pub g_scroll: f32,
    pub thumb: f32, pub thumb_cap: u32, pub browse: bool,
    pub i_dock: Dock, pub i_size: f32, pub i_hidden: HashSet<String>, pub i_edit: bool, pub i_scroll: f32, pub i_content: f32,
    pub info_rows: Vec<(String, R, bool)>,
    pub hover: Hit, pub split: Option<Split>, pub drag_src: Option<(f32, f32, usize)>,
}
impl Ui {
    pub fn from_cfg(c: &Cfg) -> Ui {
        Ui {
            toolbar: c.flag("toolbar"), tb_dock: Dock::parse(&c.text("toolbar_dock"), Dock::Top),
            tb_list: c.text("toolbar_buttons").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            g_dock: Dock::parse(&c.text("gallery_dock"), Dock::Bottom), g_grid: c.text("gallery_layout") == "grid",
            g_size: c.num("gallery_size"), g_names: c.flag("gallery_names"), g_scroll: 0.0,
            thumb: c.num("thumb_size").clamp(32.0, 320.0), thumb_cap: 0, browse: false,
            i_dock: Dock::parse(&c.text("info_dock"), Dock::Right), i_size: c.num("info_size"),
            i_hidden: c.text("info_hidden").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            i_edit: false, i_scroll: 0.0, i_content: 0.0, info_rows: vec![], hover: Hit::None, split: None, drag_src: None,
        }
    }
}

/// Every field the metadata panel can show (used by the "choose fields" editor).
pub const KNOWN_FIELDS: &[&str] = &[
    "Name", "Folder", "Size", "Modified", "Dimensions", "Megapixels", "Aspect", "Format", "Pixel format", "DPI", "Frames", "Animation",
    "Camera", "Lens", "Exposure", "Aperture", "ISO", "Focal length", "Taken", "Software", "Artist", "Copyright", "GPS",
    "parameters", "prompt", "Description", "Comment", "Title", "Author", "Source",
];

pub fn button_label(a: &str) -> &'static str {
    match a {
        "first" => "|<", "prev" => "<", "next" => ">", "last" => ">|",
        "zoom_in" => "+", "zoom_out" => "\u{2212}", "fit" => "Fit", "actual" => "1:1", "fill" => "Fill",
        "rotate_ccw" => "\u{21BA}", "rotate_cw" => "\u{21BB}", "flip" => "Flip",
        "info" => "Info", "gallery" => "Gallery", "gallery_layout" => "Grid", "gallery_dock" => "G-dock", "info_dock" => "I-dock",
        "fields" => "Fields", "hud" => "HUD", "pixel" => "Pixel", "histogram" => "Histo", "channel" => "Chan", "background" => "BG",
        "smooth" => "Smooth", "animate" => "Play", "step" => "Step", "thumb_smaller" => "T\u{2212}", "thumb_bigger" => "T+",
        "browse" => "Browse", "fullscreen" => "Full", "copy_image" => "Copy", "copy_path" => "Path", "reveal" => "Reveal",
        "open" => "Open", "delete" => "Delete", "toolbar" => "Bar", "toolbar_dock" => "B-dock", "config" => "Config", "help" => "Help",
        "quit" => "Quit",
        _ => "?",
    }
}

impl App {
    fn btn_w(&self, label: &str) -> f32 { ((label.chars().count() as f32 * 7.6 + 18.0).max(30.0)) * self.ui }

    pub fn btn_text(&self, a: &str) -> &'static str {
        if a == "animate" && !ANIM_PAUSED.load(Ordering::SeqCst) && self.cur.as_ref().map(|i| i.anim).unwrap_or(false) { return "Pause"; }
        button_label(a)
    }

    fn toolbar_flow(&self, rem: &R) -> (f32, Vec<(String, R)>) {
        let ui = self.ui;
        let (bh, gap, pad, sep) = (26.0 * ui, 3.0 * ui, 5.0 * ui, 10.0 * ui);
        let mut out = vec![];
        let items = &self.u.tb_list;
        let thick;
        if self.u.tb_dock.horiz() {
            let (mut x, mut y) = (pad, pad);
            for it in items {
                if it == "|" { x += sep; continue; }
                let w = if it == "animate" { self.btn_w("Pause") } else { self.btn_w(button_label(it)) };
                if x + w > rem.w - pad && x > pad { x = pad; y += bh + gap; }
                out.push((it.clone(), R { x, y, w, h: bh })); x += w + gap;
            }
            thick = y + bh + pad;
        } else {
            let bw = items.iter().filter(|i| *i != "|").map(|i| self.btn_w(button_label(i))).fold(0.0, f32::max);
            let (mut x, mut y) = (pad, pad);
            for it in items {
                if it == "|" { y += sep; continue; }
                if y + bh > rem.h - pad && y > pad { y = pad; x += bw + gap; }
                out.push((it.clone(), R { x, y, w: bw, h: bh })); y += bh + gap;
            }
            thick = x + bw + pad;
        }
        (thick, out)
    }

    pub fn layout(&self) -> Layout {
        let (cw, ch) = self.client();
        let ui = self.ui;
        let mut rem = R { x: 0.0, y: 0.0, w: cw, h: ch };
        let mut lay = Layout { bar: None, gal: None, info: None, view: rem, btns: vec![] };
        if self.u.toolbar {
            let (t, rel) = self.toolbar_flow(&rem);
            let t = t.min(if self.u.tb_dock.horiz() { rem.h * 0.5 } else { rem.w * 0.5 });
            let bar = carve(&mut rem, self.u.tb_dock, t);
            lay.btns = rel.into_iter().map(|(a, r)| (a, R { x: r.x + bar.x, y: r.y + bar.y, w: r.w, h: r.h })).collect();
            lay.bar = Some(bar);
        }
        if self.show_film && !self.files.is_empty() {
            if self.u.browse {
                lay.gal = Some(rem);
                rem = R { x: rem.x, y: rem.y, w: 1.0, h: 1.0 };
            } else {
                let dim = if self.u.g_dock.horiz() { rem.h } else { rem.w };
                let t = if self.u.g_grid { self.u.g_size * ui } else { (self.u.thumb + 18.0) * ui };
                lay.gal = Some(carve(&mut rem, self.u.g_dock, clamp_t(t, dim, ui)));
            }
        }
        if self.show_info && !self.u.browse {
            let dim = if self.u.i_dock.horiz() { rem.h } else { rem.w };
            lay.info = Some(carve(&mut rem, self.u.i_dock, clamp_t(self.u.i_size * ui, dim, ui)));
        }
        lay.view = R { x: rem.x, y: rem.y, w: rem.w.max(1.0), h: rem.h.max(1.0) };
        lay
    }

    // ---------------------------------------------------------------- gallery geometry
    fn g_pad(&self) -> f32 { 8.0 * self.ui }
    fn g_gap(&self) -> f32 { 6.0 * self.ui }
    fn ts(&self) -> f32 { self.u.thumb * self.ui }
    fn label_h(&self) -> f32 { if self.u.g_names { 16.0 * self.ui } else { 0.0 } }
    fn is_grid(&self) -> bool { self.u.g_grid || self.u.browse }
    pub fn g_cols(&self, g: &R) -> usize { (((g.w - 2.0 * self.g_pad()) / (self.ts() + self.g_gap())).floor() as isize).max(1) as usize }
    fn g_cell_h(&self) -> f32 { self.ts() + self.g_gap() + self.label_h() }
    fn g_max_scroll(&self, g: &R) -> f32 {
        let rows = (self.files.len() + self.g_cols(g) - 1) / self.g_cols(g).max(1);
        (rows as f32 * self.g_cell_h() + 2.0 * self.g_pad() - g.h).max(0.0)
    }
    pub fn g_rect(&self, g: &R, i: usize) -> R {
        let (ts, pad, pitch) = (self.ts(), self.g_pad(), self.ts() + self.g_gap());
        if self.is_grid() {
            let cols = self.g_cols(g);
            R { x: g.x + pad + (i % cols) as f32 * pitch, y: g.y + pad + (i / cols) as f32 * self.g_cell_h() - self.u.g_scroll, w: ts, h: ts }
        } else if self.u.g_dock.horiz() {
            R { x: g.x + g.w / 2.0 - ts / 2.0 + (i as f32 - self.idx as f32) * pitch, y: g.y + pad, w: ts, h: ts }
        } else {
            R { x: g.x + pad, y: g.y + g.h / 2.0 - ts / 2.0 + (i as f32 - self.idx as f32) * pitch, w: ts, h: ts }
        }
    }
    fn g_range(&self, g: &R) -> (usize, usize) {
        let n = self.files.len();
        if n == 0 { return (1, 0); }
        let pitch = self.ts() + self.g_gap();
        if self.is_grid() {
            let cols = self.g_cols(g);
            let ch = self.g_cell_h();
            let first_row = ((self.u.g_scroll - self.g_pad()) / ch).floor().max(0.0) as usize;
            let last_row = ((self.u.g_scroll + g.h) / ch).ceil().max(0.0) as usize;
            (first_row * cols, ((last_row + 1) * cols).min(n) - 1)
        } else {
            let span = if self.u.g_dock.horiz() { g.w } else { g.h };
            let half = (span / 2.0 / pitch).ceil() as isize + 1;
            ((self.idx as isize - half).max(0) as usize, ((self.idx as isize + half) as usize).min(n - 1))
        }
    }
    pub fn g_hit(&self, g: &R, x: f32, y: f32) -> Option<usize> {
        let n = self.files.len() as isize;
        let (pad, pitch) = (self.g_pad(), self.ts() + self.g_gap());
        let i = if self.is_grid() {
            let cols = self.g_cols(g) as isize;
            let (cx, ry) = (((x - g.x - pad) / pitch).floor() as isize, ((y - g.y - pad + self.u.g_scroll) / self.g_cell_h()).floor() as isize);
            if cx < 0 || cx >= cols || ry < 0 { return None; }
            ry * cols + cx
        } else if self.u.g_dock.horiz() {
            self.idx as isize + ((x - (g.x + g.w / 2.0)) / pitch).round() as isize
        } else {
            self.idx as isize + ((y - (g.y + g.h / 2.0)) / pitch).round() as isize
        };
        if i >= 0 && i < n { Some(i as usize) } else { None }
    }
    /// scroll the grid so the current image is visible
    pub fn ensure_visible(&mut self) {
        if !self.is_grid() { return; }
        let lay = self.layout();
        let Some(g) = lay.gal else { return };
        let cols = self.g_cols(&g);
        let ch = self.g_cell_h();
        let top = (self.idx / cols) as f32 * ch;
        let (lo, hi) = (top, top + ch + 2.0 * self.g_pad());
        if lo < self.u.g_scroll { self.u.g_scroll = lo; }
        else if hi > self.u.g_scroll + g.h { self.u.g_scroll = hi - g.h; }
        self.u.g_scroll = self.u.g_scroll.clamp(0.0, self.g_max_scroll(&g));
    }
    pub fn browse_move(&mut self, dir: isize) {
        let lay = self.layout();
        let Some(g) = lay.gal else { return };
        let cols = self.g_cols(&g) as isize;
        self.go(self.idx as isize + dir * cols);
    }

    // ---------------------------------------------------------------- thumbnails
    /// (Re)starts the background thumbnail worker for the current folder list at the current size.
    pub fn start_thumbs(&mut self) {
        let (tx, rx) = mpsc::channel::<(usize, PathBuf)>();
        let cap = ((self.ts() * 1.25).max(96.0)) as u32;
        self.u.thumb_cap = cap;
        let g = self.dir_gen;
        std::thread::spawn(move || {
            unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
            let f = wic_factory();
            while let Ok((i, p)) = rx.recv() {
                if let Some(ref f) = f {
                    if let Ok(raw) = decode_thumb(f, &p, cap) { post(WM_THUMB, Box::into_raw(Box::new((g, i, raw))) as isize); }
                }
            }
        });
        self.thumb_tx = Some(tx);
    }
    pub fn thumb_resized(&mut self) {
        if self.ts() * 1.05 > self.u.thumb_cap as f32 && self.thumb_tx.is_some() {
            self.thumbs.clear(); self.thumb_req.clear(); self.start_thumbs();
        }
        self.ensure_visible();
    }

    // ---------------------------------------------------------------- hit testing
    fn split_band(&self, p: &R, dock: Dock) -> R {
        let b = 6.0 * self.ui;
        match dock {
            Dock::Bottom => R { x: p.x, y: p.y - b / 2.0, w: p.w, h: b },
            Dock::Top => R { x: p.x, y: p.bottom() - b / 2.0, w: p.w, h: b },
            Dock::Left => R { x: p.right() - b / 2.0, y: p.y, w: b, h: p.h },
            Dock::Right => R { x: p.x - b / 2.0, y: p.y, w: b, h: p.h },
        }
    }
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let l = self.layout();
        if !self.u.browse {
            if let Some(g) = l.gal { if self.split_band(&g, self.u.g_dock).has(x, y) { return Hit::SplitGallery; } }
            if let Some(i) = l.info { if self.split_band(&i, self.u.i_dock).has(x, y) { return Hit::SplitInfo; } }
        }
        if let Some(b) = l.bar {
            if b.has(x, y) {
                for (k, (_, r)) in l.btns.iter().enumerate() { if r.has(x, y) { return Hit::Btn(k); } }
                return Hit::BarBg;
            }
        }
        if let Some(g) = l.gal {
            if g.has(x, y) { return match self.g_hit(&g, x, y) { Some(i) => Hit::Thumb(i), None => Hit::GalBg }; }
        }
        if let Some(i) = l.info {
            if i.has(x, y) {
                for (k, (_, r, _)) in self.u.info_rows.iter().enumerate() { if r.has(x, y) { return Hit::InfoRow(k); } }
                return Hit::InfoBg;
            }
        }
        Hit::View
    }

    pub fn cursor_for(&self, x: f32, y: f32) -> PCWSTR {
        if self.drag.is_some() { return IDC_SIZEALL; }
        match self.u.split.map(|s| if s == Split::Gallery { Hit::SplitGallery } else { Hit::SplitInfo }).unwrap_or_else(|| self.hit(x, y)) {
            Hit::SplitGallery => if self.u.g_dock.horiz() { IDC_SIZENS } else { IDC_SIZEWE },
            Hit::SplitInfo => if self.u.i_dock.horiz() { IDC_SIZENS } else { IDC_SIZEWE },
            Hit::Btn(_) | Hit::Thumb(_) | Hit::InfoRow(_) => IDC_HAND,
            _ => IDC_ARROW,
        }
    }

    // ---------------------------------------------------------------- input
    pub fn ui_mouse_down(&mut self, x: f32, y: f32, dbl: bool) -> bool {
        match self.hit(x, y) {
            Hit::SplitGallery => { self.u.split = Some(Split::Gallery); unsafe { SetCapture(self.hwnd); } true }
            Hit::SplitInfo => { self.u.split = Some(Split::Info); unsafe { SetCapture(self.hwnd); } true }
            Hit::Btn(k) => {
                let a = self.layout().btns.get(k).map(|b| b.0.clone());
                if let Some(a) = a { self.dispatch(&a); }
                true
            }
            Hit::BarBg | Hit::GalBg | Hit::InfoBg => true,
            Hit::Thumb(i) => {
                self.go(i as isize);
                if dbl && self.u.browse { self.u.browse = false; self.apply_fit(); }
                if !dbl { self.u.drag_src = Some((x, y, i)); unsafe { SetCapture(self.hwnd); } }
                self.ensure_visible(); self.invalidate(); true
            }
            Hit::InfoRow(k) => {
                if let Some((name, r, edit)) = self.u.info_rows.get(k).cloned() {
                    if edit || x > r.right() - 22.0 * self.ui {
                        if !self.u.i_hidden.remove(&name) { self.u.i_hidden.insert(name); }
                        self.save_layout();
                    }
                }
                self.invalidate(); true
            }
            Hit::None | Hit::View => false,
        }
    }
    pub fn ui_mouse_move(&mut self, x: f32, y: f32) -> bool {
        if let Some(s) = self.u.split {
            let l = self.layout();
            let ui = self.ui;
            match s {
                Split::Gallery => if let Some(g) = l.gal {
                    let t = match self.u.g_dock { Dock::Bottom => g.bottom() - y, Dock::Top => y - g.y, Dock::Left => x - g.x, Dock::Right => g.right() - x } / ui;
                    if self.u.g_grid { self.u.g_size = t.clamp(60.0, 2000.0); } else { self.u.thumb = (t - 18.0).clamp(32.0, 320.0); self.thumb_resized(); }
                },
                Split::Info => if let Some(g) = l.info {
                    let t = match self.u.i_dock { Dock::Bottom => g.bottom() - y, Dock::Top => y - g.y, Dock::Left => x - g.x, Dock::Right => g.right() - x } / ui;
                    self.u.i_size = t.clamp(100.0, 1200.0);
                },
            }
            self.apply_fit(); self.ensure_visible(); self.invalidate();
            return true;
        }
        if let Some((sx, sy, i)) = self.u.drag_src {
            let (tx, ty) = unsafe { (GetSystemMetrics(SM_CXDRAG).max(4) as f32, GetSystemMetrics(SM_CYDRAG).max(4) as f32) };
            if (x - sx).abs() > tx || (y - sy).abs() > ty {
                self.u.drag_src = None;
                unsafe { let _ = ReleaseCapture(); }
                if let Some(p) = self.files.get(i).cloned() { crate::shell::drag_file(self.hwnd, &p); }
                self.u.hover = Hit::None; self.invalidate();
            }
            return true;
        }
        let h = self.hit(x, y);
        if h != self.u.hover { self.u.hover = h; self.invalidate(); }
        false
    }
    pub fn ui_mouse_up(&mut self) -> bool {
        if self.u.drag_src.take().is_some() { unsafe { let _ = ReleaseCapture(); } return true; }
        if self.u.split.take().is_some() { unsafe { let _ = ReleaseCapture(); } self.save_layout(); return true; }
        false
    }
    pub fn ui_wheel(&mut self, x: f32, y: f32, delta: f32, ctrl: bool) -> bool {
        match self.hit(x, y) {
            Hit::Thumb(_) | Hit::GalBg | Hit::SplitGallery => {
                let l = self.layout();
                if ctrl { self.dispatch(if delta > 0.0 { "thumb_bigger" } else { "thumb_smaller" }); }
                else if self.is_grid() {
                    if let Some(g) = l.gal { self.u.g_scroll = (self.u.g_scroll - delta * self.g_cell_h()).clamp(0.0, self.g_max_scroll(&g)); }
                    self.invalidate();
                } else { self.go(self.idx as isize - delta.signum() as isize); }
                true
            }
            Hit::InfoRow(_) | Hit::InfoBg | Hit::SplitInfo => {
                let view_h = self.layout().info.map(|i| i.h).unwrap_or(0.0);
                let max = (self.u.i_content - view_h + 30.0 * self.ui).max(0.0);
                self.u.i_scroll = (self.u.i_scroll - delta * 40.0 * self.ui).clamp(0.0, max);
                self.invalidate(); true
            }
            Hit::Btn(_) | Hit::BarBg => true,
            _ => false,
        }
    }

    // ---------------------------------------------------------------- commands
    pub fn ui_action(&mut self, act: &str) -> bool {
        match act {
            "gallery_layout" => { self.u.g_grid = !self.u.g_grid; self.show_film = true; self.u.g_scroll = 0.0; self.thumb_resized(); self.save_layout(); }
            "gallery_dock" => { self.u.g_dock = self.u.g_dock.next(); self.show_film = true; self.save_layout(); }
            "info_dock" => {
                let was = self.u.i_dock.horiz();
                self.u.i_dock = self.u.i_dock.next();
                if was != self.u.i_dock.horiz() { self.u.i_size = if self.u.i_dock.horiz() { 170.0 } else { 330.0 }; }
                self.show_info = true; self.save_layout();
            }
            "fields" => { self.u.i_edit = !self.u.i_edit; if self.u.i_edit { self.show_info = true; } }
            "thumb_bigger" => { self.u.thumb = (self.u.thumb * 1.15).round().clamp(32.0, 320.0); self.thumb_resized(); self.save_layout(); }
            "thumb_smaller" => { self.u.thumb = (self.u.thumb / 1.15).round().clamp(32.0, 320.0); self.thumb_resized(); self.save_layout(); }
            "browse" => { self.u.browse = !self.u.browse; if self.u.browse { self.show_film = true; } self.u.g_scroll = 0.0; self.ensure_visible(); }
            "toolbar" => { self.u.toolbar = !self.u.toolbar; self.save_layout(); }
            "toolbar_dock" => { self.u.tb_dock = self.u.tb_dock.next(); self.u.toolbar = true; self.save_layout(); }
            _ => return false,
        }
        self.apply_fit();
        true
    }

    pub fn save_layout(&mut self) {
        let u = &self.u;
        let mut hidden: Vec<&String> = u.i_hidden.iter().collect();
        hidden.sort();
        let kv: Vec<(&str, String)> = vec![
            ("thumb_size", format!("{}", u.thumb as i32)), ("gallery_dock", u.g_dock.name().into()),
            ("gallery_layout", if u.g_grid { "grid" } else { "strip" }.into()), ("gallery_size", format!("{}", u.g_size as i32)),
            ("info_dock", u.i_dock.name().into()), ("info_size", format!("{}", u.i_size as i32)),
            ("info_hidden", hidden.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")),
            ("toolbar", if u.toolbar { "1" } else { "0" }.into()), ("toolbar_dock", u.tb_dock.name().into()),
        ];
        for (k, v) in &kv { self.cfg.map.insert(k.to_string(), v.clone()); }
        save_settings(&kv);
    }

    // ---------------------------------------------------------------- drawing
    unsafe fn text_h(&self, s: &str, w: f32, small: bool) -> f32 {
        let fmt = if small { self.fmt_s.as_ref() } else { self.fmt.as_ref() };
        let (Some(fmt), Some(dw)) = (fmt, self.dw.as_ref()) else { return 16.0 * self.ui };
        let ws: Vec<u16> = s.encode_utf16().collect();
        let Ok(lay) = dw.CreateTextLayout(&ws, fmt, w.max(8.0), 4000.0) else { return 16.0 * self.ui };
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = lay.GetMetrics(&mut m);
        m.height
    }

    pub unsafe fn draw_toolbar(&mut self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let lay = self.layout();
        let Some(bar) = lay.bar else { return };
        let (fg, dim, accent) = (self.cfg.color("text"), self.cfg.color("text_dim"), self.cfg.color("accent"));
        br.SetColor(&self.cfg.color("panel"));
        rt.FillRectangle(&bar.d(), br);
        let ui = self.ui;
        for (k, (a, r)) in lay.btns.iter().enumerate() {
            let hov = self.u.hover == Hit::Btn(k);
            let on = self.btn_active(a);
            let enabled = self.btn_enabled(a);
            if on { br.SetColor(&D2D1_COLOR_F { a: 0.28, ..accent }); rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d(), radiusX: 4.0 * ui, radiusY: 4.0 * ui }, br); }
            if hov { br.SetColor(&clr(1.0, 1.0, 1.0, 0.13)); rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d(), radiusX: 4.0 * ui, radiusY: 4.0 * ui }, br); }
            if on { br.SetColor(&accent); rt.FillRectangle(&rc(r.x + 5.0 * ui, r.bottom() - 2.5 * ui, r.right() - 5.0 * ui, r.bottom() - 1.0 * ui), br); }
            let t = self.btn_text(a);
            let (tw, th) = self.text_w(t, false);
            self.text(rt, br, t, r.x + (r.w - tw) / 2.0, r.y + (r.h - th) / 2.0 - 1.0, r.w + 20.0, if enabled { fg } else { dim }, false);
        }
        // separator line on the edge facing the image
        br.SetColor(&clr(1.0, 1.0, 1.0, 0.07));
        let e = match self.u.tb_dock { Dock::Top => rc(bar.x, bar.bottom() - 1.0, bar.right(), bar.bottom()), Dock::Bottom => rc(bar.x, bar.y, bar.right(), bar.y + 1.0),
            Dock::Left => rc(bar.right() - 1.0, bar.y, bar.right(), bar.bottom()), Dock::Right => rc(bar.x, bar.y, bar.x + 1.0, bar.bottom()) };
        rt.FillRectangle(&e, br);
    }

    pub fn btn_active(&self, a: &str) -> bool {
        match a {
            "info" => self.show_info, "gallery" => self.show_film, "gallery_layout" => self.u.g_grid, "hud" => self.show_hud,
            "pixel" => self.show_pixel, "histogram" => self.show_hist, "channel" => self.channel != 0, "background" => self.bgmode != 0,
            "smooth" => self.smooth != 0, "fullscreen" => self.fs, "browse" => self.u.browse, "fields" => self.u.i_edit, "toolbar" => true,
            "flip" => self.flip, "help" => self.show_help, "fit" => self.fit,
            "animate" => self.cur.as_ref().map(|i| i.anim && ANIM_PAUSED.load(Ordering::SeqCst)).unwrap_or(false),
            _ => false,
        }
    }
    pub fn btn_enabled(&self, a: &str) -> bool {
        match a {
            "animate" | "step" => self.cur.as_ref().map(|i| i.anim).unwrap_or(false),
            "next" | "prev" | "first" | "last" => self.files.len() > 1,
            "zoom_in" | "zoom_out" | "fit" | "actual" | "fill" | "rotate_ccw" | "rotate_cw" | "flip" | "copy_image" | "pixel" | "histogram" | "channel" => self.cur.is_some(),
            "copy_path" | "reveal" | "delete" => !self.cur_path.as_os_str().is_empty(),
            "gallery_layout" | "gallery_dock" | "thumb_smaller" | "thumb_bigger" | "browse" => !self.files.is_empty(),
            _ => true,
        }
    }

    pub unsafe fn draw_tooltip(&self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let Hit::Btn(k) = self.u.hover else { return };
        let lay = self.layout();
        let Some((a, r)) = lay.btns.get(k) else { return };
        let desc = ACTIONS.iter().find(|(n, _)| n == a).map(|(_, d)| *d).unwrap_or("");
        let keys = self.cfg.keys_for(a);
        let txt = if keys.is_empty() { desc.to_string() } else { format!("{}   [{}]", desc, keys.replace(',', " / ")) };
        let (tw, th) = self.text_w(&txt, true);
        let (cw, ch) = self.client();
        let pad = 6.0 * self.ui;
        let (mut x, mut y) = match self.u.tb_dock {
            Dock::Top => (r.x, r.bottom() + 6.0 * self.ui), Dock::Bottom => (r.x, r.y - th - pad * 2.0 - 6.0 * self.ui),
            Dock::Left => (r.right() + 6.0 * self.ui, r.y), Dock::Right => (r.x - tw - pad * 2.0 - 6.0 * self.ui, r.y),
        };
        x = x.min(cw - tw - pad * 2.0 - 4.0).max(4.0);
        y = y.min(ch - th - pad * 2.0 - 4.0).max(4.0);
        br.SetColor(&clr(0.02, 0.02, 0.03, 0.96));
        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rc(x, y, x + tw + pad * 2.0, y + th + pad * 2.0), radiusX: 4.0, radiusY: 4.0 }, br);
        self.text(rt, br, &txt, x + pad, y + pad, tw + 8.0, self.cfg.color("text"), true);
    }

    pub unsafe fn draw_gallery(&mut self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let lay = self.layout();
        let Some(g) = lay.gal else { return };
        let ui = self.ui;
        let (accent, dim, fg) = (self.cfg.color("accent"), self.cfg.color("text_dim"), self.cfg.color("text"));
        br.SetColor(&self.cfg.color("panel"));
        rt.FillRectangle(&g.d(), br);
        if self.is_grid() { self.u.g_scroll = self.u.g_scroll.clamp(0.0, self.g_max_scroll(&g)); }
        rt.PushAxisAlignedClip(&g.d(), D2D1_ANTIALIAS_MODE_ALIASED);
        let ts = self.ts();
        let (a, b) = self.g_range(&g);
        let grid = self.is_grid();
        for i in a..=b {
            if i >= self.files.len() { break; }
            let r = self.g_rect(&g, i);
            if r.bottom() < g.y || r.y > g.bottom() || r.right() < g.x || r.x > g.right() { continue; }
            if !self.thumbs.contains_key(&i) && !self.thumb_req.contains(&i) {
                if let Some(tx) = &self.thumb_tx { self.thumb_req.insert(i); let _ = tx.send((i, self.files[i].clone())); }
            }
            br.SetColor(&clr(1.0, 1.0, 1.0, 0.06));
            rt.FillRectangle(&r.d(), br);
            if let Some(t) = self.thumbs.get_mut(&i) {
                if t.bmp.is_none() { t.bmp = make_bitmap(rt, t.w, t.h, &t.px); }
                if let Some(bm) = &t.bmp {
                    let s = (ts / t.w as f32).min(ts / t.h as f32);
                    let (dw, dh) = (t.w as f32 * s, t.h as f32 * s);
                    let (dx, dy) = (r.x + (ts - dw) / 2.0, r.y + (ts - dh) / 2.0);
                    rt.DrawBitmap(bm, Some(&rc(dx, dy, dx + dw, dy + dh)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                }
            }
            if self.u.hover == Hit::Thumb(i) && i != self.idx { br.SetColor(&clr(1.0, 1.0, 1.0, 0.5)); rt.DrawRectangle(&r.d(), br, 1.0 * ui, None); }
            if i == self.idx { br.SetColor(&accent); rt.DrawRectangle(&rc(r.x - 1.0, r.y - 1.0, r.right() + 1.0, r.bottom() + 1.0), br, 2.0 * ui, None); }
            if grid && self.u.g_names {
                let name = self.files[i].file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                self.text(rt, br, &name, r.x, r.bottom() + 2.0 * ui, ts, if i == self.idx { fg } else { dim }, true);
            }
        }
        rt.PopAxisAlignedClip();
        // grab handle on the edge facing the image
        let band = self.split_band(&g, if self.u.browse { Dock::Bottom } else { self.u.g_dock });
        if !self.u.browse {
            let active = self.u.hover == Hit::SplitGallery || self.u.split == Some(Split::Gallery);
            br.SetColor(&if active { accent } else { clr(1.0, 1.0, 1.0, 0.1) });
            let (cx, cy) = (band.x + band.w / 2.0, band.y + band.h / 2.0);
            let e = if self.u.g_dock.horiz() { rc(cx - 22.0 * ui, cy - 1.0, cx + 22.0 * ui, cy + 1.0) } else { rc(cx - 1.0, cy - 22.0 * ui, cx + 1.0, cy + 22.0 * ui) };
            rt.FillRectangle(&e, br);
        }
    }

    pub unsafe fn draw_info(&mut self, rt: &ID2D1HwndRenderTarget, br: &ID2D1SolidColorBrush) {
        let lay = self.layout();
        let Some(r) = lay.info else { return };
        let ui = self.ui;
        let (fg, dim, accent) = (self.cfg.color("text"), self.cfg.color("text_dim"), self.cfg.color("accent"));
        br.SetColor(&self.cfg.color("panel"));
        rt.FillRectangle(&r.d(), br);
        rt.PushAxisAlignedClip(&r.d(), D2D1_ANTIALIAS_MODE_ALIASED);
        let pad = 12.0 * ui;
        let edit = self.u.i_edit;
        let meta: Vec<(String, String)> = self.cur.as_ref().map(|i| i.meta.clone()).unwrap_or_default();
        let nhidden = self.u.i_hidden.len();
        let title = if edit { "Show / hide fields - click a row".to_string() }
            else if nhidden > 0 { format!("Metadata   ({} hidden - Fields button to restore)", nhidden) } else { "Metadata".to_string() };
        self.text(rt, br, &title, r.x + pad, r.y + 6.0 * ui, r.w - pad * 2.0, dim, true);
        let horiz = self.u.i_dock.horiz();
        let colw = if horiz { (300.0 * ui).min(r.w - pad) } else { r.w - pad * 2.0 };
        let ncols = if horiz { (((r.w - pad) / colw).floor() as usize).max(1) } else { 1 };
        let kw = (92.0 * ui).min(colw * 0.4);
        let rows: Vec<(String, String)> = if edit {
            let mut names: Vec<String> = KNOWN_FIELDS.iter().map(|s| s.to_string()).collect();
            for (k, _) in &meta { if !names.contains(k) { names.push(k.clone()); } }
            for h in &self.u.i_hidden { if !names.contains(h) { names.push(h.clone()); } }
            names.into_iter().map(|n| { let v = meta.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone()).unwrap_or_default(); (n, v) }).collect()
        } else { meta.into_iter().filter(|(k, _)| !self.u.i_hidden.contains(k)).collect() };
        if rows.is_empty() && !edit { self.text(rt, br, if self.cur.is_none() { "No image" } else { "All fields hidden" }, r.x + pad, r.y + 30.0 * ui, r.w, dim, false); }
        let top = r.y + 26.0 * ui;
        let scroll = if horiz { 0.0 } else { self.u.i_scroll };
        let bottom = r.bottom() - pad * 0.5;
        let (mut col, mut y) = (0usize, top - scroll);
        let mut new_rows: Vec<(String, R, bool)> = vec![];
        let mut content_end = top;
        for (k, v) in rows {
            let vw = colw - kw - 4.0 * ui;
            let h = if edit { 18.0 * ui } else { self.text_h(&v, vw, false).max(16.0 * ui) };
            if horiz && y + h > bottom && col + 1 < ncols && y > top { col += 1; y = top; }
            let x = r.x + pad + col as f32 * (colw + pad * 0.5);
            let row = R { x: x - 4.0 * ui, y: y - 1.0, w: colw + 8.0 * ui, h: h + 2.0 };
            let idx = new_rows.len();
            if self.u.hover == Hit::InfoRow(idx) { br.SetColor(&clr(1.0, 1.0, 1.0, 0.07)); rt.FillRectangle(&row.d(), br); }
            if edit {
                let shown = !self.u.i_hidden.contains(&k);
                let bx = rc(x, y + 2.0 * ui, x + 13.0 * ui, y + 15.0 * ui);
                br.SetColor(&if shown { accent } else { dim });
                rt.DrawRectangle(&bx, br, 1.2 * ui, None);
                if shown { rt.FillRectangle(&rc(bx.left + 3.0 * ui, bx.top + 3.0 * ui, bx.right - 3.0 * ui, bx.bottom - 3.0 * ui), br); }
                self.text(rt, br, &k, x + 22.0 * ui, y, colw - 22.0 * ui, if shown { fg } else { dim }, false);
            } else {
                self.text(rt, br, &k, x, y, kw - 6.0 * ui, dim, true);
                self.text(rt, br, &v, x + kw, y, vw, fg, false);
                if self.u.hover == Hit::InfoRow(idx) { self.text(rt, br, "\u{00D7}", row.right() - 16.0 * ui, y - 1.0, 16.0 * ui, dim, false); }
            }
            new_rows.push((k, row, edit));
            y += h + 5.0 * ui;
            content_end = content_end.max(y);
        }
        self.u.info_rows = new_rows;
        self.u.i_content = content_end - top + scroll;
        rt.PopAxisAlignedClip();
        let active = self.u.hover == Hit::SplitInfo || self.u.split == Some(Split::Info);
        let band = self.split_band(&r, self.u.i_dock);
        br.SetColor(&if active { accent } else { clr(1.0, 1.0, 1.0, 0.1) });
        let (cx, cy) = (band.x + band.w / 2.0, band.y + band.h / 2.0);
        let e = if self.u.i_dock.horiz() { rc(cx - 22.0 * ui, cy - 1.0, cx + 22.0 * ui, cy + 1.0) } else { rc(cx - 1.0, cy - 22.0 * ui, cx + 1.0, cy + 22.0 * ui) };
        rt.FillRectangle(&e, br);
    }

    pub fn dump_ui(&self, s: &mut String) {
        use std::fmt::Write;
        let l = self.layout();
        let u = &self.u;
        let _ = writeln!(s, "u.toolbar={}\nu.tb_dock={}\nu.g_dock={}\nu.g_grid={}\nu.g_size={}\nu.thumb={}\nu.thumb_cap={}\nu.browse={}\nu.i_dock={}\nu.i_size={}\nu.i_edit={}\nu.g_scroll={:.1}",
            u.toolbar, u.tb_dock.name(), u.g_dock.name(), u.g_grid, u.g_size, u.thumb, u.thumb_cap, u.browse, u.i_dock.name(), u.i_size, u.i_edit, u.g_scroll);
        let mut h: Vec<&String> = u.i_hidden.iter().collect(); h.sort();
        let _ = writeln!(s, "u.hidden={}", h.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","));
        let f = |r: &R| format!("{:.0},{:.0},{:.0},{:.0}", r.x, r.y, r.w, r.h);
        let _ = writeln!(s, "rect.view={}", f(&l.view));
        if let Some(r) = l.bar { let _ = writeln!(s, "rect.bar={}", f(&r)); }
        if let Some(r) = l.gal { let _ = writeln!(s, "rect.gal={}\ngal.cols={}", f(&r), self.g_cols(&r)); }
        if let Some(r) = l.info { let _ = writeln!(s, "rect.info={}", f(&r)); }
        for (a, r) in &l.btns { let _ = writeln!(s, "btn.{}={}", a, f(r)); }
        let _ = writeln!(s, "info.rows={}", u.info_rows.iter().map(|(k, _, _)| k.as_str()).collect::<Vec<_>>().join("|"));
        if let Some(g) = l.gal { if !self.files.is_empty() { let _ = writeln!(s, "thumb.cur={}", f(&self.g_rect(&g, self.idx))); } }
        let _ = writeln!(s, "info.row0={}", u.info_rows.first().map(|(_, r, _)| f(r)).unwrap_or_default());
        let mut xs: Vec<i32> = u.info_rows.iter().map(|(_, r, _)| r.x as i32).collect();
        xs.sort(); xs.dedup();
        let _ = writeln!(s, "info.cols={}\nu.hover={:?}", xs.len(), u.hover);
        if let Some(g) = l.gal { for i in 0..self.files.len().min(24) { let _ = writeln!(s, "thumb.{}={}", i, f(&self.g_rect(&g, i))); } }
    }
}
