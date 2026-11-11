using MusicPlayer.Core;
using Microsoft.Data.Sqlite;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class LibraryScanPruningTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-scan-prune-" + Guid.NewGuid().ToString("N"));
    private readonly LibraryStore _store;

    public LibraryScanPruningTests()
    {
        Directory.CreateDirectory(_root);
        _store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
    }

    [Fact]
    public async Task Complete_parent_scan_prunes_previously_indexed_track_under_default_exclusion()
    {
        var root = Directory.CreateDirectory(Path.Combine(_root, "MusicRoot")).FullName;
        var excluded = Directory.CreateDirectory(Path.Combine(root, "Games")).FullName;
        var trackPath = Path.Combine(excluded, "old-game-audio.mp3");
        Seed(trackPath);

        var result = await Scan([root]);

        Assert.Equal(1, result.Removed);
        Assert.Null(_store.GetTrack(trackPath));
    }

    [Fact]
    public async Task Missing_or_unavailable_root_preserves_previously_indexed_tracks()
    {
        var missingRoot = Path.Combine(_root, "OfflineDrive");
        var trackPath = Path.Combine(missingRoot, "Music", "song.mp3");
        Seed(trackPath);

        var result = await Scan([missingRoot]);

        Assert.Equal(0, result.Removed);
        Assert.NotNull(_store.GetTrack(trackPath));
    }

    [Fact]
    public Task Interrupted_root_scan_preserves_previously_indexed_tracks()
    {
        var root = Directory.CreateDirectory(Path.Combine(_root, "InterruptedRoot")).FullName;
        var trackPath = Path.Combine(root, "song.mp3");
        Seed(trackPath);

        using var session = _store.BeginScan([root]);
        session.MarkRootCompleted(root, complete: false);
        var removed = session.Complete();

        Assert.Equal(0, removed);
        Assert.NotNull(_store.GetTrack(trackPath));
        return Task.CompletedTask;
    }

    [Fact]
    public async Task User_ignored_path_prunes_previously_indexed_tracks()
    {
        var root = Directory.CreateDirectory(Path.Combine(_root, "IgnoredRoot")).FullName;
        var ignored = Directory.CreateDirectory(Path.Combine(root, "PersonalArchive")).FullName;
        var trackPath = Path.Combine(ignored, "song.mp3");
        Seed(trackPath);

        var result = await Scan([root], [ignored]);

        Assert.Equal(1, result.Removed);
        Assert.Null(_store.GetTrack(trackPath));
    }

    [Fact]
    public void Overlapping_explicit_root_protects_unseen_rows_inside_a_default_exclusion()
    {
        var root = Directory.CreateDirectory(Path.Combine(_root, "OverlapRoot")).FullName;
        var excluded = Directory.CreateDirectory(Path.Combine(root, "Games")).FullName;
        var nestedRoot = Directory.CreateDirectory(Path.Combine(excluded, "SelectedLibrary")).FullName;
        var trackPath = Path.Combine(nestedRoot, "not-seen-this-run.mp3");
        Seed(trackPath);

        using var session = _store.BeginScan([root, nestedRoot]);
        session.MarkExcludedPath(excluded, ScanExcludedPathKind.DefaultExcluded);
        session.MarkRootCompleted(root, complete: true);
        session.MarkRootCompleted(nestedRoot, complete: true);

        Assert.Equal(0, session.Complete());
        Assert.NotNull(_store.GetTrack(trackPath));
    }

    private async Task<IndexResult> Scan(IEnumerable<string> roots, IEnumerable<string>? ignored = null)
    {
        using var control = new ScanControl();
        return await new LibraryIndexer(_store, Path.Combine(_root, "artwork"), new LocalAppLog(Path.Combine(_root, "Logs")))
            .ScanAsync(roots, ignored ?? [], control);
    }

    private void Seed(string path)
    {
        _store.UpsertTrack(new Track(path, Path.GetFileNameWithoutExtension(path), "Artist", "Album", "Artist", "", 0, 0,
            TimeSpan.FromMinutes(1), 123, DateTime.UtcNow, DateTime.UtcNow));
    }

    public void Dispose()
    {
        SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, recursive: true);
    }
}
