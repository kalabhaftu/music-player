# Playback stopping and queue advancement investigation

Investigated on 2026-10-09 against the local Windows logs, native LibVLC playback, and commits `1663ace`, `36e7643`, and `1ba318d`.

## Root cause

The app log repeatedly recorded `LibVLC end event ignored: event came from inactive player` while the current song was ending. `PlaybackService` compared the event's `sender` directly with its active `MediaPlayer`. LibVLCSharp actually passes its internal `MediaPlayerEventManager` as sender. Consequently every real end callback was rejected, `TrackEnded` was never raised, and the queue never received automatic advancement. Error callbacks used the same incorrect comparison.

The binding's behavior is visible in [VideoLAN's event manager source](https://github.com/videolan/libvlcsharp/blob/3.x/src/LibVLCSharp/Shared/Events/MediaPlayerEventManager.cs).

The prior fixes changed state checks and dispatcher scheduling but retained the sender comparison. Their core policy test supplied booleans directly, so it never exercised the actual native event sender and could not detect this failure.

## Seeking failures

`1ba318d` reloaded an ended song using `LoadPaused`, then returned without starting it. Dragging the progress slider back therefore prepared a paused song instead of resuming playback. Separately, seeking a restored, unstarted session cleared `_restorePosition` and assigned native `Time` before an input existed; the requested position was lost. Deferred seeking also checked only media length, which does not establish that the input can accept a seek.

## Correction

- Subscriptions capture the actual owning player and its media generation, rather than using the native event sender.
- Native callbacks queue their handling without taking the service lock or issuing playback commands. Queued events from replaced media are rejected; duplicate end notifications are suppressed.
- End/error callbacks are authoritative for their media generation. Reading native `State` later loses errors when VLC has already stopped.
- Seeking an ended song reloads and resumes at the requested position. Seeking a paused or restored song preserves pause and retains the new position until the decoder is seekable. Resume also resets ended media correctly.
- Canceling or failing a crossfade rebinds events to the surviving outgoing media, allowing its natural end to advance the queue. A successful promotion clears ended state for the incoming track.
- Seeking after the final queue entry resumes Windows media controls and listening tracking as well as the engine.

## Validation

The first three native regression tests failed against the previous implementation: queue progression timed out, seeking after end did not resume, and restored seeking returned position zero instead of the requested 1000 ms.

`tests/MusicPlayer.Playback.Tests` links the actual app playback service and queue coordinators, loads the shipped LibVLC packages, and generates silent WAV fixtures. Its dummy audio output allows real native event/decoder tests without an audio device. Windows x64 CI runs this suite before packaging. Tests cover queue advancement and final stop, seeking after natural end and after queue stop, restored and paused seeking, repeat, crossfade promotion, failed crossfade recovery, and native file-open errors.

All 9 native playback cases and the 62 existing core tests passed locally. The x64 WinUI build passed with zero warnings and zero errors. Native tests do not establish that every supported media format, physical output device, or WebView interaction works; those remain separate Windows validation tasks in the project status tracker.

## Codec seek investigation (2026-10-10)

The 19-format native suite at `0554caa` exposed default Ogg/OGA seeking that
reported Ended immediately after assigning Time=3000. A direct LibVLC reproduction
with the official FFmpeg Vorbis corpus reproduces the failure independently of
PlaybackService. Selecting VLC's bundled `avformat` reader allows the same file
to advance past 3100 ms and pass seek/pause/resume checks. SetMedia applies this
selection to Ogg/OGA, including crossfade inputs. The DFF reader experiment did
not pass and was removed; ARM64 DFF opening remains a release gate.

The format suite also exposed TagLib's missing `.wave` extension mapping. The
WAV type override is shared by indexing, tag editing, lyrics and track details.
The core regression edits a genuine PCM `.WAVE`, reads its metadata and embedded
lyrics, then verifies byte-for-byte recovery. Full format checks run on both
architectures; no failing format is silently excluded.

## DFF EOF correction and current checks (2026-10-10)

A cold DFF file reader spent over 20 seconds repeatedly rejecting FFmpeg's seek
to the exact file end during IFF header parsing. The documented StreamMediaInput
unknown-size mode accepts that seek and opens the same bytes in about one second.
A bounded read of uncompressed DSDIFF headers supplies the duration without
loading or rewriting audio. Inputs belong to their actual native player across
crossfades and are disposed after playback releases them. Compressed DST retains
the default reader. Native checks cover DFF seek, natural end/restart and exclusive
file reopening after disposal. Core checks cover duration, indexing, odd-length
unknown chunks and truncated payload rejection.

At `19577d5`, all 49 native cases passed on x64 and ARM64 (LibVLC 3.0.24), and all
76 core cases passed. x64 completed both initial and restarted WebView smoke
checks including native media keys. The ARM64 tray failure was reproduced with
a stock Windows notification icon and traced to the runner's unfinished OOBE
privacy screen. Disposable-desktop initialization corrected the independent
probe. At `7bfc083`, both architectures again passed all 49 native cases; x64
passed the complete portable UI gate, while ARM64 still required closing the
observed Start menu before its real fullscreen Escape interaction.

The installed MKV fixture also exposed null artist/genre arrays in TagLib's
Matroska reader. `79b1ba7` treats these arrays as empty, preserving fallback
titles and native duration; the genuine video regression now indexes its MKV
through the real TrackReader before playback.
Local cached-engine checks also passed 11/11 playback/queue/DFF cases with the
older cached LibVLC 3.0.23.1; these are supplementary, not the final engine gate.

## Completed code candidate (2026-10-10)

At `b1316ef`, all protected [CI checks](https://github.com/kalabhaftu/music-player/actions/runs/38053176126)
passed: 101 core cases, 44 frontend cases, 53 native cases per architecture
(three physical-endpoint cases skipped on runners without an audio device),
and complete portable UI/restart flows. The corresponding
[signed candidate](https://github.com/kalabhaftu/music-player/actions/runs/38053565166)
passed all setup/MSIX interaction, install/upgrade/uninstall and resource gates
on both architectures. These results resolve the earlier ARM64 input and
installed-package gates; audible hardware quality remains separate evidence.

The final ARM64 input obstruction was Windows' packaged `WWAHost` Microsoft
account welcome window, omitted by desktop-only `EnumWindows`. The disposable
test uses `EnumDesktopWindows` and the actual obstructing HWND, validates exact
title/system owner/session, then waits for official foreground activation.
It still requires genuine input and actual fullscreen bounds. Fixture generation
runs in a child process so an installed TagLib assembly cannot remain locked
during upgrade. Neither correction affects the user's running app.
