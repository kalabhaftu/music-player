using System.Text;
using System.Text.Json.Serialization;
using System.Text.Json;
using TagLib;

namespace MusicPlayer.Core;

public sealed record LyricsSearchResult(
    [property: JsonPropertyName("trackName")] string TrackName,
    [property: JsonPropertyName("artistName")] string ArtistName,
    [property: JsonPropertyName("albumName")] string? AlbumName,
    [property: JsonPropertyName("syncedLyrics")] string? SyncedLyrics,
    [property: JsonPropertyName("plainLyrics")] string? PlainLyrics,
    [property: JsonPropertyName("duration")] double? Duration = null);

public sealed record LyricsReadResult(string Text, string Source);

public static class LyricsFiles
{
    private static readonly LrclibSearchClient LrclibClient = new(CreateLrclibClient());

    public static string SidecarPath(string trackPath) => Path.ChangeExtension(trackPath, ".lrc");

    public static bool HasUsableSidecar(string trackPath)
    {
        try
        {
            var sidecar = SidecarPath(trackPath);
            return System.IO.File.Exists(sidecar) && Lyrics.HasUsableContent(System.IO.File.ReadAllText(sidecar, Encoding.UTF8));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.Security.SecurityException)
        {
            LocalAppLog.Shared.Warning("lyrics-reader", $"Could not inspect lyrics sidecar for '{trackPath}'.", ex);
            return false;
        }
    }

    public static bool HasEmbeddedLyrics(TagLib.File media) => Lyrics.HasUsableContent(ReadEmbeddedLyrics(media));

    public static string ReadRaw(string trackPath) => Read(trackPath).Text;

    public static LyricsReadResult Read(string trackPath)
    {
        var sidecar = SidecarPath(trackPath);
        string? malformedSidecar = null;
        try
        {
            if (System.IO.File.Exists(sidecar))
            {
                var text = System.IO.File.ReadAllText(sidecar, Encoding.UTF8);
                if (Lyrics.HasUsableContent(text))
                {
                    if (!Lyrics.IsMalformedTimedText(text)) return new(text, "sidecar");
                    malformedSidecar = text;
                }
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.Security.SecurityException)
        { LocalAppLog.Shared.Warning("lyrics-reader", $"Could not read lyrics sidecar '{sidecar}'. Trying embedded lyrics instead.", ex); }
        try
        {
            using var media = AudioTags.Open(trackPath);
            var embedded = ReadEmbeddedLyrics(media);
            if (Lyrics.HasUsableContent(embedded)) return new(embedded, "embedded");
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or TagLib.CorruptFileException or TagLib.UnsupportedFormatException or NotSupportedException or ArgumentException or System.Security.SecurityException)
        { LocalAppLog.Shared.Warning("lyrics-reader", $"Could not read embedded lyrics in '{trackPath}'.", ex); }
        if (malformedSidecar is not null) return new(malformedSidecar, "sidecar");
        return new(string.Empty, "none");
    }

    private static string ReadEmbeddedLyrics(TagLib.File media)
    {
        // Tag.Lyrics covers the common unsynchronized lyric fields (for example
        // ID3 USLT and Xiph LYRICS). ID3 also has a separate SYLT frame, which
        // Tag.Lyrics does not project; preserve its timestamps when available.
        if (media.GetTag(TagTypes.Id3v2) is TagLib.Id3v2.Tag id3)
        {
            var synchronized = TagLib.Id3v2.SynchronisedLyricsFrame.GetPreferred(
                id3, string.Empty, "eng", TagLib.Id3v2.SynchedTextType.Lyrics);
            if (synchronized?.Format == TagLib.Id3v2.TimestampFormat.AbsoluteMilliseconds)
            {
                var syncedLines = synchronized.Text
                    .Where(item => item.Time >= 0 && item.Time <= TimeSpan.MaxValue.Ticks / TimeSpan.TicksPerMillisecond
                        && !string.IsNullOrWhiteSpace(item.Text))
                    .Select(item => new LyricsLine(TimeSpan.FromTicks(item.Time * TimeSpan.TicksPerMillisecond), item.Text))
                    .OrderBy(line => line.Time)
                    .ToArray();
                if (syncedLines.Length > 0)
                    return Lyrics.Format(new LyricsDocument(syncedLines, TimeSpan.Zero));
            }
        }

        return media.Tag.Lyrics ?? string.Empty;
    }

    public static void SaveSidecar(string trackPath, string lyrics)
    {
        var destination = SidecarPath(trackPath);
        using var pathLock = new FilePathLock(destination);
        var temporary = destination + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            System.IO.File.WriteAllText(temporary, lyrics, new UTF8Encoding(false));
            System.IO.File.Move(temporary, destination, true);
        }
        finally { if (System.IO.File.Exists(temporary)) System.IO.File.Delete(temporary); }
    }

    public static async Task<IReadOnlyList<LyricsSearchResult>> SearchLrclibAsync(string title, string artist, CancellationToken cancellationToken = default)
    {
        var clean = LyricsSearchQuery.Normalize(title, artist);
        var results = await LrclibClient.SearchAsync(clean.Title.Length > 0 ? clean.Title : title, clean.Artist, cancellationToken).ConfigureAwait(false);
        if (results.Count > 0 || (clean.Title == title && clean.Artist == artist)) return results;
        return await LrclibClient.SearchAsync(title, artist, cancellationToken).ConfigureAwait(false);
    }

    private static HttpClient CreateLrclibClient()
    {
        var client = new HttpClient { Timeout = TimeSpan.FromSeconds(45) };
        client.DefaultRequestHeaders.UserAgent.ParseAdd("MusicPlayer/1.0 (+https://github.com/kalabhaftu/music-player)");
        return client;
    }
}

public sealed record TrackDetails(string Path, string Container, TimeSpan Duration, int BitrateKbps, int SampleRateHz, int BitsPerSample, long FileSize, DateTime ModifiedUtc);

public static class TrackInformation
{
    public static TrackDetails Read(string path)
    {
        var info = new FileInfo(path);
        using var media = AudioTags.Open(path);
        return new(Path.GetFullPath(path), media.MimeType, media.Properties.Duration, media.Properties.AudioBitrate,
            media.Properties.AudioSampleRate, media.Properties.BitsPerSample, info.Length, info.LastWriteTimeUtc);
    }
}
