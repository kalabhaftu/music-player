using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class LibraryStoreIntegrityTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-store-integrity-" + Guid.NewGuid().ToString("N"));
    private readonly LibraryStore _store;

    public LibraryStoreIntegrityTests()
    {
        Directory.CreateDirectory(_root);
        _store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
    }

    [Fact]
    public void Adding_to_a_missing_playlist_fails_even_when_no_paths_are_supplied()
    {
        Assert.Throws<KeyNotFoundException>(() => _store.AddToPlaylist("missing", []));
        Assert.Empty(_store.GetPlaylistSummaries());
    }

    [Fact]
    public void Removing_playlist_occurrences_rejects_negative_positions_and_ignores_stale_positions()
    {
        var playlist = _store.CreatePlaylist("Road", ["one.flac", "two.flac", "three.flac"]);

        Assert.Throws<ArgumentOutOfRangeException>(() => _store.RemoveFromPlaylist(playlist.Id, -1));
        _store.RemoveFromPlaylist(playlist.Id, 99);
        Assert.Equal(playlist.Paths, _store.GetPlaylists().Single().Paths);

        _store.RemoveFromPlaylist(playlist.Id, 1);
        Assert.Equal(new[] { Path.GetFullPath("one.flac"), Path.GetFullPath("three.flac") },
            _store.GetPlaylists().Single().Paths);
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, recursive: true);
    }
}
