# Archive Notice: MusicPlayer.App (WinUI 3 + WebView2)

> **Status:** **ARCHIVED / LEGACY**  
> **Successor (Primary):** [Repository Root (`crates/`)](../../../)  
> **Native .NET Prototype:** [`legacy/src/MusicPlayer.Desktop/`](../MusicPlayer.Desktop/)

---

### Overview

`MusicPlayer.App` is the original Windows desktop application built with **WinUI 3** hosting a local **Microsoft Edge WebView2** frontend (`WebUI/` folder containing HTML5, Vanilla JavaScript, and CSS).

### Why It Is Archived

The project has transitioned to a native, lightweight, memory-efficient desktop architecture:
1. **Primary Active Application:** **[Repository Root (`crates/`)](../../../)**
   - Built with **Rust + Slint UI** and LibVLC.
   - Significantly faster startup, <50MB RAM footprint, zero Chromium/WebView2 runtime dependencies, native COM shell drop target, and native Win32 windowing.
2. **Avalonia Native Prototype:** **[`src/MusicPlayer.Desktop/`](../MusicPlayer.Desktop/)**
   - Built with **Avalonia UI 11.2** on **.NET 10** as a pure native XAML MVVM reference sharing `MusicPlayer.Core`.

### Preserving Source Code

The complete source code of `MusicPlayer.App` is retained in this repository for historical reference, behavioral regression testing against legacy baselines, and compatibility verification.
