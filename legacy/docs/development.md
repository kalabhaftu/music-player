# Development

## Requirements

- Windows 10 version 1809 or later for running the app.
- .NET 10 SDK.
- Windows 10 SDK 10.0.26100 or later and Windows App SDK NuGet packages for WinUI builds. The WinUI shell hosts the packaged WebView2 frontend.
- No Visual Studio IDE requirement. A Windows environment with the Windows SDK build targets is required; `dotnet` CLI alone on macOS/Linux cannot compile WinUI XAML.
- Internet access to restore packages from NuGet. The app uses Windows App SDK, LibVLC and TagLib packages; a restore failure leaves the WinUI app unbuilt.

## Commands

```powershell
dotnet restore MusicPlayer.sln
dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj -c Release
node --test tests/MusicPlayer.Ui.Tests/lyrics-theme.test.mjs tests/MusicPlayer.Ui.Tests/library-pagination.test.mjs
# Install FFmpeg for representative-format fixture generation.
& tests/MusicPlayer.Playback.Tests/Prepare-MediaFixtures.ps1
$env:MUSICPLAYER_MEDIA_FIXTURES="$PWD/artifacts/media-fixtures"
dotnet test tests/MusicPlayer.Playback.Tests/MusicPlayer.Playback.Tests.csproj -c Release
dotnet build src/MusicPlayer.App/MusicPlayer.App.csproj -c Release -p:Platform=x64 -p:RuntimeIdentifier=win-x64
dotnet run --project src/MusicPlayer.App/MusicPlayer.App.csproj
```

`dotnet run` restores packages as needed, builds the app, and starts that build from the checkout. It does not produce the release ZIP, setup installer, or MSIX bundle. The first launch discovers mounted local drives automatically; use **Folders** to choose locations explicitly. Do not launch another checkout while your existing Music Player instance is running.

## Local setup builds

Install Inno Setup 7 once and use PowerShell 7. `Build-MusicPlayerInstaller.bat` uses the existing
project certificate in the current user's certificate store and Windows SDK
`signtool`; it never exports the private key or installs certificate trust.
The first run publishes and signs one x64 payload. Later runs verify and reuse
that cached payload, then replace the same installer output:

```powershell
.\Build-MusicPlayerInstaller.bat
.\Build-MusicPlayerInstaller.bat -Install
# Include app-source or dependency changes, rather than only installer changes:
.\Build-MusicPlayerInstaller.bat -RebuildApp
```

The installer and its checksum are under `artifacts/local-installer`. The optional
installation check preserves saved user data and verifies all installed payload
files against the cache. It refuses to stop a running player. The builder works
from any current directory; keep the BAT in the checkout root. App dependencies
and the Microsoft WebView2 bootstrapper are downloaded once and retained.

## UI and application services

The WinUI window is the native shell for the local WebView2 UI. Frontend sources live under `src/MusicPlayer.App/WebUI`: `index.html`, shared stylesheets, and vanilla JavaScript modules for the typed bridge, library rendering, playback presentation, and app interactions. MSBuild embeds those files in the app assembly, and the C# WebView2 bridge serves them from memory, so releases do not expose a loose `WebUI` directory. The C# bridge remains explicit; query, playlist, settings, metadata, folder, scan, queue, and playback rules are kept in focused C# services/coordinators rather than duplicated in JavaScript.

For ARM64, replace `x64`/`win-x64` with `ARM64`/`win-arm64`. The automated Windows workflow builds both architectures and uploads per-architecture artifacts.

The core and its tests target .NET 10 without WinUI and can be tested on macOS or Linux. The Windows workflow builds and uploads a self-contained portable ZIP for x64 and ARM64. WebView2 rendering, audio devices, media keys, tray integration, installer behavior, MSIX installation and Windows 10/11 visual behavior must be checked on Windows hardware.

The separate native playback tests run on Windows x64 and native ARM64. They compile the app's playback service and queue coordinators directly and use the shipped LibVLC packages. Genuine fixtures cover all 19 supported audio extensions, metadata preservation, video rendering and subtitles, alongside silent WAV/dummy-output end/error, repeat, crossfade and seeking regressions. Both Windows architectures run these checks without an audio device. FFmpeg is required to prepare the format fixtures; their official sample downloads are cached in CI. See [the playback investigation](playback-investigation.md) for the regressions these tests reproduce.

## Configuration and data

Application data lives under `%LOCALAPPDATA%\MusicPlayer`; the SQLite database, artwork cache, recoverable tag backups and rotating logs are local. Logs are in `%LOCALAPPDATA%\MusicPlayer\Logs` and can be opened from Settings. They contain error details and may include local file paths, so review them before sharing. The app has no server or public API. Playlists import M3U and M3U8 files and export UTF-8 M3U8. Lyrics can be read from supported embedded tags or adjacent `.lrc` files. LRCLIB searches send title and artist when the user starts a search or enables automatic lyrics lookup; the optional GitHub release check uses conditional requests and does not download or install releases.

## Release signing

The release workflow contains signing and verification steps for the executable, setup installer, and MSIX bundle. They run after private preflight validates the persistent self-signed project certificate matching the MSIX publisher. Trust-dependent verification is confined to disposable runners; this is not publicly trusted publisher signing. No signed release should be claimed until the preflight and packaged-artifact verification have both succeeded. See [the release process](release.md).

## Search and memory limits

The UI reads track and playlist rows in 200-entry pages; album, artist and genre grids use 60-entry pages, and artist album panels use 30-entry pages. SQLite FTS5's trigram tokenizer indexes substring searches of three or more characters; one- and two-character searches keep the existing `LIKE` behavior. Artwork thumbnails are decoded to display size and the in-memory cache is capped at an estimated 64 MB. The 100,000-track Windows x64 benchmark records the first-page search p95 against a 250 ms target.

Scanning uses per-run database generations instead of holding the whole library in a scan map. Unavailable or incomplete roots do not trigger stale-row deletion. Successful scan completion prunes only cached artwork paths no longer referenced by indexed tracks.
