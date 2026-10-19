using System.Text.Json;
using Microsoft.Data.Sqlite;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class LibraryBrowsingTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-browse-" + Guid.NewGuid().ToString("N"));
    private readonly LibraryStore _store;
    private readonly LibraryQueryService _queries;

    public LibraryBrowsingTests()
    {
        Directory.CreateDirectory(_root);
        _store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
        _queries = new LibraryQueryService(_store, _root);
    }

    private Track Track(string file, string album, string artist = "Artist", string genre = "Genre")
    {
        var path = Path.Combine(_root, file + ".flac");
        File.WriteAllBytes(path, [1, 2, 3, 4]);
        var now = DateTime.UtcNow;
        return new(path, file, artist, album, artist, genre, 0, 1, TimeSpan.FromSeconds(3), 4, File.GetLastWriteTimeUtc(path), now);
    }

    [Theory]
    [InlineData("Album", "album", "Album")]
    [InlineData("Artist", "artist", "Artist")]
    [InlineData("Genre", "genre", "Genre")]
    [InlineData("Folder", "folder", null)]
    public void Duplicate_hiding_applies_to_collection_tracks_and_playback_sources(string view, string column, string? value)
    {
        _store.UpsertTracks([Track("first", "Album"), Track("copy", "Album")]);
        _store.PrepareExactFingerprints();
        _store.SetSetting("hide-exact-duplicates", "true");
        Assert.Equal(1, _store.CountTracks(hideExactDuplicates: true));
        value ??= _root;
        using var request = JsonDocument.Parse(JsonSerializer.Serialize(new { view, groupColumn = column, groupValue = value }));
        var page = JsonSerializer.SerializeToElement(_queries.TrackPage(request.RootElement));
        Assert.Equal(1, page.GetProperty("totalCount").GetInt32());
        var paths = page.GetProperty("tracks").EnumerateArray()
            .Select(row => _queries.ResolveTrackPath(row.GetProperty("id").GetString())
                ?? throw new InvalidDataException("A displayed track lost its path.")).ToArray();
        Assert.Single(paths);
        Assert.Equal(paths, _queries.CurrentTrackPaths());
        Assert.Single(_queries.GroupTrackPaths(column, value));
        Assert.Equal(1, _store.CountGroups("album"));
        Assert.Equal(1, Assert.Single(_store.GetGroupsPage("album", hideExactDuplicates: true)).TrackCount);
    }

    [Theory]
    [InlineData(true, 1)]
    [InlineData(false, 2)]
    public void The_duplicate_setting_still_controls_the_songs_list(bool hide, int expected)
    {
        _store.UpsertTracks([Track("first", "Album"), Track("copy", "Album")]);
        _store.PrepareExactFingerprints();
        _store.SetSetting("hide-exact-duplicates", hide ? "true" : "false");
        using var request = JsonDocument.Parse("{\"view\":\"Songs\"}");
        var page = JsonSerializer.SerializeToElement(_queries.TrackPage(request.RootElement));
        Assert.Equal(expected, page.GetProperty("totalCount").GetInt32());
        Assert.Equal(expected, _queries.CurrentTrackPaths().Count);
    }

    [Theory]
    [InlineData("album")]
    [InlineData("artist")]
    [InlineData("genre")]
    public void Group_counts_and_all_pages_include_the_same_visible_labels(string column)
    {
        var names = new[] { "", "   ", " null ", "undefined", "a", "A", "Beta" };
        _store.UpsertTracks(names.Select((name, index) => Track("group-" + index, name, name, name)));
        var expected = new[] { "A", "a", "Beta" };
        Assert.Equal(3, _store.CountGroups(column));
        Assert.Equal(expected, _store.GetGroups(column));
        Assert.Equal(expected, Enumerable.Range(0, 3).SelectMany(offset => _store.GetGroupsPage(column, offset: offset, pageSize: 1)).Select(group => group.Name));
        Assert.Equal(1, _store.CountGroups(column, "Beta"));
        Assert.Equal("Beta", Assert.Single(_store.GetGroupsPage(column, "Beta")).Name);
    }

    [Theory]
    [InlineData(29)]
    [InlineData(30)]
    [InlineData(31)]
    [InlineData(60)]
    public void Artist_album_pages_announce_more_only_when_another_album_exists(int count)
    {
        _store.UpsertTracks(Enumerable.Range(0, count).Select(index => Track("song-" + index, $"Album {index:D3}")));
        var first = JsonSerializer.SerializeToElement(_queries.ArtistAlbums("Artist", 0, 30));
        Assert.Equal(Math.Min(count, 30), first.GetProperty("groups").GetArrayLength());
        Assert.Equal(count > 30, first.GetProperty("hasMore").GetBoolean());
        if (count > 30)
        {
            var last = JsonSerializer.SerializeToElement(_queries.ArtistAlbums("Artist", 30, 30));
            Assert.Equal(count - 30, last.GetProperty("groups").GetArrayLength());
            Assert.False(last.GetProperty("hasMore").GetBoolean());
        }
    }

    public void Dispose()
    {
        SqliteConnection.ClearAllPools();
        Directory.Delete(_root, true);
    }
}
