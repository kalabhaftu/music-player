Runtime bundles include `LGPL-2.1.txt` beside the application and `ThirdPartyNotices.md` at the application root. LibVLC and TagLib# are linked dynamically; their LGPL terms apply to those components. SQLite is public domain. The Windows App SDK and Microsoft.Data.Sqlite are MIT licensed. Test-only packages are not included in end-user packages.

The bundled LibVLC NuGet package must be checked at release time for module-specific license files and attribution requirements. GPL-only package variants must not be substituted for `VideoLAN.LibVLC.Windows`.
