# Music Player changelog

Changes to Music Player are documented here. The app name, executable, package identity, and local data root use Music Player naming.

## [Unreleased]

- **Inverted Lyrics Auto-Fetch Fallback:** When online lyrics auto-fetching and queue prefetch find no match for a track, automatically retry with swapped artist/title fields (checking existing `?q=` results first and sending at most one targeted fallback query) so songs with inverted tags still resolve lyrics.
- **Unavailable Queue Cleanup:** Clean stale or missing tracks from the active queue whenever playing a queue entry reports it unavailable instead of leaving the unplayable item stuck in the queue list.
- **Immersive Lyrics Continuity & Player Bar Toggles:** Opening Immersive Player while Lyrics are open in the main view now keeps Lyrics visible in Immersive mode, and clicking Lyrics, Queue, or Equalizer in the player bar while already on that view toggles it closed.
- **Startup Volume Synchronization:** Sync the persisted volume setting to `AudioEngine` at startup, upon deferred LibVLC attachment, and on resume so playback volume always matches the volume slider before the user adjusts it.
- **Artwork Isolation & Repair:** Prevent tracks without embedded cover art from inheriting or persisting artwork from `"Unknown Album"` or neighboring files, add a one-time background verification to heal previously polluted artwork references, and fix async thumbnail decoding cancellations/throttles that could leave valid album covers blank.
- **Branded Now Playing Fallback:** Replace placeholder initials in the Now Playing panel, Now Playing hero, Bottom Player Bar, and Immersive View with a monochrome Music Player headphones vector icon and fall back to the active default theme palette when no cover art is present (keeping letter badges in track/album lists).
- **Library Root Track Counts:** Count all tracks recursively across subdirectories in the **Library folders** manager instead of counting only loose files in the top directory.
- **Desktop Acrylic Transparency & Title Bar:** Replace stacked opaque card layers and hardcoded solid surfaces with translucent glass overlays when **Transparent window** is enabled, remove the mismatched Win11 caption fill (`DWMWA_COLOR_NONE`) in transparent mode, default newly enabled transparency to `35%`, and raise the maximum transparency slider cap to `85%`.

## [2.0.0]

Major native rewrite in **Rust + Slint UI + LibVLC + SQLite**, replacing the legacy WinUI 3 / WebView2 host (archived under `legacy/`).

- **Native Slint UI & Win32 Integration:** Fast cold startup with parallel LibVLC pre-warming, deferred COM/MMDevice initialization, native SMTC media controls, tray icon, taskbar thumbnail buttons, COM drag-and-drop, and window placement persistence.
- **Headless Rust Core (`crates/core`):** SQLite WAL persistence, incremental mtime + size scanner with live file watching, exact duplicate fingerprinting, Lofty metadata reading, and container-specific tag editing with byte-for-byte backup/restore.
- **Audio, EQ & Crossfade:** 10-band ISO equalizer with 20 presets and preamp, quadratic perceptual volume curve, Windows output routing, and non-blocking dual-player audio crossfading.
- **Dedicated Video Window:** Native Win32 child HWND video stage (`VideoWindow`) with embedded and nearby sidecar subtitle switching (`.srt`, `.ass`, `.ssa`, `.vtt`), playback speed (`0.5x`–`2x`), PNG frame snapshots, and `F11` fullscreen.
- **Lyrics & Themes:** Timed `.lrc` sidecar and embedded lyrics priority, on-demand LRCLIB search, Dark / Light / Artwork-Adaptive palettes, custom selection colors, and window transparency.
- **Repository & Packaging:** Promoted Rust workspace to repository root, archived WebUI (`legacy/src/MusicPlayer.App/`) and experimental Avalonia (`legacy/src/MusicPlayer.Desktop/`) under `legacy/`, and updated Windows Inno Setup packaging (`packaging/windows/`).

## [1.0.4]

- Preload lyrics automatically for upcoming queued tracks to ensure smooth lyric transitions without loading pauses.
- Restore smart playlist cards (Most Played, With Lyrics, Recently Played, Videos) with distinctive hero icons and rich gradients.
- Replace the lyrics "Jump to current" banner with a compact circular button featuring native hover tooltips.
- Add manual search capabilities with prefilled track title and artist in the "Find lyrics" dialog and eliminate duplicate modal scrollbars.
- Retain the native Windows titlebar in windowed mode for smooth moving, dragging, and snapping while preserving full-screen immersive playback.
- Prevent album artwork thumbnail collapse when resizing to compact window heights in immersive view.
- Fix artist page album playback to cleanly queue artist tracks.

## [1.0.3]

- Eliminate recursive startup disk scanning when the library is already indexed, allowing instant app startup from SQLite.
- Defer SHA-256 duplicate fingerprinting to on-demand duplicate view navigation instead of thrashing the disk on startup.
- Prevent redundant UI reload notifications during initial track availability checks.

## [1.0.2]

- Show startup feedback immediately, initialize audio and the library in parallel, and reuse unchanged track metadata when opening files.
- Keep favorites, settings, and background indexing from replacing the active library page; retain the saved library during launch checks and batch folder changes more conservatively.
- Keep queue and equalizer controls inside fullscreen, use taller vertical equalizer sliders and shared themed dropdowns, soften corners, and stabilize wrapped lyric transitions.
- Reapply volume after audio output opens, prepare crossfades earlier, and fall back to the system output when a selected device disconnects.
- Receive complete Explorer selections for queue and playlist actions, insert additions next, align queue displays, restore smart-page sort defaults, and find lyrics using cleaned upload titles.
- Index case-insensitive scan lookups, batch excluded-folder writes, throttle scan progress, and stop duplicate hashing during playback. Watcher scans wait for playback to stop and run at most once every five minutes.
- Preserve loaded pages through fullscreen drawers and window restore, protect dropdowns during settings search, ignore stale playback responses, and recover disconnected audio routes without keeping an outdated queue index.
- Keep Music Player running when its window closes if notification-area mode is enabled; add tray actions for Open, Previous, Play/Pause, Next, and Quit.
- Replace the extra startup window with transparent app artwork and an Opening label, and avoid walking the entire database before opening the app.
- Make manual Next advance immediately while automatic crossfades remain scheduled at the track ending.
- Apply complete equalizer curves smoothly on a worker, debounce transactional persistence, and preserve preset preamp levels when editing or saving Custom curves.
- Fit transport controls to snapped and narrow windows, reveal compact volume on hover or focus, collapse navigation as needed, and constrain context menus to the viewport.
- Hide the full Now Playing panel when it cannot fit; open it from the toolbar by widening the window and restore the prior width on close. Preserve normal show/hide behavior when it fits, prevent resize handles from creating unnecessary table scrollbars, and use a bolder Opening label.
- Stamp the Setup executable with the same 1.0.2.0 file and product version as the app.
- Honor exact-duplicate hiding in album, artist, genre, and folder browsing and playback sources, with matching group counts. Explicit queue and playlist occurrences are retained.

## [1.0.1]

- Restore Music Player's Explorer context-menu labels and icons without replacing other applications' registrations; add the app icon to associated file types while preserving Windows album-art thumbnails.
- Add a Settings shortcut to Windows Default Apps when Music Player is not the effective handler for registered audio formats.
- Preserve playback, the current queue occurrence, duplicates, and position when files are added to the queue through the app or Explorer; give the Windows taskbar a useful idle and playing title.
- Remove confirmed-deleted tracks from library, playlists, queue, and saved playback state, while hiding tracks on disconnected drives and retaining those entries for reconnection.
- Cache up to six library pages for immediate back-and-forth navigation, avoid hashing files in browsing requests, and return local version/About information without waiting for GitHub.
- Compare releases from one local version source and report the latest release only after a successful online comparison.

## [1.0.0]

- Correct Setup's WebView2 prerequisite detection for machine-wide and per-user installations. Reuse an installed runtime without a network request, explain when installation is required, and provide a manual/offline recovery path.
- Use album artwork as the default accent for new profiles and appearance resets, preserving existing saved theme choices.
- Fix Load more across library views, preserve complete album/artist/genre contents, and retain duplicate queue identities while paging.
- Use independent music output buffers for crossfade, wait for incoming playback, and restore correctly on failure, pause or seek.
- Prefer confident timed lyrics for display while preserving local lyrics and offline fallback; improve artwork contrast in both themes.
- Fix Ogg/OGA seeking, WAV alias metadata and DFF startup/duration handling; preserve original media bytes and tag recovery copies.
- Preserve user-owned files during setup upgrades and honor the explicit saved-data removal choice during uninstall.
- Allow enabled video extensions through file activation and name native video controls for accessibility.
- Reduce self-contained Windows packages by selecting the required WinUI components; retain LibVLC's audio/video modules and dependency notices.
- Restore the saved library before scanning and checking updates; batch filesystem changes into bounded directory scans.
- Store ordered queue entries separately from playback state, preserving duplicates and missing paths through migration and recovery. Position checkpoints no longer rewrite the queue.
- Fix natural track advancement, seeking after the queue ends, crossfade event handling, immersive lyrics, and search-context queue selection.
- Reduce paused and minimized UI work using WebView2 suspension and state resynchronization.
- Distribute self-signed packages and a public certificate with explicit Windows trust instructions. This signing is not publicly trusted and Windows may display publisher or SmartScreen warnings.

- Checkpoint recovered SQLite data before replacing a damaged library database, so its WAL is not left behind under the temporary filename.
- Preserve completed track reads when a library scan is cancelled before its normal batch size.
- Log when the Songs page is empty because the active index contains no tracks, without recording search text or media paths.
- Repair missing cached album artwork when a library scan encounters an existing track, and keep album navigation from restoring a detail route without an album.
- Reconcile indexed file availability at startup and after scans. Remove confirmed-missing tracks even when their old parent folder was deleted, while retaining entries on offline volumes and under roots or paths the scan could not inspect.
- Ignore out-of-order library/search responses, and reject lyric edits or searches that target a track that is no longer indexed.
- Correct light-theme contrast for settings, controls, and side panels.
- Package the Microsoft-signed WebView2 Evergreen bootstrapper inside Setup and install the shared runtime only when missing; fail release uploads rather than replacing assets for an already-published version.
- Replace the previous WinUI screen layout with Music Player's locally packaged WebView2 interface while keeping playback and library behavior in C#.
- Add durable collapsed-sidebar and resizable-panel preferences, resettable UI groups, built-in history/most-played/with-lyrics playlists, and remove those smart lists from primary navigation.
- Detect removed audio files in the background, reconcile index rows without user clicks, retain playlist and queue references, and expose unavailable library roots and scan skips.
- Index sidecar and embedded lyric availability for a paged With Lyrics view; identify lyric source, read supported embedded tags including ID3 synchronized lyrics, and preserve richer LRC timing metadata.
- Use GitHub ETags, single-flight checks, success-based retry timing, and a repository-validated stable-release fallback while keeping updates user-initiated and installation manual.
- Align the app and executable name, package identity, file associations, logs, and local data root with Music Player.
- Show a clear unavailable-file notice and reconcile stale library rows when their configured storage is reachable, while preserving queue and playlist occurrences for recovery.
- Keep the saved track duration visible while restored playback is paused and the audio engine has not loaded its timeline yet.
- Fix playlist selection, playback, duplicate-entry removal, rename, export and delete flows; add library-root management and live scan details.
- Expose active Shuffle and A–B repeat states and keep the desktop player usable at narrower window widths.
- Migrate the local index safely to FTS5 trigram search and database-backed scan generations; page library and playlist results and preserve rows under incomplete, unavailable and excluded paths.
- Bound artwork memory and cache storage, retain five usable tag backups per track, serialize playback state changes, and bound log/network/settings work.
- Pin GitHub Actions to commit SHAs, restrict workflow permissions, run ARM64 checks on native Windows ARM, protect `main` with required pull-request checks, and require verified signing plus a signed SHA-256 asset manifest before publishing preview or stable releases.

## [0.1.0-preview.1]

- Avoid updating closed-window controls after a library scan is cancelled during shutdown.
- Select connected Windows audio outputs, including Bluetooth headphones and speakers, and retain the choice across restarts.
- Choose Mica, Desktop Acrylic, or an opaque backdrop, plus subtle, expressive, or disabled page transitions that honor Windows accessibility settings.
- Serialize concurrent tag, lyric-sidecar and playlist writes per file, with unique staged files and backups.
- Compare stable and preview release tags correctly so preview users can see newer previews.
- Let users resize the navigation and browse panes.
- Restore A–B repeat marks with the paused playback session.
- Decode version-4 notification-area callbacks so tray activation restores the app.
- Keep the queue on the playing track when a crossfade is interrupted.
- Preserve the selected duplicate playlist entry through playback and session restore.
- Export local diagnostic logs as a shareable ZIP from Settings.
- Allow clearing tag year and track number, and reject invalid numeric values before saving.
- Include local OS, process architecture, and .NET version in startup logs for easier bug diagnosis.
- Stream directory scans for responsive cancellation and skip access-denied paths without aborting the library scan.
- Register crash logging before app resource initialization to capture more startup failures.
- Keep A–B repeat stable while paused and prevent automatic crossfades from interrupting the marked passage.
- Save completed tracks from the current database batch when a library scan is cancelled.
- Add the initial native Windows music player, local library, playback, playlist, metadata and lyrics features.
- Add Windows CI and architecture-specific release packaging workflows.
- Log crashes, playback failures, and handled app errors locally; open the log folder from Settings.
- Remove tracks deleted from directories that were scanned successfully while retaining entries under offline or unreadable folders.
- Add dedicated album, artist, genre and folder browsing, plus queue reorder, removal and clear-upcoming controls.
- Fix scan exclusions with trailing separators and restore playback volume when pausing or cancelling a crossfade.
- Expose file path, last played and rating sorts in the library.
- Keep duplicate playlist entries distinct while navigating the playback queue.
- Identify failed tracks in the local log and recover the active playback state when LibVLC reports an error.
- Throttle failed automatic update checks to the configured weekly interval.
- Clear stale synced lyrics when playback moves before the next timed line.
- Refresh artwork cache keys after tag edits and update the current track details immediately.
- Publish self-contained x64 and ARM64 portable ZIPs from successful Windows CI runs.
- Reset artwork accents when switching back to native styling and recover from damaged EQ settings.
- Edit and inspect Xiph, ID3v2, ASF and APEv2 custom text tags; preserve each format's own field names.
- Report embedded lyrics as saved when only the library refresh fails.
- Exclude system folders by their drive-root location while keeping user folders with the same name.
- Keep LRCLIB search and queue, equalizer and update feedback inside their open dialogs.
- Roll back unsaved navigation changes when Settings is cancelled or contains an invalid folder path.
- Show app feedback in dismissible banners so playback errors cannot collide with open dialogs.
- Remove the selected duplicate playlist entry instead of always removing its first occurrence.
- Honor the weekly update-check interval when saving Settings while keeping manual checks immediate.
- Display plain lyrics in Now Playing when timed LRC lines are unavailable.
- Prevent an early sort-selection event from crashing the window during XAML initialization.
- Expose additional standard metadata fields and show them in track details.
- Preserve the current track while shuffling the upcoming queue, and honor repeat modes during timed crossfades.
- Skip and log a corrupt saved playback session so it cannot block app startup.
