using System.Globalization;
using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private object PlaylistTrackPage(JsonElement payload)
    {
        var id = PlaylistId(payload);
        var offset = Math.Max(0, Int(payload, "offset"));
        var size = Math.Clamp(Int(payload, "pageSize", 100), 1, 200);
        var search = String(payload, "search");
        var entries = _store.GetPlaylistEntriesPage(id, search, offset, size);
        var tracks = entries.Select(entry => entry.Track is null
            ? new { id = _libraryQueries.TrackId(entry.Path), title = Path.GetFileNameWithoutExtension(entry.Path), artist = "Unavailable", album = "", albumArtist = "", genre = "", year = 0u, trackNumber = 0u, durationSeconds = 0d, addedDisplay = "Unavailable", lastPlayedDisplay = "", favorite = false, rating = 0, playCount = 0, artworkUrl = (string?)null, position = entry.Position, unavailable = true, fileUnavailable = _libraryQueries.IsTrackUnavailable(entry.Path) }
            : new { id = _libraryQueries.TrackId(entry.Path), title = entry.Track.Title, artist = entry.Track.Artist, album = entry.Track.Album, albumArtist = entry.Track.AlbumArtist, genre = entry.Track.Genre, year = entry.Track.Year, trackNumber = entry.Track.TrackNumber, durationSeconds = entry.Track.Duration.TotalSeconds, addedDisplay = entry.Track.AddedUtc.ToLocalTime().ToString("d", CultureInfo.CurrentCulture), lastPlayedDisplay = entry.Track.LastPlayedUtc?.ToLocalTime().ToString("d", CultureInfo.CurrentCulture) ?? "", favorite = entry.Track.Favorite, rating = entry.Track.Rating, playCount = entry.Track.PlayCount, artworkUrl = _libraryQueries.ArtworkUrl(entry.Track.ArtworkPath), position = entry.Position, unavailable = false, fileUnavailable = _libraryQueries.IsTrackUnavailable(entry.Path) }).ToArray();
        return new { tracks, totalCount = _store.CountPlaylistEntries(id, search) };
    }

    private object[] QueueDtos(int offset, int pageSize) => QueueDtos(_queue.Skip(offset).Take(pageSize).ToArray());
    private object[] QueueDtos(string[] paths)
    {
        var tracks = _store.GetTracksByPaths(paths);
        return paths.Select(path => tracks.TryGetValue(path, out var track) ? (object)_libraryQueries.TrackDto(track) : new { id = _libraryQueries.TrackId(path), title = Path.GetFileNameWithoutExtension(path), artist = "Unavailable", album = "", albumArtist = "", genre = "", year = 0u, trackNumber = 0u, durationSeconds = 0d, addedDisplay = "Unavailable", lastPlayedDisplay = "", favorite = false, rating = 0, playCount = 0, artworkUrl = (string?)null, unavailable = true, fileUnavailable = _libraryQueries.IsTrackUnavailable(path) }).ToArray();
    }

    private async Task<object> QueuePageAsync(JsonElement payload)
    {
        var offset = Math.Clamp(Int(payload, "offset"), 0, _queue.Count);
        var pageSize = Math.Clamp(Int(payload, "pageSize", 100), 1, 200);
        var paths = _queue.Skip(offset).Take(pageSize).ToArray();
        var totalCount = _queue.Count;
        var queueIndex = WebQueueIndex;
        var entries = await Task.Run(() => QueueDtos(paths));
        return new
        {
            entries, totalCount, queueIndex
        };
    }

    private object LyricsData(Track? track)
    {
        if (track is null) return new { track = (object?)null, raw = "", plainText = "", source = "none", lines = Array.Empty<object>(), offsetMilliseconds = 0 };
        var reading = LyricsFiles.Read(track.Path);
        var raw = reading.Text;
        var doc = Lyrics.Parse(raw);
        var plainText = doc.Lines.Count == 0
            ? Lyrics.PlainText(raw)
            : "";
        return new
        {
            track = _libraryQueries.TrackDto(track, true), raw, plainText, source = reading.Source,
            offsetMilliseconds = (long)doc.Offset.TotalMilliseconds,
            lines = doc.Lines.Select(line => new { seconds = (line.Time + doc.Offset).TotalSeconds, text = line.Text }).ToArray()
        };
    }

    private async Task<object> TrackDetailsDataAsync(Track? track)
    {
        if (track is null) throw new KeyNotFoundException("That track is no longer in the indexed library.");
        var trackDto = _libraryQueries.TrackDto(track, true);
        var details = await Task.Run(() => TrackInformation.Read(track.Path));
        return new
        {
            track = trackDto, path = details.Path, container = details.Container,
            durationSeconds = details.Duration.TotalSeconds, bitrateKbps = details.BitrateKbps, sampleRateHz = details.SampleRateHz,
            bitsPerSample = details.BitsPerSample, fileSize = details.FileSize,
            modifiedDisplay = details.ModifiedUtc.ToLocalTime().ToString("g", CultureInfo.CurrentCulture)
        };
    }

    private object TagsData(Track? track)
    {
        if (track is null) throw new KeyNotFoundException("That track is no longer in the indexed library.");
        var editor = new TagEditor(Path.Combine(_appData, "TagBackups"));
        return new
        {
            track = _libraryQueries.TrackDto(track, true), customFormat = TagEditor.CustomFieldFormat(track.Path),
            customFields = TagEditor.ReadCustomFields(track.Path), additionalFields = TagEditor.ReadAdditionalStandardFields(track.Path),
            backups = editor.ListBackups(track.Path).Select(backup => new { id = _libraryQueries.OpaqueId(backup.BackupPath), created = backup.CreatedUtc.ToLocalTime().ToString("g", CultureInfo.CurrentCulture) }).ToArray()
        };
    }

    private Track? TrackFrom(JsonElement payload, string key) => _libraryQueries.ResolveTrack(String(payload, key));

    private Track? ResolveRequestedLyricsTrack(JsonElement payload) =>
        string.IsNullOrWhiteSpace(String(payload, "id")) ? _playback.CurrentTrack : RequireTrack(payload, "id");

    private Track RequireTrack(JsonElement payload, string key) =>
        TrackFrom(payload, key) ?? throw new KeyNotFoundException("That track is no longer in the indexed library.");

    private Track? OptionalTrack(JsonElement payload, string key) =>
        string.IsNullOrWhiteSpace(String(payload, key)) ? null : TrackFrom(payload, key);

    private object DuplicateFiles(JsonElement payload)
    {
        var id = String(payload, "id");
        var offset = Math.Max(0, Int(payload, "offset"));
        var pageSize = Math.Clamp(Int(payload, "pageSize", 100), 1, 200);
        var page = _store.GetDuplicateTracksPage(id, offset, pageSize);
        return new { files = page.Tracks.Select(track => new { path = track.Path, title = track.Title, artist = track.Artist }).ToArray(), offset, totalCount = page.TotalCount };
    }

    private async Task<object> DuplicatePageAsync(JsonElement payload)
    {
        var offset = Math.Max(0, Int(payload, "offset"));
        var pageSize = Math.Clamp(Int(payload, "pageSize", 100), 1, 200);
        var search = String(payload, "search");
        var sort = Enum.TryParse<TrackSort>(String(payload, "sort"), true, out var parsedSort) ? parsedSort : TrackSort.Title;
        var descending = Boolean(payload, "descending");
        var page = await Task.Run(() => _store.GetDuplicateGroupsPage(search, sort, descending, offset: offset, pageSize: pageSize));
        return new { groups = page.Groups.Select(group => new { id = group.Sha256, title = group.Representative.Title, artist = group.Representative.Artist, copyCount = group.CopyCount }).ToArray(), totalCount = page.TotalCount };
    }
}
