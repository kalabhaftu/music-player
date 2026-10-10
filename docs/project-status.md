# Project status

Updated: 2026-10-11. Music Player 1.0.0's installer correction is being prepared
for republication at the user's explicit request under the same `v1.0.0` tag.
Setup now uses Microsoft's documented `pv` registry detection before invoking
the WebView2 bootstrapper, explains missing prerequisites, and supports manual
offline runtime installation. The GitHub setup-upgrade test requires the
existing-runtime skip path. No local build or installation tests are being run;
the replacement must pass the complete signed GitHub release pipeline.

The results below record the original release and do not validate the corrected
installer. The user's
running instance remains untouched. Windows UI/package tests use disposable
GitHub runners. No release binaries are downloaded to the user's PC.

## Original Music Player 1.0.0 release revision

The original release tag `v1.0.0` pointed to `3328407d5df3f68674f02c116a88372e336c4e9e`.
PRs [#2](https://github.com/kalabhaftu/music-player/pull/2) and
[#9](https://github.com/kalabhaftu/music-player/pull/9) were protected-squash-merged.
The release commit was created on GitHub.com by `web-flow`; GitHub reports
`verification.verified: true` and `reason: valid`. Local main is synchronized.

- [Main CI 38059601873](https://github.com/kalabhaftu/music-player/actions/runs/38059601873) passed all required Linux/x64/ARM64 checks, including the final Artwork default.
- [Main signing preflight 38060250632](https://github.com/kalabhaftu/music-player/actions/runs/38060250632) passed certificate identity, key, validity, purpose, timestamping, source signature and detached manifest checks.
- [Main candidate 38060262194](https://github.com/kalabhaftu/music-player/actions/runs/38060262194) passed signed packaging, complete x64/ARM64 setup/MSIX installation, upgrade, interaction, associations and both uninstall choices, plus the small/100,000-track resource comparisons and both package-size targets. Publication was intentionally skipped in private candidate mode.
- [Tag release workflow 38062507918](https://github.com/kalabhaftu/music-player/actions/runs/38062507918) passed signed build, complete x64/ARM64 package tests and resource comparisons. Its publication step downloaded every draft asset on the runner, compared bytes with the validated candidate and authenticated the manifest before publishing.
- [Music Player 1.0.0](https://github.com/kalabhaftu/music-player/releases/tag/v1.0.0) was published at **2026-10-10 15:31:27 UTC**, with `isDraft: false`, `isPrerelease: false` and all **12 expected assets**. The public certificate asset's SHA-256 digest matches the pinned certificate. Release notes link the GitHub-verified source commit and state the self-signed trust limitations.

Final publication commands:

```powershell
gh workflow run release.yml --ref main -f mode=preflight
gh workflow run release.yml --ref main -f mode=candidate
git tag -a v1.0.0 3328407d5df3f68674f02c116a88372e336c4e9e -m 'Music Player 1.0.0'
git push origin refs/tags/v1.0.0
gh release view v1.0.0 --repo kalabhaftu/music-player --json tagName,isDraft,isPrerelease,publishedAt,url,assets
```

The release includes both portable ZIPs, both setup installers, the combined
MSIX bundle, public certificate, signing instructions, dependency notices,
verification and MSIX uninstall helpers, SHA-256 manifest and detached signature.
The original release gates passed; the installer correction requires a new
signed release workflow before replacement assets are published. Physical endpoint tests
skipped on hosted runners and codec/hardware coverage limits remain documented;
they are not represented as universally tested behavior.

The retained `codex/update-verification-status` branch preserves baseline
`9759726` for reproducible comparisons after squash merges. The obsolete branches
and unused worktree have been removed; their complete verified bundles remain
outside the repository. No additional release dependency is required from the
user. Exact offline local builds would need uncached LibVLC 3.0.24; the existing
local player uses cached 3.0.23.1. No dependency download was started locally.

## Reviewed implementation

Playback ownership/generation guards fix end/error callbacks and seeking after
the queue ends. Music crossfades use independent DirectSound buffers and wait for
incoming decoder readiness; failure, pause and seek restore the outgoing track.
Queue revisions avoid whole-library reads and repeated queue signatures.

Schema 9 stores ordered queue entries separately from playback state, preserving
duplicate and unavailable paths. Position checkpoints no longer rewrite queues.
Migration backups, corruption recovery, FULL durability, playlists and tag
backups remain intact. Unchanged settings and paused checkpoints skip writes;
passive WAL maintenance does not introduce startup VACUUM.

Saved rows render before background scans and update checks. Watchers batch
affected directories, bound pending work and throttle overflow recovery and work
after explicit cancellation for five minutes; manual scans remain immediate.
Official WebView suspension reduces minimized rendering work while native
playback continues. Component selection removes unused Windows App SDK payloads
while retaining self-contained runtimes and VLC modules/notices. The launcher
builds the exact x64 output, retries restore once and explains audit-feed warnings
without disabling auditing or restarting a running player.

Completed shared edits improve artwork contrast, timed lyric lookup/offline
fallback, crossfade/output routing and pagination. Grouped collection contents
stay complete; global duplicate hiding retains its Songs context. Paging retains
queue entry identities, retries failed loads and rejects stale responses.

Signing uses one persistent **self-signed** `CN=Kalabhaftu` key. First-party
binaries, PowerShell helpers, setup/embedded uninstaller and MSIX are signed;
vendor signatures remain intact. Authenticode signatures are timestamped. CMS
authenticates the checksum manifest. This is not publicly trusted signing:
publisher/SmartScreen warnings and intentional MSIX certificate trust are
documented in [SIGNING.md](../packaging/windows/SIGNING.md).

## Verified candidate before the final theme addition

Tested code: `b1316ef0fa7d8063e32d4f81806cd23ffd2d5bc5`.

- All protected checks passed in [CI 38053176126](https://github.com/kalabhaftu/music-player/actions/runs/38053176126): **Core tests · Linux**, **x64**, **ARM64**.
  There are 101 core cases and 44 Node cases. An independent local rerun passed
  101/101 core and 44/44 Node cases with no failures or skips.
- x64 and ARM64 native tests each passed **53 cases**, with **3 physical audio-device cases
  skipped because the runner has no output device** (56 total). Genuine fixtures
  exercise all 19 audio extensions, metadata/byte-for-byte recovery, read-only
  rejection, video/subtitles, seeking, repeat, crossfade and restart after end.
  The release engine is LibVLC 3.0.24. Local physical-device checks with cached
  3.0.23.1 are supplementary; see [music-crossfade.md](music-crossfade.md).
- Both portable UI gates passed initial and restart checks: automatic discovery
  and persistence, search, duplicate queue dragging, immersive lyrics, settings,
  light/dark artwork contrast, 60 navigations, minimized advancement,
  tray/taskbar/media keys, video/subtitles, real fullscreen/Escape and PNG
  snapshots. Final warmed navigation growth must remain below 32 MiB across
  20 navigations; JavaScript errors fail the gate.
- All private candidate gates passed in [38053565166](https://github.com/kalabhaftu/music-player/actions/runs/38053565166):
  signed portable ZIPs/setup/MSIX, timestamps and authenticated checksums;
  complete installed UI/restart checks on x64 and ARM64; setup/MSIX upgrades
  from 0.9.9, associations, both uninstall data choices, source music and
  user-owned installation-file preservation; and comparative resource gates.
  Publication was intentionally skipped because this was a private candidate.

Commands:

```powershell
dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj --configuration Release --no-restore
node --test tests/MusicPlayer.Ui.Tests/lyrics-theme.test.mjs tests/MusicPlayer.Ui.Tests/library-pagination.test.mjs
# CI prepares genuine media fixtures before the native suite.
dotnet test tests/MusicPlayer.Playback.Tests/MusicPlayer.Playback.Tests.csproj --configuration Release
gh workflow run release.yml --ref codex/update-verification-status -f mode=candidate
```

Earlier complete x64 setup/MSIX checks passed at `c1c71d8` in [38050852173](https://github.com/kalabhaftu/music-player/actions/runs/38050852173).
ARM64 lifecycle and MSIX checks passed there; initial setup UI focus failed.
`7827866` uses official foreground activation and waits for the target window
to process it before requiring real input. The previous setup file lock came
from the fixture host loading installed TagLib; `a0fb3fb` isolates this helper
in a child process that exits before upgrade. Independent lifecycle checks
continue after interaction failures, but the failed interaction still fails the job.

## Resource and history evidence

The completed `eeb4711` comparison, [38048035180](https://github.com/kalabhaftu/music-player/actions/runs/38048035180),
reduced unpacked/ZIP size by **14.0%/13.2% on x64** and **13.4%/12.3% on ARM64**.
The 100-track library became visible in 4,556 ms versus 14,272 ms. Large-library
times were effectively equal, 6,971 versus 6,965 ms; combined private memory fell
from 473,833,472 to 305,250,304 bytes. Large baseline CPU was still scanning and is
not called idle. Position checkpoints write **4,152 WAL bytes** with either 100
or 100,000 queue entries, versus 11,573,112 bytes for the old large checkpoint.
Exact measurements/limitations: [optimization-validation.md](optimization-validation.md).

The final `b1316ef` comparison also passed both architecture size targets:
x64 unpacked/ZIP **14.01%/13.15%** smaller, ARM64 **13.39%/12.25%** smaller.
Small-library visibility was 4,846 versus 11,067 ms; large-library visibility
was 7,311 versus 6,651 ms in this single sample, so startup is not claimed to
improve uniformly. Large combined private memory fell from 556,240,896 to
300,675,072 bytes. Both candidate scans settled; the large baseline remained
scanning during its resource sample. Raw JSON is printed in the job log.

The obsolete remote branches `archive/music-player-v2-checkpoint-20261006` and
`codex/complete-bracken-vale`, and the clean unused `marbled-wolf` worktree,
were removed at the user's request. Their history and pre-squash main/current
branches remain in a verified complete bundle outside the checkout:
`%LOCALAPPDATA%/MusicPlayerGitArchives/music-player-pre-release-squash-20261010-b1316ef.bundle`.
SHA-256: `CCF87C68D873EC8EAFFC876FB2234CF21E7EDF16B9EF0E04B3251E18D41DB7AB`.
The separate backup bundle is also verified; its fingerprint is in
[optimization-validation.md](optimization-validation.md). The local backup branch
was removed after the complete candidate gates passed.

## Final theme addition

PR #2 was protected-squash-merged as `d12de43bc4c4c8df6223f611d01ab6b2575b87a5`.
Its tree exactly matches the verified documentation candidate `5b5e830`.
The completed shared commit `98c5460` subsequently makes Artwork the accent
default only when a profile has no saved accent mode, and for explicit appearance
resets. Existing Native/manual choices remain. PR #9 passed all protected checks
and was squash-merged as `3328407`; main CI, signing preflight and complete
merged-revision package/resource validation then passed.

## Release gates

- [x] Final code's required CI, core/native formats and complete portable UI.
- [x] Complete signed setup/MSIX UI, upgrades, associations and both uninstall choices on x64/ARM64.
- [x] Complete final before/after resource and package-size gates.
- [x] Protected squash merge PR #2; its validated files match the merged tree.
- [x] Pass final theme follow-up CI and protected merge; verify final `web-flow` signature.
- [x] Synchronize main; pass main signing preflight and full merged-revision package validation.
- [x] Remove archived local backup branch after verified candidate gates.
- [x] Create `v1.0.0` at the verified main revision.
- [x] Authenticate uploaded assets on CI and publish the first stable release.

The [release process](release.md) completed final upload verification before
publication. Optional monitoring is not part of release acceptance and is stopped.

## Dependency maintenance

Dependabot alerts and automatic security-update PRs are enabled. Repository
secret scanning and push protection were already enabled and remain enabled.
Weekly version updates cover NuGet, GitHub Actions and the UI test driver's npm
manifest. Cache restore/save updates are grouped so they can be reviewed together.
Protected checks still apply; updates are not automatically merged.

The six open Dependabot PRs were reviewed during publication: #3/#6 update cache
actions, #4 updates artifact uploads, #5 updates artifact downloads, and #7/#8
update the test SDK/runner. All existing required checks passed, but the PRs need
validation against current main before merging. #5 changes only release workflow
steps, so ordinary CI does not exercise its new download action; validate that
change with a private candidate before using it for a future release. These PRs
do not change Music Player's app version or runtime dependency set. The project
and MSIX manifest already declare 1.0.0 and 1.0.0.0 respectively.
