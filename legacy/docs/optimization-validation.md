# Optimization and release validation

Updated 2026-10-10. Candidate work is on `codex/update-verification-status`.
Playback changes in `9759726` and UI changes in `393ee8c` are preserved.
Optimization commits: `73981a8` (schema 9), `0c9c61c` (startup, watchers,
queue revisions and presentation), `00ccd77` (components/launcher),
`e24d59d` (self-signing/candidate packaging). At the initial optimization
checkpoint no stable tag or release existed. Passing builds are not installation
or interaction evidence; the final code's completed gates are recorded below.

## Verified locally

- Core suite: 76/76 passed with `dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj --configuration Release --no-restore`. Includes schema-9 migration backups, duplicate/unavailable queue paths, queue recovery, unchanged writes, 100,000-entry checkpoints, watcher batching/overflow throttling, library/queue context consistency, WAV alias metadata and DFF duration/indexing.
- Final native engine evidence at `19577d5` and `cf0c9a9`: 49/49 passed on both x64 and ARM64 with the fixture environment prepared as documented in development.md. Silent WAV/dummy output covers natural advancement, final-queue stop, seeking after end, paused/restored seek, repeat, successful and failed crossfade, native open errors, and queue revision behavior. Genuine fixtures additionally cover all 19 extensions, metadata preservation, video and subtitles. Local cached-engine checks are supplementary.
- Every `WebUI/scripts/*.js` passed `node --check`. Packaging PowerShell files passed the PowerShell parser. These are syntax checks, not GUI checks.
- x64 build succeeded with zero warnings/errors using an isolated `BaseOutputPath`; x64 and ARM64 self-contained publishes succeeded. The running user instance was not stopped or replaced.
- Preliminary x64 unpacked publish comparison: 321.94 MiB / 960 files before component selection, 271.36 MiB / 902 files after: 15.7% smaller. LibVLC modules/notices retained. Final signed archive/setup/MSIX measurements are pending.
- Managed trimming was evaluated only in `artifacts/trimming-evaluation`. Unresolved IL2026/IL2104 diagnostics in JSON/WinRT/TagLib prevent adoption. Production publishes remain untrimmed.
- Persistent self-signed RSA/SHA-256 `CN=Kalabhaftu` certificate generated; public certificate and fingerprint committed. Expected `MUSICPLAYER_SIGNING_PFX_BASE64` and `MUSICPLAYER_SIGNING_PFX_PASSWORD` secrets configured via stdin. Private material is protected outside this repository. No end-user trust was installed.
- Backup branch has 14 unique historical commits and predates substantial current features. Full-history bundle verified at `%LOCALAPPDATA%/MusicPlayerGitArchives/backup-pre-squash-music-player-v2-20261006.bundle`; SHA-256 `0569FA677F8F968E9239F48055F4738C38D70EA5C22D2CFCC239954EE3624D10`. Branch retained until release gates complete.

## Database benchmark

Command: `dotnet run --project tests/MusicPlayer.Benchmarks/MusicPlayer.Benchmarks.csproj --configuration Release -- artifacts/performance-20261009`.
The old checkpoint algorithm is reproduced exactly: full-session JSON serialization and upsert. Measurements are single local samples, not end-to-end GUI startup results.

| Tracks / queue entries | Old checkpoint WAL bytes | State-only WAL bytes | Old checkpoint ms | State-only ms | First 100 library rows ms |
|---:|---:|---:|---:|---:|---:|
| 100 | 16,512 | 4,152 | 2.19 | 1.93 | 25.99 |
| 100,000 | 11,573,112 | 4,152 | 636.94 | 2.40 | 56.78 |

FULL durability, WAL, playlists, tag backups and recovery copies remain enabled.
No routine startup VACUUM was introduced.

## Observed Windows measurements (not final acceptance)

Private run [37996130358](https://github.com/kalabhaftu/music-player/actions/runs/37996130358)
compared baseline `9759726` with `19577d5` on the same disposable x64 runner.
For 100 actual saved tracks, time to visible library rows fell from 11,242 ms to
4,596 ms. Combined native/WebView private memory after ten idle seconds was
205,488,128 bytes before and 202,461,184 bytes after; working set was 493,707,264
and 484,855,808 bytes. Ten-second idle CPU consumption fell from 0.78125 to
0.375 CPU-seconds. These are single measurements, not a universal speed claim.
The 100,000-track baseline scan did not cancel within 60 seconds, preventing
that run from reaching final package acceptance; this limitation is now recorded
explicitly and the full comparison is rerunning.

The complete x64 UI run at `19577d5` recorded combined private memory of
239,603,712, 243,122,176 and 243,879,936 bytes after successive warmed navigation
batches. The final 20 navigations grew by 757,760 bytes. Current UI checks enforce
a 32 MiB limit over the final warmed batch and retain the raw samples.

Private run [38000118310](https://github.com/kalabhaftu/music-player/actions/runs/38000118310)
completed the x64 performance gate at `9c2f75c`. Unpacked size fell from
336,530,333 to 289,367,981 bytes (14.0%); signed portable ZIP size fell from
142,351,909 to 123,622,304 bytes (13.2%). Both exceed the 10% target.
Visible library rows took 15,502 ms before / 4,744 ms after with 100 tracks,
and 12,740 ms before / 7,961 ms after with 100,000 tracks. At the ten-second
resource sample, combined private memory was 211,988,480 / 199,462,912 bytes
for 100 tracks and 471,498,752 / 320,045,056 bytes for 100,000 tracks. The larger
baseline was still scanning after cancellation, so its 7.234375 CPU-seconds
must not be called idle CPU. The candidate scan had settled and consumed
0.296875 CPU-seconds; the small settled samples consumed 0.53125 / 0.046875.
Private run [38005548773](https://github.com/kalabhaftu/music-player/actions/runs/38005548773)
at `fc99848` passed the complete performance gate, including the added ARM64
unpacked and ZIP comparisons against `9759726`; each reduction exceeded 10%.
Exact values are retained in that run's performance-evidence artifact. Final
main validation remains required. Numerical evidence is also printed in future
workflow logs so it can be reviewed without downloading package artifacts.

## Remaining stable-release gates

Private candidate [38048035180](https://github.com/kalabhaftu/music-player/actions/runs/38048035180)
at `eeb4711` passed the resource gate on the same runner used for each baseline
comparison. Unpacked/ZIP reductions were **14.0%/13.2% on x64** and
**13.4%/12.3% on ARM64**. Both full portable UI gates and both complete MSIX
installation gates also passed; setup upgrade still requires a successful rerun.

| Saved tracks | Visible library ms, before / after | Combined private bytes, before / after | Combined working-set bytes, before / after | Ten-second CPU seconds, before / after |
|---:|---:|---:|---:|---:|
| 100 | 14,272 / 4,556 | 211,906,560 / 203,313,152 | 499,720,192 / 483,016,704 | 0.59375 / 0.5 |
| 100,000 | 6,965 / 6,971 | 473,833,472 / 305,250,304 | 738,242,560 / 555,778,048 | 11.390625 (scanning) / 0.640625 (idle) |

The large-library startup times in this single sample were effectively equal;
do not describe every startup as faster. The baseline large scan remained active
and its CPU sample is not idle. Both candidate scans settled before sampling.
Raw numerical JSON is retained and printed in the workflow job log. Final merged
revision validation remains required.

## Final code candidate, b1316ef

[CI 38053176126](https://github.com/kalabhaftu/music-player/actions/runs/38053176126)
passed all protected Linux/x64/ARM64 checks. Core: 101/101; Node: 44/44.
Both architectures passed 53 native cases and skipped only three physical
audio-device cases on runners without an output endpoint. All portable UI
checks passed. [Private candidate 38053565166](https://github.com/kalabhaftu/music-player/actions/runs/38053565166)
passed signed build/authentication, complete setup and MSIX UI/install/upgrade/
association/uninstall gates on both architectures, and the resource comparison.

| Package | Baseline bytes | Candidate bytes | Reduction |
|---|---:|---:|---:|
| x64 unpacked | 336,531,048 | 289,386,104 | 14.01% |
| x64 signed portable ZIP | 142,352,103 | 123,630,720 | 13.15% |
| ARM64 unpacked | 329,230,402 | 285,132,721 | 13.39% |
| ARM64 signed portable ZIP | 131,868,364 | 115,715,044 | 12.25% |

| Saved tracks | Visible library ms, before / after | Combined private bytes, before / after | Combined working-set bytes, before / after | Ten-second CPU seconds, before / after |
|---:|---:|---:|---:|---:|
| 100 | 11,067 / 4,846 | 211,439,616 / 201,678,848 | 497,242,112 / 480,358,400 | 0.796875 / 0.421875 (both idle) |
| 100,000 | 6,651 / 7,311 | 556,240,896 / 300,675,072 | 846,041,088 / 556,212,224 | 12.65625 (scanning) / 0.703125 (idle) |

These are single samples, with startup variance across runs. Large-library
visibility was slower in this final sample; reduced startup contention is
supported by settled candidate scans and resource measurements, not a claim
that every startup became faster. Raw JSON remains in the workflow log and
performance-evidence artifact. Both architecture package reductions exceed 10%.
The merged revision's results are recorded below.

- [x] Required `Core tests · Linux`, `x64`, and `ARM64` on the tested code candidate.
- [x] Signed packaging, timestamps, assemblies/scripts, embedded uninstaller, MSIX and authenticated manifest.
- [x] Complete portable/installed library/UI/restart flows on x64/ARM64.
- [x] Setup/MSIX install, upgrade, associations and both data-retention choices.
- [x] Representative playback results for every listed audio format.
- [x] Small/100,000-track resource comparison; navigation and minimized-playback checks.
- [x] Protected PR merge; synchronize main; signing preflight and final package validation from merged revision.
- [x] Delete the archived local backup branch after gates passed and create tag `v1.0.0`.
- [x] Publish self-signed assets and verify uploaded hashes.

Signing is explicitly self-signed, **not publicly trusted**. See
[`SIGNING.md`](../packaging/windows/SIGNING.md). Publisher/SmartScreen warnings and
the MSIX certificate-trust choice must remain explicit in release notes.

## Merged release revision, 3328407

[Main CI 38059601873](https://github.com/kalabhaftu/music-player/actions/runs/38059601873),
[signing preflight 38060250632](https://github.com/kalabhaftu/music-player/actions/runs/38060250632),
and [full candidate 38060262194](https://github.com/kalabhaftu/music-player/actions/runs/38060262194)
all passed at `3328407d5df3f68674f02c116a88372e336c4e9e`, after the final Artwork
default was merged. Both installed architecture jobs passed complete setup/MSIX
interaction and lifecycle gates. The comparative performance job passed both
architecture package-size reductions of at least 10%; its numerical JSON and
startup/CPU/combined-memory samples remain in the job log and performance-evidence
artifact. Passing the gate does not imply that every startup sample is faster.

Tag `v1.0.0` points to this GitHub-verified source commit.
[38062507918](https://github.com/kalabhaftu/music-player/actions/runs/38062507918)
passed the repeated signed build, both complete installed package jobs and the
resource job. The publish job authenticated the manifest and compared every
uploaded draft asset with its validated candidate before publishing
[Music Player 1.0.0](https://github.com/kalabhaftu/music-player/releases/tag/v1.0.0)
at 2026-10-10 15:31:27 UTC. The public release has all 12 expected assets and is
neither a draft nor a prerelease. Final downloadable ZIP sizes are 123,630,739
bytes (x64) and 115,715,056 bytes (ARM64); the repeated comparison exceeded the
10% reduction target for both ZIP and unpacked sizes on both architectures.
No release binaries or uncached dependencies are downloaded to the user's PC.
