using MusicPlayer.Core;
using Microsoft.Data.Sqlite;
using System.Reflection;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class ExactDuplicateTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-duplicates-" + Guid.NewGuid().ToString("N"));
    private readonly LibraryStore _store;

    public ExactDuplicateTests()
    {
        Directory.CreateDirectory(_root);
        _store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
    }

    [Fact]
    public void Exact_bytes_form_duplicate_groups_and_hidden_track_pages_keep_search_sort_and_page_semantics()
    {
        var identical = new byte[] { 1, 2, 3, 4, 5, 6 };
        var first = WriteTrack("01-copy.flac", identical);
        var second = WriteTrack("02-copy.flac", identical);
        var third = WriteTrack("03-copy.flac", identical) with { Title = "Special tag on a copy" };
        var sameTagsDifferentBytes = WriteTrack("04-different.flac", [9, 8, 7, 6, 5, 4]);
        _store.UpsertTracks([first, second, third, sameTagsDifferentBytes]);

        var groups = _store.GetDuplicateGroups(sort: TrackSort.Path, pageSize: 10);
        var group = Assert.Single(groups);
        Assert.Equal(3, group.CopyCount);
        Assert.Equal(first.Path, group.Representative.Path);
        Assert.Equal(identical.LongLength, group.FileSize);
        Assert.Equal(1, _store.CountDuplicateGroups());
        Assert.Equal(new[] { first.Path, second.Path, third.Path },
            _store.GetDuplicateTracks(group.Sha256, pageSize: 10).Select(track => track.Path));
        var searchedGroup = Assert.Single(_store.GetDuplicateGroups(search: "special", sort: TrackSort.Path));
        Assert.Equal(3, searchedGroup.CopyCount);
        Assert.Equal(third.Path, searchedGroup.Representative.Path);
        Assert.Equal(1, _store.CountDuplicateGroups(search: "special"));

        Assert.Equal(2, _store.CountTracks(hideExactDuplicates: true));
        Assert.Equal(2, _store.CountTracks(search: "same tags", hideExactDuplicates: true));
        var pageOne = _store.GetTracksPage(sort: TrackSort.Path, pageSize: 1, offset: 0, hideExactDuplicates: true).Single();
        var pageTwo = _store.GetTracksPage(sort: TrackSort.Path, pageSize: 1, offset: 1, hideExactDuplicates: true).Single();
        Assert.Equal(first.Path, pageOne.Path);
        Assert.Equal(sameTagsDifferentBytes.Path, pageTwo.Path);
        Assert.Equal(new[] { first.Path, sameTagsDifferentBytes.Path },
            _store.GetTrackPaths(sort: TrackSort.Path, hideExactDuplicates: true));
        var combinedTrackPage = _store.GetTracksPageWithCount(sort: TrackSort.Path, offset: 1, pageSize: 1, hideExactDuplicates: true);
        Assert.Equal(2, combinedTrackPage.TotalCount);
        Assert.Equal(sameTagsDifferentBytes.Path, Assert.Single(combinedTrackPage.Tracks).Path);
        var combinedGroupPage = _store.GetDuplicateGroupsPage(search: "special", sort: TrackSort.Path, pageSize: 1);
        Assert.Equal(1, combinedGroupPage.TotalCount);
        Assert.Equal(3, Assert.Single(combinedGroupPage.Groups).CopyCount);
    }

    [Fact]
    public void Fingerprints_are_recomputed_when_a_file_size_or_write_time_changes()
    {
        var bytes = new byte[] { 10, 20, 30, 40, 50, 60 };
        var firstPath = Path.Combine(_root, "first.flac");
        var secondPath = Path.Combine(_root, "second.flac");
        File.WriteAllBytes(firstPath, bytes);
        File.WriteAllBytes(secondPath, bytes);
        var first = ReadTrack(firstPath);
        var second = ReadTrack(secondPath);
        _store.UpsertTracks([first, second]);
        Assert.Single(_store.GetDuplicateGroups());

        File.WriteAllBytes(firstPath, [60, 50, 40, 30, 20, 10]);
        File.SetLastWriteTimeUtc(firstPath, DateTime.UtcNow.AddMinutes(2));
        _store.UpsertTrack(ReadTrack(firstPath)); // Rescan metadata invalidates the cached fingerprint snapshot.

        Assert.Empty(_store.GetDuplicateGroups());
        Assert.Equal(2, _store.CountTracks(hideExactDuplicates: true));
    }

    [Fact]
    public void File_changing_from_a_unique_indexed_size_into_a_duplicate_size_is_discovered()
    {
        var changingPath = Path.Combine(_root, "was-unique.flac");
        var matchingPath = Path.Combine(_root, "already-size-four.flac");
        File.WriteAllBytes(changingPath, [1, 2, 3]);
        File.WriteAllBytes(matchingPath, [1, 2, 3, 4]);
        _store.UpsertTracks([ReadTrack(changingPath), ReadTrack(matchingPath)]);
        Assert.Empty(_store.GetDuplicateGroups());

        File.WriteAllBytes(changingPath, [1, 2, 3, 4]);
        File.SetLastWriteTimeUtc(changingPath, DateTime.UtcNow.AddMinutes(3));
        _store.UpsertTrack(ReadTrack(changingPath)); // A rescan exposes the new size immediately.

        var group = Assert.Single(_store.GetDuplicateGroups());
        Assert.Equal(2, group.CopyCount);
    }

    [Fact]
    public void Duplicate_hashing_crosses_incremental_work_batches_without_losing_copies()
    {
        var bytes = new byte[] { 4, 3, 2, 1 };
        var tracks = Enumerable.Range(0, 40)
            .Select(index => WriteTrack($"batch-{index:D2}.flac", bytes))
            .ToArray();
        _store.UpsertTracks(tracks);

        var group = Assert.Single(_store.GetDuplicateGroups(pageSize: 1));

        Assert.Equal(40, group.CopyCount);
        Assert.Equal(40, _store.GetDuplicateTracksPage(group.Sha256, pageSize: 50).TotalCount);
    }

    [Fact]
    public void Removing_a_track_cascades_its_fingerprint_and_invalidates_the_snapshot()
    {
        var bytes = new byte[] { 7, 7, 7, 7 };
        var first = WriteTrack("keep.flac", bytes);
        var second = WriteTrack("remove.flac", bytes);
        _store.UpsertTracks([first, second]);
        Assert.Single(_store.GetDuplicateGroups());

        _store.RemoveTracks([second.Path]);

        using var connection = new SqliteConnection(new SqliteConnectionStringBuilder
        {
            DataSource = Path.Combine(_root, "library.db")
        }.ToString());
        connection.Open();
        using var orphanCount = connection.CreateCommand();
        orphanCount.CommandText = "SELECT COUNT(*) FROM track_fingerprints WHERE path=$path";
        orphanCount.Parameters.AddWithValue("$path", second.Path);
        Assert.Equal(0L, Convert.ToInt64(orphanCount.ExecuteScalar(), System.Globalization.CultureInfo.InvariantCulture));
        Assert.Empty(_store.GetDuplicateGroups());
        Assert.Equal(first.Path, _store.GetTrack(first.Path)!.Path);
    }

    [Fact]
    public void A_file_returning_after_an_inaccessible_scan_is_rehashed_on_retry()
    {
        var bytes = new byte[] { 6, 5, 4, 3, 2, 1 };
        var firstPath = Path.Combine(_root, "available.flac");
        var returningPath = Path.Combine(_root, "returning.flac");
        File.WriteAllBytes(firstPath, bytes);
        File.WriteAllBytes(returningPath, bytes);
        _store.UpsertTracks([ReadTrack(firstPath), ReadTrack(returningPath)]);
        Assert.Single(_store.GetDuplicateGroups());

        // A scan invalidates the 30-minute metadata snapshot before it inspects the missing path.
        typeof(LibraryStore).GetField("_fingerprintSnapshotCurrent", BindingFlags.Instance | BindingFlags.NonPublic)!
            .SetValue(_store, false);
        File.Delete(returningPath);
        Assert.Empty(_store.GetDuplicateGroups());

        File.WriteAllBytes(returningPath, bytes);
        typeof(LibraryStore).GetField("_unavailableFingerprintRetryUtc", BindingFlags.Instance | BindingFlags.NonPublic)!
            .SetValue(_store, DateTime.UtcNow.AddMinutes(-2)); // Simulate the retry interval without sleeping.
        Assert.Equal(2, Assert.Single(_store.GetDuplicateGroups()).CopyCount);
    }

    [Fact]
    public void Version_three_database_migrates_additively_and_preserves_existing_tracks()
    {
        var database = Path.Combine(_root, "version-three.db");
        var path = Path.Combine(_root, "existing.flac");
        var stamp = DateTime.UtcNow.ToString("O", System.Globalization.CultureInfo.InvariantCulture);
        using (var connection = new SqliteConnection(new SqliteConnectionStringBuilder { DataSource = database }.ToString()))
        {
            connection.Open();
            using var command = connection.CreateCommand();
            command.CommandText = """
                CREATE TABLE tracks(
                  path TEXT PRIMARY KEY,title TEXT NOT NULL,artist TEXT NOT NULL DEFAULT '',album TEXT NOT NULL DEFAULT '',album_artist TEXT NOT NULL DEFAULT '',
                  genre TEXT NOT NULL DEFAULT '',year INTEGER NOT NULL DEFAULT 0,track_number INTEGER NOT NULL DEFAULT 0,duration_ms INTEGER NOT NULL DEFAULT 0,
                  file_size INTEGER NOT NULL DEFAULT 0,modified_utc TEXT NOT NULL,added_utc TEXT NOT NULL,favorite INTEGER NOT NULL DEFAULT 0,
                  rating INTEGER NOT NULL DEFAULT 0,play_count INTEGER NOT NULL DEFAULT 0,last_played_utc TEXT NULL,artwork_path TEXT NULL);
                INSERT INTO tracks(path,title,modified_utc,added_utc) VALUES($path,'Keep me',$stamp,$stamp);
                PRAGMA user_version=3;
                """;
            command.Parameters.AddWithValue("$path", path);
            command.Parameters.AddWithValue("$stamp", stamp);
            command.ExecuteNonQuery();
        }

        var migrated = new LibraryStore(database, new LocalAppLog(Path.Combine(_root, "MigrationLogs")));

        Assert.Equal("Keep me", migrated.GetTrack(path)!.Title);
        using var verify = new SqliteConnection(new SqliteConnectionStringBuilder { DataSource = database }.ToString());
        verify.Open();
        using var version = verify.CreateCommand(); version.CommandText = "PRAGMA user_version";
        Assert.Equal(10L, Convert.ToInt64(version.ExecuteScalar(), System.Globalization.CultureInfo.InvariantCulture));
        using var table = verify.CreateCommand(); table.CommandText = "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='track_fingerprints'";
        Assert.Equal(1L, Convert.ToInt64(table.ExecuteScalar(), System.Globalization.CultureInfo.InvariantCulture));
        using var trigger = verify.CreateCommand(); trigger.CommandText = "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='track_fingerprints_invalidate_after_track_update'";
        Assert.Equal(1L, Convert.ToInt64(trigger.ExecuteScalar(), System.Globalization.CultureInfo.InvariantCulture));
    }

    private Track WriteTrack(string filename, byte[] content)
    {
        var path = Path.Combine(_root, filename);
        File.WriteAllBytes(path, content);
        return ReadTrack(path);
    }

    private static Track ReadTrack(string path)
    {
        var info = new FileInfo(path);
        return new Track(Path.GetFullPath(path), "Same tags", "Artist", "Album", "Artist", "Jazz", 2024, 1,
            TimeSpan.FromMinutes(3), info.Length, info.LastWriteTimeUtc, DateTime.UtcNow);
    }

    public void Dispose()
    {
        SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, recursive: true);
    }
}
