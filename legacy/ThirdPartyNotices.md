# Third-party notices

Music Player includes or dynamically links the following third-party components. This is an attribution list, not a complete dependency inventory; build tools and test-only packages are excluded. Each component remains under its own license, and this notice does not replace the license text shipped with the corresponding package.

| Component | Use | Version | License | Source |
|---|---|---|---|---|
| Microsoft Edge WebView2 SDK | Embedded WebView interface | 1.0.3179.45 | [BSD-3-Clause](third-party-licenses/WebView2.txt) | [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) |
| LibVLC | Local media playback engine and codecs included by the Windows package | 3.0.24 | LGPL-2.1-or-later; bundled modules and plugins may carry additional notices | [VideoLAN VLC](https://code.videolan.org/videolan/vlc) |
| LibVLCSharp | .NET binding for LibVLC | 3.10.1 | LGPL-2.1-or-later | [LibVLCSharp](https://github.com/videolan/libvlcsharp) |
| TagLib# | Audio metadata reading and writing | 2.3.0 | LGPL-2.1-or-later | [TagLib#](https://github.com/mono/taglib-sharp) |
| Microsoft.Data.Sqlite | Local SQLite access | 10.0.12 | MIT | [Microsoft.Data.Sqlite](https://github.com/dotnet/efcore) |
| SQLitePCLRaw | Native SQLite provider and platform binding | 2.1.12 (transitive) | Apache-2.0 | [SQLitePCLRaw](https://github.com/ericsink/SQLitePCL.raw) |
| SQLite | Database engine used by SQLitePCLRaw | Bundled with SQLitePCLRaw 2.1.12 | Public domain | [SQLite](https://www.sqlite.org/copyright.html) |
| Windows App SDK WinUI and dependencies | Native WinUI 3 application framework | 1.8.260803003 (WinUI) | Microsoft Software License Terms | [Windows App SDK](https://github.com/microsoft/WindowsAppSDK) |
The end-user app includes this notice, the [LGPL 2.1 license text](third-party-licenses/LGPL-2.1.txt), the [WebView2 SDK license](third-party-licenses/WebView2.txt), and the MIT project license. Package versions reflect the versions resolved for the app; transitive components may have separate versions and license terms. Before a stable release, verify the module-specific notices in the exact bundled LibVLC package and include them with the release. GPL-only LibVLC packages are excluded. See the official [GNU LGPL 2.1 text](https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html) and [VLC third-party licenses](https://www.videolan.org/legal.html).

Trademark and product names belong to their respective owners. No endorsement is implied.
