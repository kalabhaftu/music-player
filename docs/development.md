# Development

## Primary: Rust + Slint Workspace (Repository Root)

### Requirements

- Windows 10 version 1809 or later (for running the Windows desktop host).
- Rust toolchain (stable, MSVC target `x86_64-pc-windows-msvc`).
- Visual Studio Build Tools / Windows SDK (required by MSVC linker and Win32 APIs).
- LibVLC runtime (`libvlc.dll`, `libvlccore.dll`, and `plugins/`), bundled automatically by the installer or staged alongside the binary.

### Commands

```powershell
# Check all targets without producing binaries
cargo check --all-targets

# Run unit, integration, and benchmark validation tests
cargo test --workspace

# Build Debug and Release binaries
cargo build --workspace
cargo build --release --bin music-player-app

# Run the release binary
cargo run --release --bin music-player-app
```

### Local Windows Installer Builds (Rust)

Install Inno Setup 6/7 once and use PowerShell 7. `packaging/windows/Build-LocalInstaller.ps1` packages `target/release/music-player-app.exe` (renamed to `MusicPlayer.exe`), icons, and the bundled `libvlc` runtime via `packaging/windows/MusicPlayer.iss`:

```powershell
pwsh -NoProfile -File .\packaging\windows\Build-LocalInstaller.ps1 -RebuildApp
```

---

## Architecture Overview

- **`crates/core` (`music-player-core`):** Headless engine owning SQLite WAL persistence, tag reading/writing (`lofty`), incremental directory scanning and file watching, duplicate detection, LRCLIB lyrics lookup, 10-band equalizer, and LibVLC audio/video orchestration. Zero UI toolkit dependencies.
- **`crates/app` (`music-player-app`):** Slint UI (`crates/app/ui/`), controller state orchestration (`crates/app/src/controllers/`), native Win32 child HWND video window (`video_window.rs`), SMTC/tray/taskbar integration (`os.rs`, `os_shell.rs`), and COM drag-and-drop (`com_drop.rs`).

## Configuration and Data

Application data lives under `%LOCALAPPDATA%\MusicPlayer`:
- `music_catalog.db` (SQLite WAL database)
- Artwork cache, recoverable tag backups, and rotating logs (`%LOCALAPPDATA%\MusicPlayer\Logs`)
- Playlists import/export M3U and M3U8 files
- Lyrics are read from sidecar `.lrc` files or embedded tags; LRCLIB searches send only title and artist when triggered by the user or enabled in Settings.

---

## Legacy: C# / .NET 10 Workspace (`legacy/`)

The retired WinUI 3 + WebView2 app (`legacy/src/MusicPlayer.App/`), shared .NET core (`legacy/src/MusicPlayer.Core/`), and experimental Avalonia prototype (`legacy/src/MusicPlayer.Desktop/`) are preserved under `legacy/`:

```powershell
dotnet restore legacy/MusicPlayer.sln
dotnet test legacy/tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj -c Release
dotnet run --project legacy/src/MusicPlayer.App/MusicPlayer.App.csproj
dotnet run --project legacy/src/MusicPlayer.Desktop/MusicPlayer.Desktop.csproj
```
