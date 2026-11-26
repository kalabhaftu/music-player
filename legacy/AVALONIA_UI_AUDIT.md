# Avalonia Native UI vs. WebView2 UI — Comprehensive Gap Analysis & Audit

> **Audit Date:** November 15, 2026  
> **Target Branch:** `avalonia-native-ui`  
> **Source Comparison:** `src/MusicPlayer.Desktop` (Avalonia 11.2 XAML) vs. `src/MusicPlayer.App` (WinUI 3 + WebView2)  

---

## Executive Summary

While the Avalonia native UI (`MusicPlayer.Desktop`) has achieved foundational visual alignment with the WebView2 interface (100% vector SVG paths, exact design tokens, dark palette `#070807`, Inter typography, and shared SQLite database), it currently functions as a **playback-capable prototype** rather than a full feature-complete replacement for the WebView2 application.

This document catalogs every functional, visual, audio engine, and system integration gap that must be addressed to reach 1:1 parity.

---

## 1. Playback Engine & Audio Features

| Feature | WebView2 App (`MusicPlayer.App`) | Avalonia Native (`MusicPlayer.Desktop`) | Status / Notes |
| :--- | :--- | :--- | :--- |
| **Media Engine** | LibVLC via `PlaybackService.cs` | LibVLC via `DesktopPlaybackService.cs` | **Active** (Resolved media disposal lifetime bug). |
| **Dual-Player Gapless Crossfade** | Dual `MediaPlayer` (`_active` & `_spare`) with pre-buffering next track | Single `MediaPlayer` instance | **Missing**. There is an audible gap between tracks; no pre-buffering. |
| **10-Band Graphic Equalizer** | Native LibVLC equalizer DSP with preamp and 18 presets | Equalizer icon present; no DSP filter wired | **Missing**. Sliders and preset engine not hooked to LibVLC equalizer. |
| **A–B Looping** | Fully implemented state machine (`Off` -> `A Set` -> `A-B Active`) | Button exists in UI; no logic | **Missing**. Clicking A–B button currently does nothing. |
| **Audio Output Device Selection** | Enumerates devices; DirectSound vs WASAPI routing | Uses OS default audio device | **Missing**. |
| **DSD / DFF Audio Support** | Custom `DffStreamInput` parser for DSD playback | Relies on default LibVLC file stream | **Partial**. DFF files with custom sample rates may fail. |
| **Playback Speed / Pitch** | Variable rate (0.5x – 2.0x) | Fixed 1.0x | **Missing**. |

---

## 2. Views & Navigation Parity

### A. Playlists View
- [x] **Smart Playlists**: Monogram cards (Most Played, With Lyrics, Recently Played) present and clickable.
- [ ] **Custom Playlist Creation**: WebView2 has a modal dialog to name and create playlists; Avalonia has button only.
- [ ] **M3U / M3U8 Import & Export**: File picker and parser not yet wired to `DesktopLibraryService`.
- [ ] **Playlist Reordering**: Drag-and-drop track reordering within playlists is missing.

### B. Songs / Favorites / Search View
- [x] **Table Header & Columns**: `#`, `Title` (with thumbnail), `Album`, `Time`, and Heart favorite toggle.
- [x] **Playback Trigger**: Double-tap on row plays track.
- [x] **Track Artwork Thumbnail**: Displays embedded/cached album art or monogram fallback gradient.
- [ ] **Context Menu (Right-Click)**: Missing context menu (`Play next`, `Add to queue`, `Add to playlist`, `Edit tags`, `Reveal in File Explorer`).
- [ ] **Column Resizing & Sorting**: WebView2 allows clicking column headers to sort by Title, Artist, Album, Duration, or Date Added.
- [ ] **Multi-selection**: Selecting multiple tracks (Shift/Ctrl + Click) to enqueue or batch edit.

### C. Lyrics View
- [x] **Synced Lyrics Display**: Active line highlighted in bright white bold text; inactive lines dimmed.
- [x] **Seek on Click**: Tapping any line seeks playback to that line's timestamp.
- [x] **Jump to Current Button**: Floating circular button with sync icon present.
- [ ] **Smooth Auto-Scroll**: Avalonia currently lacks auto-scroll centering the active line in the viewport as the song progresses.
- [ ] **Online Lyrics Fetching (LRCLIB)**: WebView2 queries the LRCLIB REST API when local `.lrc` or embedded tags are absent; Avalonia currently only reads local sidecars/tags.
- [ ] **"Edit Lyrics" Modal**: Dialog to adjust timestamps and line text.
- [ ] **"Find Lyrics" Modal**: Manual search dialog to match online lyrics.

### D. Folders View
- [x] List of configured music folders from SQLite store.
- [ ] **Folder Browser**: Navigating subfolders and playing directory trees.
- [ ] **Folder Picker Dialog**: Adding new library roots via Windows folder picker (`StorageProvider`).
- [ ] **Folder Exclusion Rules**: Excluding hidden folders or custom glob patterns.

### E. Settings View
- [ ] **Library Management**: Manual "Rescan Library" trigger with progress percentage and file counter.
- [ ] **Exclusion Patterns**: Configurable list of ignored folders/files.
- [ ] **Theme Settings**: Toggle between Dark, Light, and Dynamic Album Art Theming.
- [ ] **Audio Settings**: Device dropdown, buffer size, replay gain.
- [ ] **Tag Editor**: Full metadata editing dialog with TagLibSharp.

---

## 3. UI/UX & Visual Experience

| Element | WebView2 App | Avalonia Native | Status / Notes |
| :--- | :--- | :--- | :--- |
| **Vector Icons** | 35+ SVG icons (`app.js`) | 35+ `<StreamGeometry>` icons (`App.axaml`) | **100% Parity**. All emojis eliminated. |
| **Color Tokens** | `#070807` dark theme | Exact match in `App.axaml` | **100% Parity**. |
| **Dynamic Art Theming** | Background, sidebars, and panels dynamically tint to dominant colors of current cover art | Fixed `#070807` background | **Missing**. `ArtworkPalette` extractor exists in Core but is not bound to UI brushes. |
| **Modal Dialogs** | Generic modal backdrop with blur and keyboard trap | None | **Missing**. |
| **Toast Notifications** | Slide-up status notification banner | None | **Missing**. |
| **Queue Reordering** | Drag-and-drop reordering in Now Playing panel | Read-only ListBox with Clear button | **Partial**. |
| **Immersive Player Backdrop** | Blurred full-screen album art backdrop with CSS glow | Semi-transparent solid `#F5070807` | **Needs Polish**. Add blurred backdrop image. |

---

## 4. Windows System & OS Integration

- [ ] **System Media Transport Controls (SMTC)**:
  - WebView2 integrates with Windows SMTC (hardware media keys, lock screen media controls, Bluetooth headset play/pause/skip).
  - Avalonia currently has no SMTC handler.
- [ ] **Discord Rich Presence (RPC)**:
  - Shows current song title, artist, album, and live playback progress in Discord profile.
- [ ] **File Associations & Shell Launching**:
  - Double-clicking an audio file in Windows Explorer (`.mp3`, `.flac`, `.wav`) to launch and play directly in the app.
- [ ] **Taskbar Thumbnail Toolbar**:
  - Hovering the Windows taskbar icon showing mini Play/Pause, Previous, and Next buttons.
- [ ] **Window Dragging & Custom Titlebar**:
  - Seamless borderless titlebar with custom drag zones and Windows 11 snap layout integration.

---

## 5. Priority Roadmap for Avalonia Parity

1. **Phase 1: Essential Usability (Highest Priority)**
   - Wire `StorageProvider` folder picker to add/remove library folders in Folders view.
   - Implement Track Context Menu (`Play Next`, `Add to Queue`, `Open Containing Folder`).
   - Add auto-scrolling to Lyrics view (`ScrollIntoView` on active line change).
   - Implement online LRCLIB lyrics lookup fallback.

2. **Phase 2: Audio Engine Enhancements**
   - Implement dual-player prebuffering for gapless playback in `DesktopPlaybackService`.
   - Hook LibVLC Equalizer to UI sliders and preset selector.
   - Implement A–B loop timer logic.

3. **Phase 3: OS Integration & Polish**
   - Add Windows SMTC (`SystemMediaTransportControls`) for keyboard media keys.
   - Implement Dynamic Artwork Theming (binding `ArtworkPalette` tones to window and panel brushes).
   - Add blurred backdrop to Immersive Player.
