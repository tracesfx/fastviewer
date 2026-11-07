use crate::clr;
use std::collections::HashMap;
use std::path::PathBuf;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub const ACTIONS: &[(&str, &str)] = &[
    ("next", "Next image"),
    ("prev", "Previous image"),
    ("first", "First image"),
    ("last", "Last image"),
    ("zoom_in", "Zoom in"),
    ("zoom_out", "Zoom out"),
    ("fit", "Fit to window"),
    ("actual", "Toggle 100% / fit"),
    ("fill", "Fill window"),
    ("rotate_cw", "Rotate clockwise"),
    ("rotate_ccw", "Rotate counter-clockwise"),
    ("flip", "Flip horizontally"),
    ("info", "Toggle metadata panel"),
    ("gallery", "Show / hide gallery"),
    ("gallery_layout", "Gallery: strip <-> grid"),
    ("gallery_dock", "Move gallery to next edge"),
    ("thumb_bigger", "Bigger thumbnails"),
    ("thumb_smaller", "Smaller thumbnails"),
    ("browse", "Browse: full-window thumbnail grid"),
    ("info_dock", "Move metadata panel to next edge"),
    ("fields", "Choose which metadata fields show"),
    ("toolbar", "Show / hide toolbar"),
    ("toolbar_dock", "Move toolbar to next edge"),
    ("hud", "Toggle top-left info"),
    ("pixel", "Pixel inspector + loupe"),
    ("histogram", "Histogram"),
    ("channel", "Cycle R / G / B / A / Luma"),
    ("animate", "Play / pause animated GIF"),
    ("step", "Step one animation frame"),
    ("background", "Cycle background"),
    ("smooth", "Cycle smoothing: auto/smooth/pixels"),
    ("fullscreen", "Fullscreen"),
    ("copy_image", "Copy image to clipboard"),
    ("copy_path", "Copy file path"),
    ("reveal", "Show in Explorer"),
    ("open", "Open file..."),
    ("delete", "Move to Recycle Bin"),
    ("config", "Edit settings file"),
    ("help", "This help"),
    ("quit", "Back / quit"),
];

pub const DEFAULT_INI: &str = "\
; FastViewer settings. Lines starting with ; are comments. Delete a line to restore its default.
; Colors are #rrggbb.

bg=#1b1b1d
bg_light=#e9e9ea
checker_a=#d0d0d0
checker_b=#9c9c9c
panel=#111113
text=#e8e8ea
text_dim=#8d8d94
accent=#4da3ff
font=Segoe UI
font_size=13

; window
width=1100
height=760
maximized=0
fullscreen=0

; startup panels (1 = shown)
show_info=0
show_gallery=1
show_hud=1

; fit_upscale=1 also enlarges small images to fill the window
fit_upscale=0
zoom_step=1.25

; ---- layout (these are also saved automatically when you change them in the app) ----
; toolbar: 1 = shown; toolbar_dock: top / bottom / left / right
toolbar=1
toolbar_dock=top
; buttons in order, | = separator. Delete names to hide buttons. Any action name from [keys] works.
toolbar_buttons=first,prev,next,last,|,zoom_out,zoom_in,fit,actual,fill,|,rotate_ccw,rotate_cw,flip,|,info,gallery,gallery_layout,gallery_dock,thumb_smaller,thumb_bigger,browse,info_dock,fields,hud,|,pixel,histogram,channel,background,smooth,|,animate,step,|,fullscreen,copy_image,copy_path,reveal,open,delete,|,toolbar_dock,toolbar,config,help
; gallery: dock = top / bottom / left / right; layout = strip (one row/column) or grid
gallery_dock=bottom
gallery_layout=strip
; gallery_size = panel thickness in grid layout (in strip layout the thumbnail size sets it)
gallery_size=240
gallery_names=1
thumb_size=72
; metadata panel: dock + size (width for left/right, height for top/bottom)
info_dock=right
info_size=330
; comma separated field names to hide, e.g. info_hidden=Folder,Megapixels,Aspect
info_hidden=
; wheel: zoom or navigate  (Ctrl+wheel does the opposite)
wheel=zoom
; wrap=1 loops from last image back to the first
wrap=1

[keys]
; Comma separated. Modifiers: Ctrl+ Shift+ Alt+. Keys: A-Z, 0-9, F1-F12, Left, Right, Up, Down,
; Home, End, PageUp, PageDown, Space, Enter, Esc, Tab, Backspace, Delete, Plus, Minus
next=Right,Space,PageDown
prev=Left,Backspace,PageUp
first=Home
last=End
zoom_in=Plus,Up
zoom_out=Minus,Down
fit=0,F
actual=1
fill=2
rotate_cw=R
rotate_ccw=Shift+R
flip=X
info=I
gallery=G
gallery_layout=Ctrl+G
gallery_dock=Shift+G
thumb_bigger=Ctrl+Plus
thumb_smaller=Ctrl+Minus
browse=Tab
info_dock=Shift+I
fields=Ctrl+I
toolbar=Ctrl+T
toolbar_dock=Shift+T
hud=T
pixel=P
histogram=H
channel=C
animate=A
step=Period
background=B
smooth=S
fullscreen=F11,Enter
copy_image=Ctrl+C
copy_path=Ctrl+Shift+C
reveal=Ctrl+E
open=Ctrl+O
delete=Delete
config=F2
help=F1
quit=Esc,Q
";

pub struct Cfg { pub map: HashMap<String, String> }

pub fn ini_candidates() -> (PathBuf, PathBuf) {
    let exe = std::env::current_exe().unwrap_or_default();
    let beside = exe.with_file_name("fastviewer.ini");
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("FastViewer").join("fastviewer.ini");
    (beside, appdata)
}
/// %FASTVIEWER_INI% if set, else a file beside the exe (portable mode) if present, else %APPDATA%.
pub fn active_ini() -> PathBuf {
    if let Some(p) = std::env::var_os("FASTVIEWER_INI") { return PathBuf::from(p); }
    let (a, b) = ini_candidates();
    if a.exists() { a } else { b }
}

/// Writes settings back into the ini, keeping comments and other lines. Creates the file (from the
/// commented defaults) if it does not exist. Keys that are missing are inserted before [keys].
pub fn save_settings(kv: &[(&str, String)]) {
    let path = active_ini();
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_INI.to_string());
    text = text.trim_start_matches('\u{feff}').to_string();
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    for (k, v) in kv {
        let mut in_keys = false;
        let mut done = false;
        for l in lines.iter_mut() {
            let t = l.trim();
            if t.starts_with('[') { in_keys = t.eq_ignore_ascii_case("[keys]"); continue; }
            if in_keys || t.starts_with(';') { continue; }
            if let Some((lk, _)) = t.split_once('=') {
                if lk.trim().eq_ignore_ascii_case(k) { *l = format!("{}={}", k, v); done = true; break; }
            }
        }
        if !done {
            let at = lines.iter().position(|l| l.trim().eq_ignore_ascii_case("[keys]")).unwrap_or(lines.len());
            lines.insert(at, format!("{}={}", k, v));
        }
    }
    if let Some(d) = path.parent() { let _ = std::fs::create_dir_all(d); }
    let _ = std::fs::write(&path, lines.join(nl) + nl);
}

impl Cfg {
    fn parse(&mut self, text: &str) {
        let mut sec = String::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with(';') { continue; }
            if l.starts_with('[') { sec = l.trim_matches(|c| c == '[' || c == ']').to_lowercase(); continue; }
            if let Some((k, v)) = l.split_once('=') {
                let k = k.trim().to_lowercase();
                let key = if sec == "keys" { format!("k.{}", k) } else { k };
                self.map.insert(key, v.trim().to_string());
            }
        }
    }
    pub fn load() -> Cfg {
        let mut c = Cfg { map: HashMap::new() };
        c.parse(DEFAULT_INI);
        if let Ok(t) = std::fs::read_to_string(active_ini()) { c.parse(t.trim_start_matches('\u{feff}')); }
        c
    }
    pub fn text(&self, k: &str) -> String { self.map.get(k).cloned().unwrap_or_default() }
    pub fn num(&self, k: &str) -> f32 {
        self.map.get(k).and_then(|v| v.parse().ok())
            .or_else(|| { let mut d = Cfg { map: HashMap::new() }; d.parse(DEFAULT_INI); d.map.get(k).and_then(|v| v.parse().ok()) })
            .unwrap_or(1.0)
    }
    pub fn flag(&self, k: &str) -> bool { self.map.get(k).map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false) }
    pub fn color(&self, k: &str) -> D2D1_COLOR_F { self.color_a(k, 1.0) }
    pub fn color_a(&self, k: &str, a: f32) -> D2D1_COLOR_F {
        let s = self.map.get(k).map(|s| s.trim_start_matches('#')).unwrap_or("ff00ff");
        let v = u32::from_str_radix(s, 16).unwrap_or(0xff00ff);
        clr(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, a)
    }
    pub fn keys_for(&self, action: &str) -> String { self.map.get(&format!("k.{}", action)).cloned().unwrap_or_default() }

    pub fn bindings(&self) -> Vec<(u32, u8, String)> {
        let mut out = vec![];
        for (a, _) in ACTIONS {
            for spec in self.keys_for(a).split(',') {
                let mut mods = 0u8;
                let mut key = "";
                for part in spec.split('+') {
                    match part.trim().to_lowercase().as_str() {
                        "ctrl" => mods |= 1,
                        "shift" => mods |= 2,
                        "alt" => mods |= 4,
                        _ => key = part.trim(),
                    }
                }
                for vk in vk_of(key) { out.push((vk, mods, a.to_string())); }
            }
        }
        out
    }
}

fn vk_of(name: &str) -> Vec<u32> {
    let n = name.to_lowercase();
    if n.len() == 1 {
        let c = n.as_bytes()[0];
        if c.is_ascii_lowercase() { return vec![(c - 32) as u32]; }
        if c.is_ascii_digit() { return vec![c as u32, 0x60 + (c - b'0') as u32]; }
    }
    if let Some(f) = n.strip_prefix('f') { if let Ok(k) = f.parse::<u32>() { if (1..=12).contains(&k) { return vec![0x6F + k]; } } }
    match n.as_str() {
        "left" => vec![0x25], "up" => vec![0x26], "right" => vec![0x27], "down" => vec![0x28],
        "home" => vec![0x24], "end" => vec![0x23], "pageup" => vec![0x21], "pagedown" => vec![0x22],
        "space" => vec![0x20], "enter" | "return" => vec![0x0D], "esc" | "escape" => vec![0x1B],
        "tab" => vec![0x09], "backspace" => vec![0x08], "delete" | "del" => vec![0x2E],
        "plus" => vec![0xBB, 0x6B], "period" => vec![0xBE], "comma" => vec![0xBC], "minus" => vec![0xBD, 0x6D],
        _ => vec![],
    }
}
