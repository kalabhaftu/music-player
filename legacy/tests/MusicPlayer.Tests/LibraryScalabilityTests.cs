using MusicPlayer.Core;
using Microsoft.Data.Sqlite;
using System.Diagnostics;
using System.Runtime.InteropServices;
using Xunit;
using Xunit.Abstractions;

namespace MusicPlayer.Tests;

public sealed class LibraryScalabilityTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-scale-tests-" + Guid.NewGuid().ToString("N"));
    private readonly LibraryStore _store;
    private readonly ITestOutputHelper _output;

    public LibraryScalabilityTests(ITestOutputHelper output)
    {
        _output = output;
        Directory.CreateDirectory(_root);
        _store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
    }

    [Fact]
    public void V1_migration_preserves_library_session_settings_and_playlist_occurrences()
    {
        var database = Path.Combine(_root, "legacy.db");
        var trackPath = Path.Combine(_root, "legacy-song.flac");
        var created = DateTime.UtcNow.ToString("O");
        using (var connection = new SqliteConnection(new SqliteConnectionStringBuilder { DataSource = database }.ToString()))
        {
            connection.Open();
            using var command = connection.CreateCommand();
            command.CommandText = """
                CREATE TABLE tracks(path TEXT PRIMARY KEY,title TEXT NOT NULL,artist TEXT NOT NULL DEFAULT '',album TEXT NOT NULL DEFAULT '',album_artist TEXT NOT NULL DEFAULT '',
                  genre TEXT NOT NULL DEFAULT '',year INTEGER NOT NULL DEFAULT 0,track_number INTEGER NOT NULL DEFAULT 0,duration_ms INTEGER NOT NULL DEFAULT 0,
                  file_size INTEGER NOT NULL DEFAULT 0,modified_utc TEXT NOT NULL,added_utc TEXT NOT NULL,favorite INTEGER NOT NULL DEFAULT 0,
                  rating INTEGER NOT NULL DEFAULT 0,play_count INTEGER NOT NULL DEFAULT 0,last_played_utc TEXT NULL,artwork_path TEXT NULL);
                CREATE TABLE playlists(id TEXT PRIMARY KEY,name TEXT NOT NULL,created_utc TEXT NOT NULL);
                CREATE TABLE playlist_tracks(playlist_id TEXT NOT NULL,position INTEGER NOT NULL,track_path TEXT NOT NULL,PRIMARY KEY(playlist_id,position));
                CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                CREATE TABLE playback_session(id INTEGER PRIMARY KEY,payload TEXT NOT NULL);
                CREATE TABLE tag_backups(id INTEGER PRIMARY KEY AUTOINCREMENT,original_path TEXT NOT NULL,backup_path TEXT NOT NULL,created_utc TEXT NOT NULL);
                INSERT INTO tracks(path,title,artist,album,album_artist,genre,year,track_number,duration_ms,file_size,modified_utc,added_utc)
                  VALUES($path,'Legacy title','Legacy artist','Album','Legacy artist','Jazz',2001,2,90000,123,$stamp,$stamp);
                INSERT INTO playlists(id,name,created_utc) VALUES('legacy-list','Legacy',$stamp);
                INSERT INTO playlist_tracks(playlist_id,position,track_path) VALUES('legacy-list',0,$path),('legacy-list',1,$path);
                INSERT INTO settings(key,value) VALUES('theme','Dark');
                INSERT INTO playback_session(id,payload) VALUES(1,'{"TrackPath":null,"PositionMilliseconds":0,"Queue":[],"Shuffle":false,"RepeatMode":"Off"}');
                PRAGMA user_version=1;
                """;
            command.Parameters.AddWithValue("$path", trackPath);
            command.Parameters.AddWithValue("$stamp", created);
            command.ExecuteNonQuery();
        }

        var migrated = new LibraryStore(database, new LocalAppLog(Path.Combine(_root, "MigrationLogs")));

        Assert.Equal("Legacy title", migrated.GetTrack(trackPath)!.Title);
        Assert.Equal("Dark", migrated.GetSetting("theme"));
        Assert.Equal(new[] { trackPath, trackPath }, migrated.GetPlaylists().Single().Paths);
        Assert.Equal(trackPath, migrated.GetTracksPage("gacy", pageSize: 1).Single().Path);
        Assert.Equal(2, Directory.GetFiles(_root, "legacy.db.migration-*.bak").Length);
    }

    [Fact]
    public void Fts_substring_search_preserves_short_wildcard_and_stable_paging_semantics()
    {
        var tracks = new[]
        {
            MakeTrack("Alpha% Echo", Path.Combine(_root, "one", "first.flac"), "Café Ensemble"),
            MakeTrack("Echo Alpha", Path.Combine(_root, "two", "second.flac"), "Other Artist"),
            MakeTrack("The Echoes", Path.Combine(_root, "three", "third.flac"), "Other Artist"),
            MakeTrack("Unrelated", Path.Combine(_root, "four", "fourth.flac"), "Other Artist")
        };
        _store.UpsertTracks(tracks);

        Assert.Equal(new[] { tracks[0].Path }, _store.GetTracks(search: "%").Select(track => track.Path));
        Assert.Equal(3, _store.CountTracks(search: "ech"));
        Assert.Equal(new[] { tracks[0].Path, tracks[1].Path, tracks[2].Path }, _store.GetTrackPaths(search: "ECH"));

        var all = _store.GetTracks(search: "ech").Select(track => track.Path).ToArray();
        var paged = _store.GetTracksPage("ech", offset: 0, pageSize: 2)
            .Concat(_store.GetTracksPage("ech", offset: 2, pageSize: 2)).Select(track => track.Path).ToArray();
        Assert.Equal(all, paged);
        Assert.Equal(all, _store.GetTrackPaths(search: "ech"));
        Assert.Equal(new[] { tracks[0].Path }, _store.GetTracks(search: "%").Select(track => track.Path));
    }

    [Fact]
    public void Windows_x64_first_page_search_meets_250ms_p95_on_a_deterministic_100k_fixture()
    {
        // The performance target is specified for the Windows x64 CI runner. Keep the
        // deterministic database behavior covered on every platform by the other tests.
        if (!OperatingSystem.IsWindows() || RuntimeInformation.ProcessArchitecture != Architecture.X64) return;

        _store.UpsertTracks(Enumerable.Range(0, 100_000).Select(index =>
        {
            var artist = index % 100 == 0 ? "Orchard Ensemble" : "Local Artist";
            return MakeTrack($"Track {index:D6}", Path.Combine(_root, "fixture", $"{index:D6}.flac"), artist);
        }));
        Assert.Equal(1_000, _store.CountTracks(search: "orch"));

        // Warm SQLite's file and query plans, then measure the actual page API used by the UI.
        for (var index = 0; index < 3; index++) Assert.Equal(50, _store.GetTracksPage("orch", pageSize: 50).Count);
        var samples = new double[40];
        for (var index = 0; index < samples.Length; index++)
        {
            var timer = Stopwatch.StartNew();
            Assert.Equal(50, _store.GetTracksPage("orch", pageSize: 50).Count);
            timer.Stop();
            samples[index] = timer.Elapsed.TotalMilliseconds;
        }
        Array.Sort(samples);
        var p95 = samples[(int)Math.Ceiling(samples.Length * 0.95) - 1];
        _output.WriteLine($"Windows x64 100k-track first-page search: p95={p95:F1} ms, max={samples[^1]:F1} ms.");

        Assert.True(p95 < 250,
            $"100k-track first-page search p95 was {p95:F1} ms; target is below 250 ms. Samples: {string.Join(", ", samples.Select(value => value.ToString("F1", System.Globalization.CultureInfo.InvariantCulture)))}");
    }

    [Fact]
    public void Removing_a_root_keeps_overlapping_tracks_and_playlist_paths()
    {
        var removedRoot = Path.Combine(_root, "Music");
        var overlappingRoot = Path.Combine(removedRoot, "Albums");
        var covered = Path.Combine(overlappingRoot, "covered.flac");
        var removed = Path.Combine(removedRoot, "solo", "removed.flac");
        var anotherRoot = Path.Combine(_root, "AnotherMusic");
        var anotherRemoved = Path.Combine(anotherRoot, "another.flac");
        var outside = Path.Combine(_root, "Elsewhere", "outside.flac");
        _store.UpsertTracks([MakeTrack("Covered", covered), MakeTrack("Removed", removed), MakeTrack("Another", anotherRemoved), MakeTrack("Outside", outside)]);
        var playlist = _store.CreatePlaylist("Keep references", [removed, covered, anotherRemoved]);
        Directory.CreateDirectory(Path.GetDirectoryName(removed)!);
        File.WriteAllText(removed, "This test file must not be deleted by root removal.");
        Directory.CreateDirectory(anotherRoot);
        File.WriteAllText(anotherRemoved, "This test file must also remain on disk.");

        var deletedCount = _store.RemoveTracksUnderUnselectedRoots([removedRoot, anotherRoot], [overlappingRoot, Path.GetDirectoryName(outside)!]);

        Assert.Equal(2, deletedCount);
        Assert.Null(_store.GetTrack(removed));
        Assert.Null(_store.GetTrack(anotherRemoved));
        Assert.NotNull(_store.GetTrack(covered));
        Assert.NotNull(_store.GetTrack(outside));
        Assert.Equal(new[] { removed, covered, anotherRemoved }, _store.GetPlaylists().Single(item => item.Id == playlist.Id).Paths);
        Assert.True(File.Exists(removed)); // Only the index is changed; media files remain untouched.
        Assert.True(File.Exists(anotherRemoved));
    }

    [Fact]
    public void Playlist_entry_pages_keep_duplicate_positions_and_unindexed_paths()
    {
        var knownPath = Path.Combine(_root, "known.flac");
        var missingPath = Path.Combine(_root, "missing", "missing.flac");
        _store.UpsertTrack(MakeTrack("Known Echo", knownPath));
        var playlist = _store.CreatePlaylist("Duplicates", [knownPath, missingPath, knownPath]);

        var entries = _store.GetPlaylistEntriesPage(playlist.Id, offset: 0, pageSize: 2);

        Assert.Equal(new[] { 0, 1 }, entries.Select(entry => entry.Position));
        Assert.Equal(new[] { knownPath, missingPath }, entries.Select(entry => entry.Path));
        Assert.NotNull(entries[0].Track);
        Assert.Null(entries[1].Track);
        Assert.Equal(new[] { knownPath, knownPath }, _store.GetPlaylistPaths(playlist.Id, "ech"));
        Assert.Equal(2, _store.CountPlaylistEntries(playlist.Id, "ech"));
        Assert.Equal(new[] { knownPath, missingPath, knownPath }, _store.GetPlaylistPaths(playlist.Id));
    }

    [Fact]
    public void Incomplete_scan_generation_preserves_root_and_excluded_subtree_rows()
    {
        var root = Path.Combine(_root, "ScanRoot");
        var inaccessible = Path.Combine(root, "Private", "old.flac");
        var systemExcluded = Path.Combine(root, "Program Files", "old.flac");
        _store.UpsertTracks([MakeTrack("Private", inaccessible), MakeTrack("Excluded", systemExcluded)]);

        using (var scan = _store.BeginScan([root]))
        {
            scan.MarkRootCompleted(root, false);
            Assert.Equal(0, scan.Complete());
        }
        Assert.NotNull(_store.GetTrack(inaccessible));

        using (var scan = _store.BeginScan([root]))
        {
            scan.MarkExcludedPath(Path.Combine(root, "Program Files"));
            scan.MarkRootCompleted(root, true);
            Assert.Equal(1, scan.Complete());
        }
        Assert.Null(_store.GetTrack(inaccessible));
        Assert.NotNull(_store.GetTrack(systemExcluded));
    }

    [Fact]
    public void Tag_backup_index_keeps_only_five_newest_entries_per_exact_track_path()
    {
        var path = Path.Combine(_root, "track.flac");
        for (var index = 0; index < 7; index++)
            _store.RecordTagBackup(new TagBackup(path, Path.Combine(_root, $"backup-{index}.flac"), DateTime.UtcNow.AddMinutes(index)));
        _store.RecordTagBackup(new TagBackup(Path.Combine(_root, "other.flac"), Path.Combine(_root, "other-backup.flac"), DateTime.UtcNow));

        var retained = _store.GetTagBackups(path);
        Assert.Equal(5, retained.Count);
        Assert.Equal(Path.Combine(_root, "backup-6.flac"), retained[0].BackupPath);
        Assert.Equal(Path.Combine(_root, "backup-2.flac"), retained[^1].BackupPath);
    }

    [Fact]
    public void Artwork_sweep_deletes_only_unreferenced_flat_cache_files()
    {
        var cache = Path.Combine(_root, "Artwork");
        Directory.CreateDirectory(cache);
        var shared = Path.Combine(cache, "shared.png");
        var orphan = Path.Combine(cache, "orphan.jpg");
        var unrelated = Path.Combine(cache, "notes.txt");
        File.WriteAllBytes(shared, [1, 2, 3]);
        File.WriteAllBytes(orphan, [4, 5, 6]);
        File.WriteAllText(unrelated, "not artwork");
        _store.UpsertTracks([
            MakeTrack("First", Path.Combine(_root, "first.flac")) with { ArtworkPath = shared },
            MakeTrack("Second", Path.Combine(_root, "second.flac")) with { ArtworkPath = shared }
        ]);

        Assert.Equal(1, _store.PruneUnreferencedArtwork(cache));
        Assert.True(File.Exists(shared));
        Assert.False(File.Exists(orphan));
        Assert.True(File.Exists(unrelated));
    }

    private Track MakeTrack(string title, string path, string artist = "Artist") => new(
        Path.GetFullPath(path), title, artist, "Album", artist, "Jazz", 2024, 1, TimeSpan.FromMinutes(3), 1200,
        DateTime.UtcNow, DateTime.UtcNow);

    public void Dispose()
    {
        SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, true);
    }
}
