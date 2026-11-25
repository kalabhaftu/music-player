# Music Player (Rust Workspace) — Agent & Developer Rules

These rules apply to all work inside `music-player-rust/`.

## 1. Architecture & Parity Principles
- **Headless Core (`crates/core`)**: Keep all database SQLite WAL queries, tag reading/writing, audio/video LibVLC orchestration, equalizer/crossfade state, lyrics parsing/fetching, and scanner pipelines strictly inside `crates/core` with zero UI toolkit dependencies.
- **Desktop Host (`crates/app`)**: `crates/app` owns the Slint UI, controller state orchestration, native Win32 shell hooks (SMTC, tray, taskbar buttons, child video HWND), and background task coordination.
- **Feature Parity**: Match behavior from the C# / WebUI baseline (`../src/`) — including defaults (e.g., video extensions default to off during scans until enabled), crossfade rules, equalizer behavior, lyrics skip on video, keyboard shortcuts, and error reporting. Never silently strip or stub out required logic to make a build pass.

## 2. Test Organization (Strict Separation)
- **No Inline Test Code in Production Modules**: Do NOT place `#[cfg(test)] mod tests`, test fixtures, or test-only hooks inside functional `.rs` files in `crates/core/src/` or `crates/app/src/controllers/` etc.
- **Dedicated Test Locations**:
  - Core unit tests go in `crates/core/tests/unit_tests.rs`.
  - Core integration & benchmark tests go in `crates/core/tests/integration_tests.rs` and `crates/core/tests/benchmark_validation.rs`.
  - Binary crate tests go in `crates/app/src/app_tests.rs`, wired solely via `#[cfg(test)] mod app_tests;` at the top of `crates/app/src/main.rs`.
- Expose clean public APIs when tests need observability; never leave dead test helpers or `#[allow(dead_code)]` shims behind.

## 3. Code Quality & Zero Warnings
- Fix compiler and Slint LSP warnings at the source (e.g. remove unused helpers, migrate deprecated Slint properties such as `viewport-y` -> `content-y`) rather than suppressing them with attributes.
- Avoid duplicating UI components, Win32 helpers, or playback/queue mutation paths across callbacks and controllers.
- Keep heavy work (LibVLC plugin init, COM audio device enumeration, library indexing, metadata reads, network calls) off the UI first-paint path.

## 4. Commit Message Standards
Every commit touching `music-player-rust/` must use a clear, descriptive, multi-part message:
- **Subject line**: `<type>(<scope>): <imperative summary>` (e.g., `feat(video): add native child HWND video playback window and subtitle routing`). Keep under 72 characters.
- **Body**: Explain **why** the change was made, **what** root causes or architectural behaviors changed, which files/components were affected, and any user-visible impact (defaults, migrations, shortcuts, UI layout). Use concise bullet points for multi-part changes.
- **Verification**: State exact checks executed after the final edit (`cargo test --workspace`, `cargo check --all-targets`, `cargo build --release`, installer build).

## 5. Verification & Packaging Checklist
Before declaring work finished on changes to `music-player-rust/`:
1. Verify zero inline `#[cfg(test)]` blocks remain in functional files.
2. Run `cargo test --manifest-path music-player-rust/Cargo.toml --workspace`.
3. Build both Debug (`cargo build --manifest-path music-player-rust/Cargo.toml --workspace`) and Release (`cargo build --release --manifest-path music-player-rust/Cargo.toml --bin music-player-app`).
4. When packaging is requested, stage `music-player-rust/packaging/windows/MusicPlayer.iss` with `MusicPlayer.exe`, icons, and bundled `libvlc` runtime before invoking Inno Setup (`ISCC.exe`).
