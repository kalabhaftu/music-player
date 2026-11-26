# Third-party notices

Music Player (Rust + Slint edition) includes or dynamically loads the following third-party components. This is an attribution list, not a complete dependency inventory; build-only and test-only packages are excluded. Each component remains under its own license, and this notice does not replace the license text shipped with the corresponding crate or runtime.

| Component | Use | Version | License | Source |
|---|---|---|---|---|
| Slint | Native UI toolkit and rendering | 1.18.1 | GPL-3.0 / Royalty-Free | [Slint](https://slint.dev/) |
| LibVLC | Local media playback engine and codecs included by the Windows package | 3.0.24 | LGPL-2.1-or-later; bundled modules and plugins may carry additional notices | [VideoLAN VLC](https://code.videolan.org/videolan/vlc) |
| Lofty | Audio metadata reading | 0.25.5 | MIT OR Apache-2.0 | [Lofty](https://github.com/Serial-ATA/lofty-rs) |
| rusqlite | Local SQLite database binding | 0.38.0 | MIT | [rusqlite](https://github.com/rusqlite/rusqlite) |
| SQLite | Embedded database engine (bundled with rusqlite) | Bundled | Public domain | [SQLite](https://www.sqlite.org/copyright.html) |
| image | Artwork decoding and PNG/JPEG processing | 0.25.10 | MIT OR Apache-2.0 | [image-rs](https://github.com/image-rs/image) |
| windows / windows-sys | Native Windows APIs (SMTC, tray, COM, Win32) | 0.62.2 / 0.60 | MIT OR Apache-2.0 | [windows-rs](https://github.com/microsoft/windows-rs) |
| notify | Live filesystem watcher for library folders | 8.2.0 | CC0-1.0 | [notify](https://github.com/notify-rs/notify) |

The end-user app includes this notice, the [LGPL 2.1 license text](third-party-licenses/LGPL-2.1.txt), and the MIT project license (`LICENSE`). Before a stable release, verify the module-specific notices in the exact bundled LibVLC runtime and include them with the release. GPL-only LibVLC packages are excluded. See the official [GNU LGPL 2.1 text](https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html) and [VLC third-party licenses](https://www.videolan.org/legal.html).

Trademark and product names belong to their respective owners. No endorsement is implied.
