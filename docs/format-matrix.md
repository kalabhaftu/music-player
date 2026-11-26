# Audio, Video, and Tag Format Matrix

The primary Rust application (`crates/core`) uses **LibVLC** for media playback and **Lofty** (plus container-specific writers in `crates/core/src/metadata/tag_editor.rs`) for metadata reading and writing.

## Supported Audio Containers

The library scanner indexes the following 19 audio extensions:

| Format Family | Extensions | Indexed | Tag Editing (`tag_editor.rs`) |
| :--- | :--- | :---: | :--- |
| **MPEG / RIFF / Lossless** | `.mp3`, `.wav`, `.wave`, `.flac` | Yes | Supported (ID3v2, RIFF INFO/ID3, Vorbis comments) |
| **Apple / AAC** | `.aac`, `.m4a`, `.m4b` | Yes | Supported (MP4 atoms; `.m4a`/`.m4b` standard fields) |
| **Xiph / Ogg** | `.ogg`, `.oga`, `.opus` | Yes | Supported (Vorbis / Opus comments) |
| **Windows / Legacy Lossless** | `.wma`, `.ape`, `.wv`, `.tta`, `.mpc` | Yes | `.wma` (ASF), `.ape` (APEv2); `.wv`/`.tta`/`.mpc` read-only or container-limited |
| **Apple AIFF / DSD** | `.aiff`, `.aif`, `.dsf`, `.dff` | Yes | `.aiff`/`.aif` (ID3/COMM), `.dsf` (ID3); `.dff` read-only |

Tag edits create automatic backups in `%LOCALAPPDATA%\MusicPlayer` before atomic replacement.

## Optional Video Files

Video extensions (`.mp4`, `.m4v`, `.mkv`, `.avi`, `.mov`, `.wmv`, `.webm`, etc.) are **excluded from library scans by default** until enabled under **Settings → Library → Scan exclusions**.

- Video files can still be opened directly from File Explorer at any time.
- Video playback opens in a dedicated native Win32 child HWND window (`VideoWindow`) using LibVLC `mmdevice` output, with live subtitle track selection (embedded + nearby `.srt`, `.ass`, `.ssa`, `.vtt` sidecars), playback speed (`0.5x`–`2x`), repeat, PNG snapshots, and `F11`/`Esc` fullscreen.
- Crossfading and automatic/online lyrics lookups are skipped for video files.

## Licensing

Playback support depends on the bundled LGPL LibVLC runtime (`libvlc.dll`, `libvlccore.dll`, and `plugins/`). No GPL-only LibVLC package is used. See [ThirdPartyNotices.md](../ThirdPartyNotices.md).
