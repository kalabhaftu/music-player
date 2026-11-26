# Contributing

Thanks for helping improve Music Player. Bug reports, focused feature proposals, documentation fixes, and code contributions are welcome.

## Before opening an issue or pull request

- Search existing issues and pull requests for the same problem.
- Use the bug or feature form so reports include the details needed to reproduce them.
- Keep changes focused and describe the user-visible behavior they affect.
- Never attach credentials, private music, or unredacted logs. Music Player logs can contain local file paths.

## Development setup

Music Player is a Windows desktop app built with .NET 10, WinUI, WebView2, and a local SQLite library. Core services and their xUnit tests can also run on Linux and macOS. See [docs/development.md](docs/development.md) for SDK requirements and commands.

The usual checks are:

```powershell
dotnet restore MusicPlayer.sln
dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj --configuration Release
dotnet build src/MusicPlayer.App/MusicPlayer.App.csproj --configuration Release -p:Platform=x64 -p:RuntimeIdentifier=win-x64
```

The Windows CI workflow also builds and smoke-starts x64 and ARM64 portable packages. Changes to WebView UI, playback, scanning, Windows shell behavior, packaging, or installation should include a short manual verification note for the affected Windows flows.

## Pull requests

1. Create a focused branch and make the smallest change that addresses the issue.
2. Run the relevant checks and record the commands and results in the pull request.
3. Update `CHANGELOG.md` under `[Unreleased]` when the change affects users.
4. Describe any Windows-only behavior that still needs manual verification.
5. Keep package additions justified and call out new runtime files or license obligations.

Maintainers review changes for correctness, data safety, accessibility, and fit with the local-first design. A pull request may be revised or declined when its behavior is unclear or cannot be supported safely.
