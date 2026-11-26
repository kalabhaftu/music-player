using System.Text.Json;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class LibraryQueueContextTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-context-" + Guid.NewGuid().ToString("N"));

    [Theory]
    [InlineData("Songs", "does-not-match", 2)]
    [InlineData("Search", "Alpha", 1)]
    [InlineData("Favorites", "does-not-match", 1)]
    public void Displayed_rows_and_queue_use_identical_search_and_filter_context(string view, string search, int count)
    {
        var store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
        var now = DateTime.UtcNow;
        store.UpsertTracks(new[] { "Alpha", "Beta" }.Select((title, index) => new Track(Path.Combine(_root, title + ".wav"), title,
            "Artist", "Album", "Artist", "", 0, 0, TimeSpan.FromSeconds(3), 100, now, now, Favorite: index == 0)));
        var queries = new LibraryQueryService(store, _root);
        using var request = JsonDocument.Parse(JsonSerializer.Serialize(new { view, search }));
        var displayed = JsonSerializer.SerializeToElement(queries.TrackPage(request.RootElement));
        var displayedPaths = displayed.GetProperty("tracks").EnumerateArray()
            .Select(row => queries.ResolveTrackPath(row.GetProperty("id").GetString()) ?? throw new InvalidDataException("A displayed row lost its handle.")).ToArray();
        Assert.Equal(count, displayedPaths.Length);
        Assert.Equal(displayedPaths, queries.CurrentTrackPaths());
    }

    public void Dispose()
    {
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root)) Directory.Delete(_root, true);
    }
}
