# FastViewer - spec

**Goal:** the fastest-opening, lightest, most configurable image viewer for Windows.

## Priorities
1. **Startup speed** - double-click to image on screen in ~50-65 ms.
2. **Light** - single ~0.55 MB exe, no runtime/DLL dependencies, no codecs bundled (uses Windows' built-in WIC).
3. **Formats** - PNG, JPEG, WebP, GIF (first frame), BMP, TIFF, ICO, HEIC/AVIF/JXL (where the OS codec is installed), JXR, DDS. EXIF orientation applied.
4. **Metadata panel (I)** - file info, dimensions, format, pixel format, DPI, EXIF camera/lens/exposure/ISO/focal length/date/GPS, PNG text chunks (e.g. AI generation parameters).
5. **Gallery strip (G)** - folder thumbnails, natural sort, lazy background loading; click or wheel to navigate. Next/previous images are prefetched.
6. **Inspection tools** - zoom to cursor, pan, pixel inspector + 11x11 loupe (P), histogram (H), channel isolation R/G/B/A/Luma (C), measure tool (Shift+drag), rotate/flip, checker/light/black backgrounds (B), pixel-vs-smooth scaling (S).
7. **Customizable** - `fastviewer.ini` (beside the exe, or %APPDATA%\FastViewer): colors, font, startup panels, zoom step, wheel behavior, and every keyboard shortcut. F2 opens it, F1 shows shortcuts.

## How it is fast
- Decode starts on a worker thread before the window is created.
- The first frame is drawn with plain GDI as soon as pixels exist; Direct2D/GPU init (slow on some drivers) happens on another thread, then takes over.
- WIC does decoding with no extra code in the exe; static CRT, LTO, panic=abort.

## Non-goals (v1)
Editing, animated GIF playback, RAW camera files (need the OS RAW codec), color-management/ICC.

## Added in v1.1
- **Animated GIF** - frames composed with proper offsets/transparency/disposal and per-frame delays; `A` play/pause, `.` step a frame. Playback runs on its own thread and costs nothing for still images.
- **Large drag & drop / command-line lists** - drop any number of files and folders (mixed, from different folders); the gallery becomes exactly that list (folders expanded, duplicates removed, natural sort). The drop buffer is parsed in one pass, so 50,000 paths take ~1.4 s. A single dropped file still opens its whole folder.
- **App icon + manifest** - generated at build time (`build.rs`, no SDK needed); per-monitor DPI, long paths, UTF-8.
- **Installer** - `install.ps1` / `uninstall.ps1` (per-user, no admin), or `fastviewer.exe --register` / `--unregister` (`--quiet` suppresses the message box).
- **Self test** - `tools\make-testdata.ps1` + `tools\selftest.ps1` drive the real exe through its own window (133 checks: formats, EXIF/GPS/PNG text, navigation, zoom/pan, rotate, channels, pixel inspector, measure, fullscreen, clipboard, delete, config, GIF, 50k-file drops, registration, startup time). Test hooks only exist when `FASTVIEWER_TEST` is set.

## Added in v1.2 - flexible UI
- **Toolbar with a button for every function** (39). Hover for a tooltip with the description and shortcut; toggle buttons light up. It can sit on any edge (`Shift+T` / the B-dock button), wraps onto more rows/columns when space is tight, hides with `Ctrl+T`, and `toolbar_buttons=` in the ini picks which buttons appear and in what order.
- **Docking layout** - toolbar, gallery and metadata panel each dock to any edge (top / bottom / left / right) and never overlap; the image fits whatever is left. Drag a panel's edge to resize it.
- **Gallery** - *strip* (a row or column centred on the current image) or *grid* (wrapping, scrolling, with file names), on any edge. Thumbnail size 32-320 via buttons, `Ctrl+Plus/Minus`, `Ctrl+wheel`, or by dragging the panel edge (thumbnails are regenerated sharper when enlarged). **Browse** (`Tab`) turns the whole window into a thumbnail grid: arrows/Up/Down move, double-click opens.
- **Metadata panel** - on any edge; on top/bottom it flows into columns. Hide a field with the x on its row; `Ctrl+I` opens a chooser listing every field with checkboxes.
- **Layout is remembered** - changes are written back to `fastviewer.ini` (comments and other settings preserved). `FASTVIEWER_INI` overrides the settings path.
- Tests: `tools\selftest_ui.ps1` (108 checks) alongside `tools\selftest.ps1` (133).
