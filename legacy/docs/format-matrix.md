# Audio and tag format matrix

The playback backend is bundled LibVLC. The list below records formats the scanner recognizes; it does not assert that every file variant, codec, metadata field or device configuration has passed Windows playback tests.

| File extension | Indexed | Playback status |
|---|---:|---|
| MP3, WAV/WAVE, FLAC | Yes | Passed x64 + ARM64 native fixture checks at `b1316ef` |
| AAC, M4A, M4B | Yes | Passed x64 + ARM64 native fixture checks at `b1316ef` |
| Ogg, OGA, Opus | Yes | Passed x64 + ARM64 native fixture checks at `b1316ef` |
| WMA, APE, WV, TTA, MPC | Yes | Passed x64 + ARM64 native fixture checks at `b1316ef` |
| AIFF/AIF, DSF, DFF | Yes | Passed x64 + ARM64 native fixture checks at `b1316ef` |

TagLib# exposes the common title, artist, album, genre, track/disc numbering, composers, comments, sorting fields, grouping, BPM, conductor, copyright, publisher, ISRC, MusicBrainz IDs, ReplayGain, lyrics and artwork properties where the container supports them. The editor also exposes custom Xiph/Vorbis comments, ID3v2 text frames and user text, ASF text descriptors, and APEv2 text items; custom fields are unavailable for M4A/M4B. ID3v2 frame IDs use `ID3:XXXX` notation. TagLib# may ignore a standard property unsupported by a particular container. At `19577d5`, 17 editable formats passed title changes, preservation of artist/album/duration, and byte-for-byte backup/restore on x64 and ARM64. DFF and TTA are read-only with TagLib# 2.3; tests verify safe rejection without changing media bytes. Artwork and uncommon file variants still need separate validation.

Playback support depends on the bundled LGPL package and its modules. Codec patent rules vary by country; users and distributors are responsible for checking local requirements. No GPL-only LibVLC package is used.

## Optional video files

Video extensions are excluded from library scans by default. They can be enabled or added under **Music folders → Scan exclusions → File extensions**. Enabled video files open in a separate, resizable Music Player window with playback, seek, volume, previous, and next controls. This uses the LibVLC engine already bundled with the app; playback still depends on the codecs available for that file. Native H.264/AAC MKV rendering, embedded subtitles, rate changes, seeking and PNG snapshots passed x64 and ARM64 fixture tests at `19577d5`. The x64 portable video window also passed pause/play, seek, speed, subtitle selection, fullscreen/Escape and PNG snapshot interactions at `a73e62f`. Installed-package and ARM64 interactions remain separate gates.

Fixture provenance: `tests/MusicPlayer.Playback.Tests/Prepare-MediaFixtures.ps1` generates genuine FFmpeg-encoded files and DSD silence containers, and obtains APE/MPC files from the official FFmpeg codec corpus. Final code CI [38053176126](https://github.com/kalabhaftu/music-player/actions/runs/38053176126) records decode/seek/pause/resume and metadata checks on both architectures using LibVLC 3.0.24. This validates native decoding rather than physical speakers or every codec variant.

At `b1316ef`, complete portable video interaction checks passed on x64 and ARM64.
Signed installed setup/MSIX UI checks passed in [38053565166](https://github.com/kalabhaftu/music-player/actions/runs/38053565166),
including pause/play, seek, speed, embedded subtitle selection, fullscreen with
real Escape input, PNG snapshots, close/pause and restart. These results supersede
the earlier open installed/ARM64 interaction gates described above.
