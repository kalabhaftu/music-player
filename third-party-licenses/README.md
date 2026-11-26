Runtime bundles include `LGPL-2.1.txt` beside the application and `ThirdPartyNotices.md` at the application root. LibVLC (`libvlc.dll`, `libvlccore.dll`, and `plugins/`) is loaded dynamically; its LGPL-2.1-or-later terms apply to those components. SQLite is public domain. Rust crate dependencies are MIT, Apache-2.0, CC0-1.0, or Slint-licensed. Test-only packages are not included in end-user packages.

The bundled LibVLC runtime must be checked at release time for module-specific license files and attribution requirements. GPL-only LibVLC builds must not be substituted.
