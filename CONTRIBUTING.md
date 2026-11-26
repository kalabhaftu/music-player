# Contributing

Thanks for helping improve Music Player. Bug reports, focused feature proposals, documentation fixes, and code contributions are welcome.

## Before opening an issue or pull request

- Search existing issues and pull requests for the same problem.
- Use the bug or feature form so reports include the details needed to reproduce them.
- Keep changes focused and describe the user-visible behavior they affect.
- Never attach credentials, private music, or unredacted logs. Music Player logs can contain local file paths.

## Development setup

Music Player is a native Windows desktop app built with **Rust**, **Slint UI**, **LibVLC**, and **SQLite** (at the repository root). See [AGENTS.md](AGENTS.md) and [docs/development.md](docs/development.md) for architecture rules, SDK requirements, and commands.

The usual checks for the primary Rust workspace are:

```powershell
cargo check --all-targets
cargo test --workspace
cargo build --release --bin music-player-app
```

The retired C# / WinUI 3 / WebView2 app and experimental Avalonia prototype are preserved under `legacy/` (`legacy/MusicPlayer.sln`) for historical reference and behavioral parity checks.

## Pull requests

1. Create a focused branch and make the smallest change that addresses the issue.
2. Follow the test separation and code quality rules in [AGENTS.md](AGENTS.md).
3. Run `cargo test --workspace` and `cargo build --release --bin music-player-app`, and record the exact checks in your pull request.
4. Update `CHANGELOG.md` under `[Unreleased]` when the change affects users.
5. Describe any Windows-only behavior that still needs manual verification.
6. Keep crate additions justified and call out new runtime files or license obligations.

Maintainers review changes for correctness, data safety, accessibility, and fit with the local-first design. A pull request may be revised or declined when its behavior is unclear or cannot be supported safely.
