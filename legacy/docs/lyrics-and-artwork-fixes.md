# Lyrics and artwork corrections

Validated locally on 2026-10-10. This change is limited to artwork palettes and lyrics lookup/display; it does not establish the separate installed-package release gates.

## Root causes and behavior

- Artwork surfaces used an arithmetic RGB average, which can invent a hue between unrelated colors. The accent selector split related shades into many RGB bins, while transparent pixels and ignored embedded color profiles could skew the result. The replacement groups related cover hues, requires a meaningful share of visible pixels, keeps grayscale neutral, and uses the dominant supported hue for surfaces. Accent text has at least 4.5:1 contrast against generated surfaces. Windows converts decoded artwork to sRGB and retains straight alpha before sampling. Failed stale artwork requests cannot clear the current track's palette. Now Playing titles use the artwork accent; control/text-selection ink uses linear sRGB contrast.
- Both native and frontend lookup applied arbitrary cooldowns after ordinary connection failures. Only actual HTTP 429 responses now impose cooldowns, using the server's Retry-After. Catalog requests stay serialized and spaced by 1.3 seconds. A request has a 45-second HTTP timeout, one transient retry, and a 100-second total deadline including queuing; the search bridge allows 110 seconds. Manual lookup can immediately retry empty automatic results or connection failures. Automatic/manual callers for one track share pending work, and successful search/display caches are bounded to 128 tracks.
- Automatic matching could choose plain lyrics before another confident result with timestamps. Matching now prefers usable timed lyrics after checking title, artist and available duration. Non-Latin titles are preserved. Malformed timed content falls back to provided plain lyrics. Manual results identify timed/untimed content.
- Frontend parsing applied an LRC offset only to lines after the offset tag and discarded negative shifted times; this differed from native parsing. It now applies the final offset to every line, preserves negative shifted times, validates seconds, and hides metadata in plain display. Native parsing also handles a leading BOM. Both views use the same active-line lookup, follow backward seeks, and avoid repeated scrolling when the active line is unchanged.

Online automatic lyrics remain a display cache. Existing timed lyrics take precedence. With automatic lookup enabled, an untimed embedded lyric or sidecar now permits a lookup for a confident timed version for display. The original file stays untouched and remains the fallback for offline, missing, plain-only, or mismatched results. Untimed online results expire after five minutes so new timestamps can be found; timed results expire after thirty minutes. Persisting lyrics still uses the explicit editor save, with sidecar/embedded storage and backups unchanged. Untimed lyrics cannot accurately follow playback; the UI labels this rather than manufacturing timestamps.

## Verification

- `dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj --no-restore --verbosity minimal`: **88 passed**, no failures/skips. Covers palette transparency/dominant hues/grayscale/contrast, HTTP transient retry, immediate retry after failure, actual rate limits, cancellation, Unicode queries, and LRC save/reload timing.
- `node --test tests/MusicPlayer.Ui.Tests/lyrics-theme.test.mjs`: **11 passed**. Covers automatic timed selection, wrong-version/artist/duration rejection, Unicode matching, trailing/negative offsets, malformed timing, slow pending-request sharing, manual retries, server cooldowns, contrast, and calls to both views' scrolling implementation including backward seeking. Added to the Linux required CI job; remote CI has not been claimed as passing by this local change.
- Node syntax checks: all WebUI scripts passed.
- `dotnet build src/MusicPlayer.App/MusicPlayer.App.csproj --configuration Release --no-restore -p:Platform=x64 -p:RuntimeIdentifier=win-x64 -p:AppxPackageSigningEnabled=false -p:GenerateAppxPackageOnBuild=false -o artifacts/lyrics-theme-build`: **passed, zero warnings/errors**.
- A read-only live LRCLIB search for NF's “I Miss The Days” returned timed lyrics. No lyric content, credentials, or user music files were logged or changed by the check.

The running app was not opened/restarted for validation. These tests exercise the scroll implementation with a DOM fixture; manual visual review of real album covers in the installed app remains separate.

The decoder uses Microsoft's documented [ColorManageToSRgb](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.imaging.colormanagementmode) option. Catalog documentation is available at [LRCLIB](https://lrclib.net/docs).

## Follow-up verification, 2026-10-10

`node --test tests/MusicPlayer.Ui.Tests/lyrics-theme.test.mjs` now passes **15/15**,
including an untimed local lyric upgraded for display, local fallback on network
failure or a wrong match, avoiding requests for timed local lyrics or disabled
auto-fetch, and refreshing expired untimed results. All six WebUI scripts pass
Node syntax checks. The isolated x64 app compile passes with zero warnings/errors;
its native-package limitation is recorded in [music-crossfade.md](music-crossfade.md).
