# Library browsing regression checks

Album, artist, and genre pages fetch 60 cards. Their old Load more handler advanced
by 200 and called a renderer without inserting its returned HTML. All Load more
actions now use the shared render path and advance by the records actually fetched.
Failed requests retain the button and offset for retry; concurrent clicks and stale
responses cannot skip batches or append to another view. Folder batches stay before
the library-roots section, and a short final batch no longer repeats previous cards.
Playlist rows remain in the loaded collection. Search pages recover a valid page if
results shrink. Artist album requests use one extra record to determine whether
another batch exists, including when the final page contains exactly 30 albums.

Duplicate hiding applies to general track lists and their matching queues. Album,
artist, genre, folder, and playlist contents remain complete. Group counts and pages
share the same visible-label predicate and deterministic ordering. No saved tags,
library entries, or user settings are rewritten by these changes.

Local validation on 2026-10-10:

- `dotnet test tests/MusicPlayer.Tests/MusicPlayer.Tests.csproj --no-restore --configuration Release`: 101/101 passed, including 13 new browsing cases.
- `node --test tests/MusicPlayer.Ui.Tests/lyrics-theme.test.mjs tests/MusicPlayer.Ui.Tests/library-pagination.test.mjs`: 44/44 passed. The browsing tests exercise the real renderer with a captured-markup DOM fixture and mocked native responses, including loading all 989 cards without omission or repetition.
- `node --check` passed for all six WebUI scripts; `git diff --check` passed.
- Isolated x64 Release app build passed with zero warnings/errors using the existing cached-package override for VLC 3.0.23.1. The tracked release dependency remains VLC 3.0.24.

These checks do not constitute installed-app interaction validation. The running
user instance was not stopped or relaunched. CI now includes the new Node suite.

## Final candidate evidence

At `b1316ef`, [CI 38053176126](https://github.com/kalabhaftu/music-player/actions/runs/38053176126)
passed all 101 core and 44 frontend cases, including the pagination cases above,
and complete Windows UI checks on x64/ARM64. Signed installed setup/MSIX gates
passed in [38053565166](https://github.com/kalabhaftu/music-player/actions/runs/38053565166).
