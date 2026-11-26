## Summary

<!-- What user-visible behavior or maintenance gap does this change address? -->

## Verification

- [ ] Workspace tests pass (`cargo test --workspace`) and `cargo check --all-targets` passes with zero warnings.
- [ ] Release build succeeds (`cargo build --release --bin music-player-app`) and UI/Windows shell behavior was verified when applicable.
- [ ] `DESIGN.md` and `AGENTS.md` conventions (no inline `#[cfg(test)]` in functional modules, Slint `Palette` token compliance) are followed.
- [ ] `CHANGELOG.md` is updated for a user-visible change.
- [ ] New runtime files, crate dependencies, and license obligations are described.
- [ ] Logs, screenshots, and examples do not expose private paths, media, or credentials.

## Review notes

<!-- List known limitations, manual steps, or follow-up work. -->
