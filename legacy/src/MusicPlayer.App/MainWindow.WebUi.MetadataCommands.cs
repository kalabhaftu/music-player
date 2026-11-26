using System.Globalization;
using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task<object?> HandleWebUiMetadataCommandAsync(string name, JsonElement payload)
    {
        switch (name)
        {
            case "saveLyrics": return await SaveLyricsAsync(payload);
            case "searchLyrics": return await SearchLyricsAsync(payload);
            case "removeLyrics": return await RemoveLyricsAsync(payload);
            case "saveTags": await SaveTagsAsync(payload); return null;
            case "restoreTags": await RestoreTagsAsync(payload); return null;
            case "pickArtwork": return await PickArtworkAsync();
            default: throw new InvalidOperationException("This Music Player command is not available in the metadata handler.");
        }
    }

    private async Task<object> SaveLyricsAsync(JsonElement payload)
    {
        var track = ResolveRequestedLyricsTrack(payload) ?? throw new InvalidOperationException("Choose a track before editing lyrics.");
        EnsureMetadataTargetAvailable(track);
        var text = String(payload, "text"); var mode = String(payload, "mode"); var offset = Int(payload, "offsetMilliseconds");
        var storage = await _trackMetadata.SaveLyricsAsync(track.Path, text, mode == "embed", offset);
        await RefreshEditedTrackAsync(track.Path); PublishLibraryChanged();
        return new { storage };
    }

    private async Task<object> SearchLyricsAsync(JsonElement payload)
    {
        var track = ResolveRequestedLyricsTrack(payload);
        var title = String(payload, "title");
        var artist = String(payload, "artist");
        if (string.IsNullOrWhiteSpace(title) && track is not null) title = track.Title;
        if (string.IsNullOrWhiteSpace(artist) && track is not null) artist = track.Artist;
        if (string.IsNullOrWhiteSpace(title) && string.IsNullOrWhiteSpace(artist))
            return new { results = Array.Empty<object>() };
        try
        {
            var results = await _trackMetadata.SearchLyricsAsync(title, artist);
            return new { results };
        }
        catch (LyricsRateLimitException ex)
        {
            return new { results = Array.Empty<object>(), rateLimited = true, retryAfterMilliseconds = Math.Ceiling(ex.RetryAfter.TotalMilliseconds) };
        }
    }

    private async Task<object> RemoveLyricsAsync(JsonElement payload)
    {
        var track = ResolveRequestedLyricsTrack(payload) ?? throw new InvalidOperationException("Choose a track before removing lyrics.");
        EnsureMetadataTargetAvailable(track);
        await _trackMetadata.RemoveLyricsAsync(track.Path);
        _store.UpdateTrack(track.Path, "has_lyrics", 0);
        await RefreshEditedTrackAsync(track.Path);
        PublishLibraryChanged();
        return new { success = true };
    }

    private async Task SaveTagsAsync(JsonElement payload)
    {
        var track = RequireTrack(payload, "id");
        EnsureMetadataTargetAvailable(track);
        var tags = payload.TryGetProperty("tags", out var value) && value.ValueKind == JsonValueKind.Object ? value : default;
        string? Optional(string name) => tags.ValueKind == JsonValueKind.Object && tags.TryGetProperty(name, out var item) && item.ValueKind != JsonValueKind.Null ? item.ToString() : null;
        var custom = JsonDictionary(tags, "customFields"); var additional = JsonDictionary(tags, "additionalFields");
        uint? Year() => uint.TryParse(Optional("year"), NumberStyles.Integer, CultureInfo.InvariantCulture, out var year) ? year : null;
        uint? TrackNumber() => uint.TryParse(Optional("trackNumber"), NumberStyles.Integer, CultureInfo.InvariantCulture, out var number) ? number : null;
        var artworkToken = String(payload, "artworkToken");
        var artPath = !string.IsNullOrWhiteSpace(artworkToken) && _webArtworkPaths.TryGetValue(artworkToken, out var selectedArtwork) ? selectedArtwork : null;
        var edit = new TagEdit(Optional("title"), Optional("artist"), Optional("album"), Optional("albumArtist"), Optional("genre"), Year(), TrackNumber(),
            ArtworkPath: string.IsNullOrWhiteSpace(artPath) ? null : artPath, CustomFields: custom, AdditionalFields: additional);
        await _trackMetadata.SaveTagsAsync(track.Path, edit);
        await RefreshEditedTrackAsync(track.Path); PublishLibraryChanged();
    }

    private async Task RestoreTagsAsync(JsonElement payload)
    {
        var track = RequireTrack(payload, "id");
        EnsureMetadataTargetAvailable(track);
        var backups = _trackMetadata.ListTagBackups(track.Path);
        if (backups.Count == 0) throw new InvalidOperationException("No saved tag backups are available for this track.");
        var selectedId = String(payload, "backupId");
        var backup = string.IsNullOrWhiteSpace(selectedId) ? backups[0] : backups.FirstOrDefault(item => _libraryQueries.OpaqueId(item.BackupPath) == selectedId)
            ?? throw new InvalidOperationException("That tag backup is no longer available.");
        await _trackMetadata.RestoreTagsAsync(backup); await RefreshEditedTrackAsync(track.Path); PublishLibraryChanged();
    }

    private void EnsureMetadataTargetAvailable(Track track)
    {
        if (File.Exists(track.Path)) return;
        _trackAvailability.MarkUnavailable(track.Path);
        _webBridge?.SendEvent("trackAvailabilityChanged", new { id = _libraryQueries.TrackId(track.Path), fileUnavailable = true });
        PublishLibraryChanged();
        throw new FileNotFoundException("This audio file is unavailable. Reconnect its drive or restore the file before editing its tags or lyrics.");
    }

    private static IReadOnlyDictionary<string, string> JsonDictionary(JsonElement root, string name)
    {
        if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty(name, out var map) || map.ValueKind != JsonValueKind.Object) return new Dictionary<string, string>();
        return map.EnumerateObject().ToDictionary(item => item.Name, item => item.Value.ToString(), StringComparer.OrdinalIgnoreCase);
    }
}
