namespace MusicPlayer.Core;

public sealed record Track(
    string Path,
    string Title,
    string Artist,
    string Album,
    string AlbumArtist,
    string Genre,
    uint Year,
    uint TrackNumber,
    TimeSpan Duration,
    long FileSize,
    DateTime ModifiedUtc,
    DateTime AddedUtc,
    bool Favorite = false,
    int Rating = 0,
    int PlayCount = 0,
    DateTime? LastPlayedUtc = null,
    string? ArtworkPath = null,
    bool HasLyrics = false)
{
    public string RatingDisplay => Rating == 0 ? "☆" : new string('★', Rating);
    public string FavoriteGlyph => Favorite ? "♥" : "♡";
    public string FavoriteIconKind => Favorite ? "heart-filled" : "heart";
    public string YearDisplay => Year == 0 ? "" : Year.ToString(System.Globalization.CultureInfo.InvariantCulture);
    public string DurationDisplay
    {
        get
        {
            var totalSeconds = Math.Max(0L, (long)Duration.TotalSeconds);
            var minutes = totalSeconds / 60;
            return minutes >= 60
                ? $"{minutes / 60}:{minutes % 60:00}:{totalSeconds % 60:00}"
                : $"{minutes}:{totalSeconds % 60:00}";
        }
    }
}

public sealed record LibraryGroup(string Name, int TrackCount, string? ArtworkPath, string? Artist, uint Year = 0);
public sealed record LibraryStats(int TotalTracks, long TotalBytes);

public sealed record ScanProgress(int FilesFound, int DirectoriesVisited, string CurrentPath, int IndexedTracks = -1);
public sealed record LyricsLine(TimeSpan Time, string Text);
public sealed record LyricsDocument(IReadOnlyList<LyricsLine> Lines, TimeSpan Offset, IReadOnlyDictionary<string, string>? Metadata = null)
{
    public string At(TimeSpan position)
    {
        var time = position - Offset;
        for (var i = Lines.Count - 1; i >= 0; i--)
            if (Lines[i].Time <= time) return Lines[i].Text;
        return string.Empty;
    }
}

public sealed record Playlist(string Id, string Name, IReadOnlyList<string> Paths, DateTime CreatedUtc);
public sealed record PlaylistSummary(string Id, string Name, DateTime CreatedUtc, int TrackCount);
public sealed record PlaylistEntry(int Position, string Path, Track? Track);
public sealed record PlaybackSession(
    string? TrackPath,
    long PositionMilliseconds,
    IReadOnlyList<string> Queue,
    bool Shuffle,
    string RepeatMode,
    long? RepeatAMilliseconds = null,
    long? RepeatBMilliseconds = null,
    int QueueIndex = -1);
public sealed record TagEdit(
    string? Title = null,
    string? Artist = null,
    string? Album = null,
    string? AlbumArtist = null,
    string? Genre = null,
    uint? Year = null,
    uint? TrackNumber = null,
    string? Lyrics = null,
    string? ArtworkPath = null,
    IReadOnlyDictionary<string, string>? CustomFields = null,
    IReadOnlyDictionary<string, string>? AdditionalFields = null);

public sealed record TagBackup(string OriginalPath, string BackupPath, DateTime CreatedUtc);
