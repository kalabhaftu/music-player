# Music Player (Rust Workspace) — Agent & Developer Rules

These rules apply to the primary Rust workspace at the repository root (`Cargo.toml`, `crates/`).

## 1. Architecture & Parity Principles
- **Headless Core (`crates/core`)**: Keep all database SQLite WAL queries, tag reading/writing, audio/video LibVLC orchestration, equalizer/crossfade state, lyrics parsing/fetching, and scanner pipelines strictly inside `crates/core` with zero UI toolkit dependencies.
- **Desktop Host (`crates/app`)**: `crates/app` owns the Slint UI, controller state orchestration, native Win32 shell hooks (SMTC, tray, taskbar buttons, child video HWND), and background task coordination.
- **Feature Parity**: Match behavior from the retired C# / WebUI baseline (`legacy/src/MusicPlayer.App/`) — including defaults (e.g., video extensions default to off during scans until enabled), crossfade rules, equalizer behavior, lyrics skip on video, keyboard shortcuts, and error reporting. Never silently strip or stub out required logic to make a build pass.

## 2. Test Organization (Strict Separation)
- **No Inline Test Code in Production Modules**: Do NOT place `#[cfg(test)] mod tests`, test fixtures, or test-only hooks inside functional `.rs` files in `crates/core/src/` or `crates/app/src/controllers/` etc.
- **Dedicated Test Locations**:
  - Core unit tests go in `crates/core/tests/unit_tests.rs`.
  - Core integration & benchmark tests go in `crates/core/tests/integration_tests.rs` and `crates/core/tests/benchmark_validation.rs`.
  - Binary crate tests go in `crates/app/src/app_tests.rs`, wired solely via `#[cfg(test)] mod app_tests;` at the top of `crates/app/src/main.rs`.
- Expose clean public APIs when tests need observability; never leave dead test helpers or `#[allow(dead_code)]` shims behind.

## 3. Code Quality, Design System & Zero Warnings
- **Design System Compliance**: All Slint UI additions and edits under `crates/app/ui/` must follow [`DESIGN.md`](DESIGN.md) — use `Palette` tokens from `crates/app/ui/widgets.slint` (supporting Dark, Light, Artwork-Adaptive, and Transparency modes), match existing layout geometry (`NavEntryRow`, `PlayerBar`, `RightPanel`, `card-gap`), and preserve keyboard shortcuts and accessibility states. Do not hardcode off-palette hex colors or ad-hoc spacing.
- Fix compiler and Slint LSP warnings at the source (e.g. remove unused helpers, migrate deprecated Slint properties such as `viewport-y` -> `content-y`) rather than suppressing them with attributes.
- Avoid duplicating UI components, Win32 helpers, or playback/queue mutation paths across callbacks and controllers.
- Keep heavy work (LibVLC plugin init, COM audio device enumeration, library indexing, metadata reads, network calls) off the UI first-paint path.

## 4. Commit, Changelog & Release Standards
- **Commit Message Format:** Every commit touching the Rust workspace must use a clear, descriptive, multi-part message:
  - **Subject line**: `<type>(<scope>): <imperative summary>` (e.g., `feat(video): add native child HWND video playback window and subtitle routing`). Keep under 72 characters.
  - **Body**: Explain **why** the change was made, **what** root causes or architectural behaviors changed, which files/components were affected, and any user-visible impact (defaults, migrations, shortcuts, UI layout). Use concise bullet points for multi-part changes.
  - **Verification**: State exact checks executed after the final edit (`cargo test --workspace`, `cargo check --all-targets`, `cargo build --release`, installer build).
- **Changelog Discipline (`CHANGELOG.md`):** Log user-facing feature changes and bug fixes in `CHANGELOG.md` under the release version — do **not** clutter `CHANGELOG.md` with internal refactors or every routine code change.
- **Consistent Versioning & Signed Releases:** Keep semantic versions synchronized across `crates/app/Cargo.toml`, `crates/core/Cargo.toml`, `CHANGELOG.md`, and git tags. All release commits and release tags must be cryptographically signed (`git commit -S` / `git tag -s` or GitHub verified merge signature).

## 5. Verification & Packaging Checklist
Before declaring work finished on changes to the Rust workspace:
1. Verify zero inline `#[cfg(test)]` blocks remain in functional files.
2. Run `cargo test --workspace`.
3. Build both Debug (`cargo build --workspace`) and Release (`cargo build --release --bin music-player-app`).
4. When packaging is requested, stage `packaging/windows/MusicPlayer.iss` with `MusicPlayer.exe`, icons, and bundled `libvlc` runtime before invoking Inno Setup (`ISCC.exe`).
