# FastViewer

Ultra-fast, tiny, customizable image viewer for Windows (Rust, raw Win32, no GUI framework). See `SPEC.md` for the feature list.
Top priority: **startup speed** (double-click to pixels on screen, currently ~50-60 ms). Don't regress it.

## Build / run / test
- Build: `cargo build --release` (exe: `target\release\fastviewer.exe`, ~0.75 MB, static CRT, no DLL deps).
- Run: `fastviewer.exe <file> [more files/folders...]`. Other flags: `--register` / `--unregister` (per-user file associations, `--quiet` hides the message box).
- Install: `install.ps1 [-Dest dir]` / `uninstall.ps1`.
- Tests (PowerShell, drive the real exe through its own HWND, capture with `PrintWindow`, never touch the desktop):
  1. `tools\make-testdata.ps1 -Out <dir>` generates test images (EXIF/GPS jpeg, PNG tEXt, animated GIF, alpha PNG, WebP, 5000 small files).
  2. `tools\selftest.ps1 -Data <dir> -Out <workdir>` - core features (133 checks).
  3. `tools\selftest_ui.ps1 -Data <dir> -Out <workdir>` - toolbar/gallery/panel layout (108 checks; reuses helpers from selftest.ps1).
  - Run suites detached (`Start-Process powershell ... -RedirectStandardOutput`) - they take several minutes and a foreground tool call times out at 10 min.
  - Both must stay green before committing UI/behavior changes.

## Architecture (`src/`)
- `main.rs` - `App` state, window proc, view/zoom/pan math, navigation, painting, test hooks. One global `APP` pointer (single UI thread).
- `ui.rs` - docking layout engine (`layout()` carves toolbar -> gallery -> info -> view), toolbar, gallery strip/grid/browse, metadata panel with hideable fields, hit-testing, splitters, `save_layout()`.
- `decode.rs` - decoding via **Windows Imaging Component** (no codecs bundled), EXIF/GPS/PNG-text metadata, GIF compositing + animation thread, folder/list building (`spawn_list_multi`).
- `config.rs` - `fastviewer.ini` defaults (`DEFAULT_INI`), `ACTIONS` table, key bindings, `save_settings()` (edits the ini in place, keeps comments).
- `shell.rs` - registry association, clipboard, recycle bin, dialogs, opt-in startup timing (`FASTVIEWER_TIMING=1`).
- `build.rs` - generates the app icon + manifest as a `.res` by hand (no rc.exe/SDK needed).

## Why startup is fast (keep these properties)
- Decode starts on a worker thread **before** the window exists; window shows first.
- First frame is drawn with plain **GDI** (`paint_gdi`); Direct2D/DirectWrite init (~250 ms on some GPUs) runs on its own thread (`spawn_gpu`) and takes over via `WM_GPU`.
- Never block the UI thread on I/O or GPU init. Thumbnails/prefetch/animation all use worker threads posting `WM_APP+n` messages with boxed payloads.

## Conventions / gotchas
- Adding a user-facing action: add to `ACTIONS` + default `[keys]` in `config.rs`, a label in `ui::button_label`, handle in `App::dispatch` (or `ui_action`), and optionally add to `toolbar_buttons`. The self tests assert every action has a toolbar button.
- Layout/size settings persist automatically (`save_layout`). `FASTVIEWER_INI` overrides the settings path - tests rely on it so they never touch the user's real ini.
- Test hooks (state dump `WM_APP+11`, fake modifiers `+9`, real `WM_DROPFILES` injection `+10`, no-focus/bottom-of-z-order window) are **only active when `FASTVIEWER_TEST` is set**. Extend `dump_state` / `dump_ui` rather than adding screen-scraping to tests.
- Large drops: parse the HDROP buffer linearly (`read_drop`); `DragQueryFileW(i)` is O(n^2).
- `bool`s print as `true`/`false` in the state dump; PowerShell `-eq` is case-insensitive so tests compare against `'True'`.
- Source files must stay plain ASCII/UTF-8 **without BOM**; use `'\u{...}'` escapes for special chars. Don't use `perl -pi` with non-ASCII replacements (it double-encodes) and don't round-trip sources through PowerShell `Set-Content -Encoding utf8` (adds a BOM).
- In PowerShell test scripts variable names are case-insensitive and script-scoped: don't reuse `$t0`, `$Out`, `$out` etc. (the helpers use `$script:t0`).
- Mouse/keyboard in tests are posted messages: any real input during a run can corrupt it, hence the no-activate test window.
- Release profile: `opt-level=3`, fat LTO, `panic=abort`, stripped. `.cargo/config.toml` forces `+crt-static`.
