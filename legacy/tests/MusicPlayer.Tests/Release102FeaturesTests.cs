using System.Text.Json;
using Microsoft.Data.Sqlite;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class Release102FeaturesTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-release102-" + Guid.NewGuid().ToString("N"));
    private readonly LibraryStore _store;
    private readonly LibraryQueryService _queries;

    public Release102FeaturesTests()
    {
        Directory.CreateDirectory(_root);
        _store = new LibraryStore(Path.Combine(_root, "library.db"), new LocalAppLog(Path.Combine(_root, "Logs")));
        _queries = new LibraryQueryService(_store, _root);
    }

    [Fact]
    public void Folders_page_only_counts_and_displays_direct_children_leaving_subfolders_separate()
    {
        var musicDir = Path.Combine(_root, "Music");
        var subDir = Path.Combine(musicDir, "Kendrick");
        Directory.CreateDirectory(musicDir);
        Directory.CreateDirectory(subDir);

        var rootSongPath = Path.Combine(musicDir, "root.flac");
        var subSongPath = Path.Combine(subDir, "sub.flac");
        File.WriteAllBytes(rootSongPath, [1, 2, 3, 4]);
        File.WriteAllBytes(subSongPath, [5, 6, 7, 8]);

        var now = DateTime.UtcNow;
        var rootTrack = new Track(rootSongPath, "Root Song", "Artist", "Album", "Artist", "Genre", 0, 1, TimeSpan.FromSeconds(3), 4, File.GetLastWriteTimeUtc(rootSongPath), now);
        var subTrack = new Track(subSongPath, "Sub Song", "Artist", "Album", "Artist", "Genre", 0, 2, TimeSpan.FromSeconds(4), 4, File.GetLastWriteTimeUtc(subSongPath), now);

        _store.UpsertTracks([rootTrack, subTrack]);

        // Direct folder count: musicDir should only have 1 (rootSong), NOT 2!
        Assert.Equal(1, _store.CountTracks(groupColumn: "folder", groupValue: musicDir));
        Assert.Equal(1, _store.CountTracks(groupColumn: "folder", groupValue: subDir));

        // Group tracks for musicDir should contain ONLY rootSong
        var musicTracks = _queries.GroupTrackPaths("folder", musicDir);
        Assert.Single(musicTracks);
        Assert.Equal(Path.GetFullPath(rootSongPath), musicTracks[0]);

        // Sub folder tracks should contain ONLY subSong
        var subTracks = _queries.GroupTrackPaths("folder", subDir);
        Assert.Single(subTracks);
        Assert.Equal(Path.GetFullPath(subSongPath), subTracks[0]);

        // FolderData DTOs must provide id: path
        using var request = JsonDocument.Parse(JsonSerializer.Serialize(new { offset = 0, pageSize = 50 }));
        var data = JsonSerializer.SerializeToElement(_queries.FolderData(request.RootElement, new { }));
        var folders = data.GetProperty("folders").EnumerateArray().ToArray();
        Assert.Equal(2, folders.Length);
        foreach (var f in folders)
        {
            Assert.True(f.TryGetProperty("id", out var idProp));
            Assert.False(string.IsNullOrWhiteSpace(idProp.GetString()));
            Assert.Equal(f.GetProperty("path").GetString(), idProp.GetString());
        }
    }

    [Fact]
    public void Lyrics_parser_strips_section_headers_without_breaking_sync()
    {
        var lrc = """
            [ti:Test]
            [ar:Test]
            [Verse 1]
            [00:10.50] [Verse 1] First line of the song
            [Chorus]
            [00:20.00][Chorus] Second line of the song
            [00:30.00]Third line without section
            """;

        var doc = Lyrics.Parse(lrc);
        Assert.Equal(3, doc.Lines.Count);
        Assert.Equal("First line of the song", doc.Lines[0].Text);
        Assert.Equal(TimeSpan.FromSeconds(10.5), doc.Lines[0].Time);
        Assert.Equal("Second line of the song", doc.Lines[1].Text);
        Assert.Equal(TimeSpan.FromSeconds(20.0), doc.Lines[1].Time);
        Assert.Equal("Third line without section", doc.Lines[2].Text);
        Assert.Equal(TimeSpan.FromSeconds(30.0), doc.Lines[2].Time);

        var plain = Lyrics.PlainText(lrc);
        Assert.DoesNotContain("[Verse", plain);
        Assert.DoesNotContain("[Chorus", plain);
        Assert.Contains("First line of the song", plain);
        Assert.Contains("Second line of the song", plain);
    }

    [Fact]
    public void Lyrics_sidecar_can_be_deleted()
    {
        var audioFile = Path.Combine(_root, "test.mp3");
        File.WriteAllBytes(audioFile, [1, 2, 3]);
        var sidecar = LyricsFiles.SidecarPath(audioFile);
        File.WriteAllText(sidecar, "[00:01.00]Hello");
        Assert.True(File.Exists(sidecar));

        LyricsFiles.DeleteSidecar(audioFile);
        Assert.False(File.Exists(sidecar));
    }

    [Fact]
    public void Cold_scan_session_detects_unchanged_files()
    {
        var audioFile = Path.Combine(_root, "scantest.mp3");
        File.WriteAllBytes(audioFile, [1, 2, 3, 4]);
        var now = DateTime.UtcNow;
        var info = new FileInfo(audioFile);
        var track = new Track(audioFile, "Scan Track", "Artist", "Album", "Artist", "Genre", 0, 1, TimeSpan.FromSeconds(5), info.Length, info.LastWriteTimeUtc, now);
        _store.UpsertTrack(track);

        using var session = _store.BeginScan([_root]);
        Assert.True(session.IsUnchanged(audioFile, info.Length, info.LastWriteTimeUtc, Path.Combine(_root, "art")));
    }

    [Fact]
    public void Smart_autoplay_recommends_similar_genre_and_artist_tracks_excluding_queue()
    {
        var now = DateTime.UtcNow;
        var trackHipHop1 = new Track(Path.Combine(_root, "hiphop1.mp3"), "HipHop 1", "Kendrick", "Album A", "Kendrick", "Hip-Hop", 2020, 1, TimeSpan.FromSeconds(180), 1000, now, now, Favorite: true, PlayCount: 50);
        var trackHipHop2 = new Track(Path.Combine(_root, "hiphop2.mp3"), "HipHop 2", "J. Cole", "Album B", "J. Cole", "Hip-Hop", 2021, 2, TimeSpan.FromSeconds(200), 1000, now, now, Favorite: false, PlayCount: 20);
        var trackRock = new Track(Path.Combine(_root, "rock.mp3"), "Rock Track", "Queen", "Album C", "Queen", "Rock", 1980, 1, TimeSpan.FromSeconds(210), 1000, now, now, Favorite: false, PlayCount: 2);
        File.WriteAllBytes(trackHipHop1.Path, [1]);
        File.WriteAllBytes(trackHipHop2.Path, [2]);
        File.WriteAllBytes(trackRock.Path, [3]);

        _store.UpsertTracks([trackHipHop1, trackHipHop2, trackRock]);

        var recommendations = _store.GetSmartAutoplayTracks(trackHipHop1.Path, "Hip-Hop", "Kendrick", [trackHipHop1.Path], 5);
        Assert.NotEmpty(recommendations);
        Assert.DoesNotContain(trackHipHop1.Path, recommendations);
        Assert.Contains(trackHipHop2.Path, recommendations);
    }

    [Fact]
    public void AutoplaySimilar_setting_defaults_to_true_and_persists()
    {
        var settingsService = new PlayerSettingsService(_store);
        var settings = settingsService.ProjectWebSettings("Dark", null, null);
        var json = JsonSerializer.Serialize(settings);
        using var doc = JsonDocument.Parse(json);
        Assert.True(doc.RootElement.GetProperty("autoplaySimilar").GetBoolean());

        using var payload = JsonDocument.Parse(JsonSerializer.Serialize(new
        {
            settings = new { autoplaySimilar = false }
        }));
        settingsService.PersistWebSettings(payload.RootElement);

        var updated = settingsService.ProjectWebSettings("Dark", null, null);
        using var updatedDoc = JsonDocument.Parse(JsonSerializer.Serialize(updated));
        Assert.False(updatedDoc.RootElement.GetProperty("autoplaySimilar").GetBoolean());
    }

    public void Dispose()
    {
        SqliteConnection.ClearAllPools();
        if (Directory.Exists(_root))
        {
            try { Directory.Delete(_root, true); } catch { }
        }
    }
}
