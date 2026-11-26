# Music Player Design System (Rust + Slint)

This document defines the exact visual tokens, layout geometry, and interaction rules implemented by the primary **Rust + Slint** desktop application (`crates/app/ui/`).

> All UI changes under `crates/app/ui/` must follow these tokens and patterns rather than introducing hardcoded colors or ad-hoc spacing.

---

## 1. Color & Theme System (`Palette` in `crates/app/ui/widgets.slint`)

All surfaces and text colors resolve dynamically through `Palette` based on:
- **Dark mode (default)**
- **Light mode** (`State.light-theme`)
- **Artwork-adaptive palette** (`State.artwork-palette` + `State.art-*`)
- **Optional window transparency** (`State.transparent-window`, `Palette.background-opacity`)

### Core Surface & Text Tokens

| Token | Dark (Default) | Light (`State.light-theme`) | Artwork-Adaptive (`State.artwork-palette`) |
| :--- | :--- | :--- | :--- |
| `Palette.canvas` | `#070807` | `#f0f2ef` | `State.art-background` |
| `Palette.surface` | `#090a09` | `#e7eae6` | `State.art-sidebar` |
| `Palette.main` | `#090a09` | `#f0f2ef` | `State.art-main` |
| `Palette.panel` | `#0e100f` | `#e7eae6` | `State.art-panel` |
| `Palette.panel-2` | `#131614` | `#dfe3de` | `State.art-panel` |
| `Palette.panel-3` | `#181b19` | `#d4d9d3` | `State.art-raised` |
| `Palette.raised` | `#171917` | `#e3e6e2` | `State.art-raised` |
| `Palette.hover` | `#212421` | `#d9ded8` | `State.art-hover` |
| `Palette.control` | `#242724` | `#dfe3de` | `State.art-control` |
| `Palette.row-hover` | `#131613` | `#e5e8e4` | `State.art-hover` |
| `Palette.text` | `#f7f8f6` | `#191d19` | `#f7f6f2` |
| `Palette.text-strong` | `#f2f4f1` | `#191d19` | `#f7f6f2` |
| `Palette.text-subtle` | `#cdd2cc` | `#343b34` | `#e0dfd8` |
| `Palette.muted` | `#a0a79f` | `#545c54` | `#d1d0c9` |
| `Palette.muted-2` | `#717970` | `#747c74` | `#aaa9a2` |
| `Palette.icon-muted` | `#939a93` | `#727a72` | `#939a93` |
| `Palette.disabled` | `#3a403a` | `#aab2aa` | `#3a403a` |
| `Palette.border` | `#ffffff14` | `#1219121f` | `#ffffff14` |
| `Palette.border-soft` | `#ffffff0d` | `#12191214` | `#ffffff0d` |
| `Palette.border-hover` | `#ffffff22` | `#1219122b` | `#ffffff22` |

### Accent & Selection

- **Default Accent:** `State.accent` = `#b7ff2d` with `State.accent-ink` = `#101210` (adjusted dynamically when `artwork-palette` is active to maintain readable contrast).
- **Selection:** `Palette.selection` uses `State.selection-color-value` when `State.selection-color-mode == "Custom"`, otherwise falls back to `State.accent`.

### Fallback Artwork Gradients (`Palette.ga / gb / badge`, indices `1..10`)

When embedded or local artwork is missing, `Palette.ga(i)` -> `Palette.gb(i)` provides 10 muted dark gradient pairs (e.g., `#1b1d26` -> `#3b4358`, `#162124` -> `#30494e`, `#251b1d` -> `#4e373c`), and `Palette.badge(i)` provides the middle stop for compact row thumbnails. Fallback art is purely visual and never written as library metadata.

---

## 2. Window & Layout Geometry

- **Main Window (`AppWindow` in `crates/app/ui/appwindow.slint`):**
  - Preferred size: `1024px × 640px`; minimum size: `720px × 500px`.
  - Standard native OS frame in normal browsing; frameless (`no-frame: true`) with custom `FramelessWindowChrome` only when `State.immersive-player-open` is active.
- **Left Navigation Rail (`shell/sidebar.slint`):**
  - `NavEntryRow`: `38px` item height (`65px` when preceded by a `27px` section header), `8px` border radius, `12px` horizontal padding, `13px` icon-to-label spacing.
  - Section headers: `10px`, weight `700`, `letter-spacing: 1.1px`, `Palette.muted-2`.
- **Bottom Player Bar (`shell/player_bar.slint`):**
  - Fixed `88px` height with a `1px` `Palette.border` top hairline.
  - Centered transport column: `clamp(240px, State.window-width * 0.38 * 1px, 480px)`; secondary controls (such as Shuffle) yield gracefully at `window-width <= 650px`.
- **Right Contextual Panel (`shell/right_panel.slint`):**
  - Hosts **Queue** (`QueueRow`: `52px` height, `6px` radius) and **Track Info** (`InfoRow`: `34px` height, `9px` label / `11px` value).
- **Content Grid Spacing:**
  - `Palette.card-gap(browse-width)` = `clamp(browse-width * 0.05px, 10px, 20px)`.
- **Dedicated Video Window (`video_window.slint` + `video_window.rs`):**
  - Separate window hosting a native Win32 child HWND surface attached to LibVLC, with speed, subtitle, repeat, snapshot, and fullscreen (`F11` / `Esc`) controls.

---

## 3. Motion, Tooltips & Accessibility

- **Transitions:** `350ms ease-in-out` on window/bar background theme changes; `100ms`–`120ms` on row and nav hover states.
- **Tooltips:** `DelayedTooltip` uses a `500ms` hover delay (`widgets.slint`) and cancels immediately on press.
- **Keyboard Navigation:** Global `FocusScope` in `AppWindow` handles `Space`, `Ctrl+K` (search), `F11` (fullscreen), `F1` / `?` (Help & Shortcuts modal), `Esc` (dismiss context menu -> modal -> immersive -> fullscreen -> scan drawer), plus arrow-key seeking/volume and `M`/`S`/`R`/`L`/`I`.
- **Error & Empty States:** Missing files, empty libraries, scan errors, empty queues, and unavailable audio endpoints render inline actionable states within the active view.
