# Music Player

Music Player is a Windows desktop app for playing and organizing a local music library. It uses a WinUI 3 window with a packaged WebView2 interface. C# remains responsible for playback, library access, playlists, settings, filesystem operations, and Windows integrations.

## Features

- Search and browse songs, albums, artists, genres, favorites, history, and playlists.
- Scan local folders with exclusions, progress, pause, resume, and cancel controls.
- Manage the queue, shuffle, repeat, A–B repeat, volume, crossfade, output device, and ten-band equalizer.
- Edit supported tags with backups; view lyrics, search LRCLIB on request, and inspect track details.
- Find exact duplicate files, inspect their locations, and open them in File Explorer.
- Use Windows media controls, media keys, file activation, optional tray behavior, and paused-session restore.

## Build

Use Windows 10 version 1809 or later, the .NET 10 SDK, and Windows SDK 10.0.26100 or later.

```powershell
dotnet restore MusicPlayer.sln
dotnet build src/MusicPlayer.App/MusicPlayer.App.csproj -c Release -p:Platform=x64 -p:RuntimeIdentifier=win-x64
dotnet run --project src/MusicPlayer.App/MusicPlayer.App.csproj
```

For ARM64, use `-p:Platform=ARM64 -p:RuntimeIdentifier=win-arm64`. The core tests can be run with `dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj -c Release`.

## Local files and network access

The SQLite database, settings, artwork cache, tag backups, and logs are stored under `%LOCALAPPDATA%\MusicPlayer`. The app has no account, streaming service, or cloud sync. Lyrics can come from sidecar `.lrc` files or supported embedded audio tags; LRCLIB receives only title and artist after a user starts a search. The optional GitHub update check runs at most once every six hours after a successful response and can be disabled in Settings. Updates open the release page and are never installed automatically.

Windows format support depends on the bundled LibVLC modules and the specific file/device combination. See [the format matrix](docs/format-matrix.md). Release signing requirements and current signing status are documented in [the release process](docs/release.md). Remaining migration and release checks are listed in [project status](docs/project-status.md).

## Third-party components

The app uses WinUI 3 from the Windows App SDK, WebView2 for its interface, LibVLCSharp and LibVLC for media playback, TagLib# for audio metadata, and Microsoft.Data.Sqlite with SQLite for its local library. The WebView2 Evergreen Runtime is supplied separately by Microsoft. See [ThirdPartyNotices.md](ThirdPartyNotices.md) for versions, licenses, and bundled notices.

## License

The source is MIT licensed. Third-party components retain their own license terms; see [ThirdPartyNotices.md](ThirdPartyNotices.md).

## Contributing and support

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Use the GitHub issue forms to [report a bug](https://github.com/kalabhaftu/music-player/issues/new?template=bug_report.yml) or [request a feature](https://github.com/kalabhaftu/music-player/issues/new?template=feature_request.yml). For troubleshooting and safe log sharing, see [SUPPORT.md](SUPPORT.md). Security issues should be reported privately as described in [SECURITY.md](SECURITY.md).
