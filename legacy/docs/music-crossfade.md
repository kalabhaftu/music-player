# Music crossfade correction

The music fade uses two LibVLC players. VLC 3's MMDevice output changes the
Windows audio session's master volume, so the outgoing and incoming players
could change each other's volume instead of fading independently. See the
[VLC 3 MMDevice implementation](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_output/mmdevice.c).

Music now uses VLC's bundled DirectSound output, whose volume belongs to each
audio buffer. Video continues using MMDevice; video does not crossfade. Saved
Windows endpoint IDs map through the documented
[PKEY_AudioEndpoint_GUID](https://learn.microsoft.com/en-us/windows/win32/coreaudio/pkey-audioendpoint-guid),
including normalization of WinRT interface IDs. Changing output restarts music
at its saved position and restores the queue if a fade was in progress.

The outgoing track retains full volume while the incoming playback clock is
starting. Once ready, a monotonic clock drives 20 ms fade updates. The curve
preserves combined audio power and accounts for VLC's cubic volume scale;
changing the master volume preserves this curve. A failed or canceled fade
restores the outgoing player, including its end notification if it ended while
the fade was waiting. VLC volume persistence is disabled because the app already
saves its own master volume, and fade steps should not rewrite VLC preferences.

## Local verification, 2026-10-10

- 17 playback, queue-revision, and crossfade checks passed. New checks cover
  independent real Windows player volumes, curve power, master-volume changes,
  mid-fade seek/pause, endpoint mapping, and position-preserving output changes.
  Existing checks cover promotion/queue advancement, missing-file failure,
  natural end, repeated playback, and seeking after end.
- The x64 Release app compile passed with zero warnings/errors. Output and
  intermediate files are isolated under `artifacts/crossfade-app-check`.
- The full core suite passed **88/88** on its final rerun. An earlier run had
  one failure in `Removing_a_track_cascades_its_fingerprint_and_invalidates_the_snapshot`;
  that test then passed in isolation and in the full rerun. This scoped change
  does not edit the core database/fingerprint implementation.
- Playback and app checks used cached `VideoLAN.LibVLC.Windows` **3.0.23.1**,
  selected only in an ignored verification project/property file. The shared
  candidate requests **3.0.24**, whose restore currently fails because the package
  download ends prematurely. The tracked dependency selection was not changed.
- Native checks use silent WAV fixtures and do not open/restart the user's app.
  Audible transition quality on the user's device, output hot-plug behavior,
  and the candidate's 3.0.24 runtime still need separate verification. This
  evidence does not establish installed-package or release readiness.

The focused native check uses `MUSICPLAYER_TEST_WINDOWS_AUDIO=1` on this machine.
The three real audio-endpoint checks also run automatically when endpoint
enumeration finds a Windows output; only machines with no output skip them.
The environment override requires these checks even if enumeration is empty.
Command:
`dotnet test artifacts/crossfade-validation/CachedNative.Tests.csproj --no-restore --filter "FullyQualifiedName~PlaybackIntegrationTests|FullyQualifiedName~CrossfadeIntegrationTests|FullyQualifiedName~QueueRevisionTests" --verbosity quiet`.
The ignored verification project mirrors the checked-in playback project with
the cached native package selected explicitly.

## Release-engine candidate checks

At `b1316ef`, [CI 38053176126](https://github.com/kalabhaftu/music-player/actions/runs/38053176126)
passed 53 native cases on each architecture with LibVLC 3.0.24. Three physical
endpoint cases were skipped because those runners have no audio device;
cached-engine physical checks above remain separate. The complete signed
setup/MSIX and native UI gates also passed on both architectures in
[38053565166](https://github.com/kalabhaftu/music-player/actions/runs/38053565166).
Audible quality and arbitrary output-device hot-plug combinations are not
claimed to have been tested on every user's hardware.
