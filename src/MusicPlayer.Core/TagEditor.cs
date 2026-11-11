using System.Security.Cryptography;
using System.Text;
using System.Globalization;
using System.Buffers;
using TagLib;

namespace MusicPlayer.Core;

public sealed class TagEditor(string backupDirectory)
{
    private const int BackupRetentionPerTrack = 5;
    private readonly string _backupDirectory = Path.GetFullPath(backupDirectory);
    private static readonly HashSet<string> CommonFields = new(StringComparer.OrdinalIgnoreCase)
    {
        "TITLE", "ARTIST", "ALBUM", "ALBUMARTIST", "ALBUM ARTIST", "GENRE", "DATE", "YEAR", "TRACK", "TRACKNUMBER", "LYRICS",
        "COVERART", "METADATA_BLOCK_PICTURE", "WM/Title", "WM/Author", "WM/AlbumTitle", "WM/AlbumArtist", "WM/Genre", "WM/Year",
        "WM/TrackNumber", "WM/Lyrics", "WM/Picture", "Album Artist", "Track",
        "TITLE_SORT", "TITLESORT", "SORTNAME", "SUBTITLE", "DESCRIPTION", "ARTIST_SORT", "ARTISTSORT", "SORTARTIST", "ARTIST_ROLE",
        "PERFORMER_ROLE", "ALBUM_ARTIST_SORT", "ALBUMARTISTSORT", "COMPOSER", "COMPOSERS", "COMPOSER_SORT", "ALBUM_SORT", "SORTALBUM",
        "COMMENT", "TRACKCOUNT", "TRACKTOTAL", "TOTALTRACKS", "DISC", "DISCNUMBER", "DISCCOUNT", "DISCTOTAL", "TOTALDISCS", "GROUPING",
        "CONTENTGROUP", "BPM", "BEATSPERMINUTE", "CONDUCTOR", "COPYRIGHT", "DATE_TAGGED", "MUSICBRAINZ_ARTIST_ID", "MUSICBRAINZ_RELEASE_GROUP_ID",
        "MUSICBRAINZ_RELEASE_ID", "MUSICBRAINZ_RELEASE_ARTIST_ID", "MUSICBRAINZ_TRACK_ID", "MUSICBRAINZ_DISC_ID", "MUSICIP_ID", "AMAZON_ID",
        "MUSICBRAINZ_RELEASE_STATUS", "MUSICBRAINZ_RELEASE_TYPE", "MUSICBRAINZ_RELEASE_COUNTRY", "REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_PEAK",
        "REPLAYGAIN_ALBUM_GAIN", "REPLAYGAIN_ALBUM_PEAK", "INITIAL_KEY", "REMIXED_BY", "PUBLISHER", "ISRC",
        "WM/TitleSort", "WM/SubTitle", "WM/Description", "WM/ArtistSort", "WM/AlbumArtistSort", "WM/Composers", "WM/ComposerSort",
        "WM/AlbumSort", "WM/Comment", "WM/TrackCount", "WM/PartOfSet", "WM/BeatsPerMinute", "WM/Conductor", "WM/Copyright", "WM/Publisher",
        "WM/ISRC", "WM/InitialKey", "WM/ModifiedBy", "MusicBrainz/Artist Id", "MusicBrainz/Release Group Id", "MusicBrainz/Album Id",
        "MusicBrainz/Album Artist Id", "MusicBrainz/Track Id", "MusicBrainz/Disc Id", "MusicBrainz/Album Status", "MusicBrainz/Album Type",
        "MusicBrainz/Album Release Country", "MusicBrainz Artist Id", "MusicBrainz Release Group Id", "MusicBrainz Album Id", "MusicBrainz Album Artist Id",
        "MusicBrainz Track Id", "MusicBrainz Disc Id", "MusicBrainz Album Status", "MusicBrainz Album Type", "MusicBrainz Album Release Country"
    };
    private static readonly HashSet<string> CommonId3Frames = new(StringComparer.OrdinalIgnoreCase)
        { "TIT2", "TPE1", "TALB", "TPE2", "TCON", "TYER", "TDRC", "TRCK", "TSOT", "TIT3", "TIT1", "TSOP", "TMCL", "TIPL", "TCOM", "TSOC", "TSO2", "TSOA", "TPOS", "TBPM", "TPE3", "TCOP", "TKEY", "TPE4", "TPUB", "TSRC" };

    public static IReadOnlyList<(string Key, string Label)> AdditionalStandardFields { get; } =
    [
        ("TITLE_SORT", "Title sort"), ("SUBTITLE", "Subtitle"), ("DESCRIPTION", "Description"),
        ("ARTIST_SORT", "Artist sort"), ("ARTIST_ROLE", "Artist role / instruments"), ("ALBUM_ARTIST_SORT", "Album artist sort"),
        ("COMPOSERS", "Composers"), ("COMPOSER_SORT", "Composer sort"), ("ALBUM_SORT", "Album sort"),
        ("COMMENT", "Comment"), ("TRACK_COUNT", "Track count"), ("DISC", "Disc number"), ("DISC_COUNT", "Disc count"),
        ("GROUPING", "Grouping"), ("BPM", "Beats per minute"), ("CONDUCTOR", "Conductor"), ("COPYRIGHT", "Copyright"),
        ("MUSICBRAINZ_ARTIST_ID", "MusicBrainz artist ID"), ("MUSICBRAINZ_RELEASE_GROUP_ID", "MusicBrainz release group ID"),
        ("MUSICBRAINZ_RELEASE_ID", "MusicBrainz release ID"), ("MUSICBRAINZ_RELEASE_ARTIST_ID", "MusicBrainz release artist ID"),
        ("MUSICBRAINZ_TRACK_ID", "MusicBrainz track ID"), ("MUSICBRAINZ_DISC_ID", "MusicBrainz disc ID"),
        ("MUSICIP_ID", "MusicIP ID"), ("AMAZON_ID", "Amazon ID"), ("MUSICBRAINZ_RELEASE_STATUS", "Release status"),
        ("MUSICBRAINZ_RELEASE_TYPE", "Release type"), ("MUSICBRAINZ_RELEASE_COUNTRY", "Release country"),
        ("DATE_TAGGED", "Date tagged (ISO 8601)"),
        ("REPLAYGAIN_TRACK_GAIN", "ReplayGain track gain (dB)"), ("REPLAYGAIN_TRACK_PEAK", "ReplayGain track peak"),
        ("REPLAYGAIN_ALBUM_GAIN", "ReplayGain album gain (dB)"), ("REPLAYGAIN_ALBUM_PEAK", "ReplayGain album peak"),
        ("INITIAL_KEY", "Initial key"), ("REMIXED_BY", "Remixed by"), ("PUBLISHER", "Publisher"), ("ISRC", "ISRC")
    ];

    public static string? CustomFieldFormat(string path) => FormatFor(path) switch
    {
        CustomFormat.Xiph => "Xiph/Vorbis comments",
        CustomFormat.Id3v2 => "ID3v2 text frames and user text",
        CustomFormat.Asf => "ASF descriptors",
        CustomFormat.Ape => "APEv2 items",
        _ => null
    };

    public static IReadOnlyDictionary<string, string> ReadCustomFields(string path)
    {
        using var media = AudioTags.Open(path);
        return ReadCustomFields(media, path);
    }

    public static IReadOnlyDictionary<string, string> ReadAdditionalStandardFields(string path)
    {
        using var media = AudioTags.Open(path);
        return AdditionalStandardFields.ToDictionary(field => field.Key, field => ReadAdditionalField(media.Tag, field.Key), StringComparer.OrdinalIgnoreCase);
    }

    public TagBackup Save(string path, TagEdit edit) => SaveCore(path, edit, null, default);

    /// <summary>Copies and edits the file away from the UI thread, checking cancellation between copy chunks and before replacement.</summary>
    public Task<TagBackup> SaveAsync(string path, TagEdit edit, IProgress<TagEditProgress>? progress = null,
        CancellationToken cancellationToken = default) =>
        Task.Run(() => SaveCore(path, edit, progress, cancellationToken), cancellationToken);

    public TagBackup Restore(TagBackup backup) => RestoreCore(backup, null, default);

    /// <summary>Preserves the current file as a new undo snapshot before restoring the selected snapshot.</summary>
    public Task<TagBackup> RestoreAsync(TagBackup backup, IProgress<TagEditProgress>? progress = null,
        CancellationToken cancellationToken = default) =>
        Task.Run(() => RestoreCore(backup, progress, cancellationToken), cancellationToken);

    public IReadOnlyList<TagBackup> ListBackups(string path)
    {
        var original = Path.GetFullPath(path);
        var suffix = BackupSuffix(original);
        if (!Directory.Exists(_backupDirectory)) return [];
        return Directory.EnumerateFiles(_backupDirectory, $"*-{suffix}-*.bak", SearchOption.TopDirectoryOnly)
            .Select(file => new TagBackup(original, file, ParseBackupStamp(file)))
            .OrderByDescending(item => item.CreatedUtc)
            .Take(BackupRetentionPerTrack)
            .ToArray();
    }

    private TagBackup SaveCore(string path, TagEdit edit, IProgress<TagEditProgress>? progress, CancellationToken cancellationToken)
    {
        path = Path.GetFullPath(path);
        using var pathMutex = new FilePathLock(path);
        cancellationToken.ThrowIfCancellationRequested();
        Directory.CreateDirectory(_backupDirectory);
        var backup = NewBackupPath(path);
        var backupStage = backup + ".partial";
        var staged = TemporaryPath(path, "stage");
        var totalBytes = new FileInfo(path).Length;
        try
        {
            CopyCancellable(path, staged, "Preparing edit", totalBytes, progress, cancellationToken);
            CopyCancellable(path, backupStage, "Preparing recovery copy", totalBytes, progress, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            using (var media = AudioTags.Open(staged))
            {
                var tag = media.Tag;
                if (edit.Title is not null) tag.Title = edit.Title;
                if (edit.Artist is not null) tag.Performers = Split(edit.Artist);
                if (edit.Album is not null) tag.Album = edit.Album;
                if (edit.AlbumArtist is not null) tag.AlbumArtists = Split(edit.AlbumArtist);
                if (edit.Genre is not null) tag.Genres = Split(edit.Genre);
                if (edit.Year.HasValue) tag.Year = edit.Year.Value;
                if (edit.TrackNumber.HasValue) tag.Track = edit.TrackNumber.Value;
                if (edit.Lyrics is not null)
                {
                    tag.Lyrics = string.IsNullOrWhiteSpace(edit.Lyrics) ? null : edit.Lyrics;
                    if (string.IsNullOrWhiteSpace(edit.Lyrics) && media.GetTag(TagTypes.Id3v2) is TagLib.Id3v2.Tag id3)
                    {
                        id3.RemoveFrames("SYLT");
                        id3.RemoveFrames("USLT");
                    }
                }
                if (edit.ArtworkPath is not null) tag.Pictures = [new Picture(edit.ArtworkPath)];
                ApplyAdditionalFields(tag, edit.AdditionalFields);
                ApplyCustomFields(media, path, edit.CustomFields);
                media.Save();
            }
            cancellationToken.ThrowIfCancellationRequested();
            System.IO.File.Move(backupStage, backup, false);
            // Same-directory replacement leaves the original intact until the edited copy is complete.
            System.IO.File.Move(staged, path, true);
            PruneBackupFiles();
            return new(path, backup, ParseBackupStamp(backup));
        }
        finally
        {
            TryDelete(staged);
            TryDelete(backupStage);
        }
    }

    private TagBackup RestoreCore(TagBackup selected, IProgress<TagEditProgress>? progress, CancellationToken cancellationToken)
    {
        var original = Path.GetFullPath(selected.OriginalPath);
        var source = Path.GetFullPath(selected.BackupPath);
        if (!IsManagedBackup(original, source)) throw new InvalidDataException("The selected backup does not belong to this track's Music Player backup history.");
        if (!System.IO.File.Exists(source)) throw new FileNotFoundException("The saved tag backup is missing.", source);
        using var pathMutex = new FilePathLock(original);
        cancellationToken.ThrowIfCancellationRequested();
        Directory.CreateDirectory(_backupDirectory);
        var undoBackup = NewBackupPath(original);
        var backupStage = undoBackup + ".partial";
        var restoreStage = TemporaryPath(original, "restore");
        var totalBytes = new FileInfo(original).Length;
        try
        {
            CopyCancellable(original, backupStage, "Preparing undo copy", totalBytes, progress, cancellationToken);
            CopyCancellable(source, restoreStage, "Preparing restore", new FileInfo(source).Length, progress, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            System.IO.File.Move(backupStage, undoBackup, false);
            System.IO.File.Move(restoreStage, original, true);
            PruneBackupFiles();
            return new(original, undoBackup, ParseBackupStamp(undoBackup));
        }
        finally
        {
            TryDelete(restoreStage);
            TryDelete(backupStage);
        }
    }

    private string NewBackupPath(string original)
    {
        var stamp = DateTime.UtcNow.ToString("yyyyMMdd-HHmmssfff", CultureInfo.InvariantCulture);
        return Path.Combine(_backupDirectory, $"{stamp}-{BackupSuffix(original)}-{Guid.NewGuid():N}.bak");
    }

    private static string BackupSuffix(string original) =>
        Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(Path.GetFullPath(original))))[..10];

    private bool IsManagedBackup(string original, string candidate)
    {
        var directory = Path.GetDirectoryName(candidate);
        return directory is not null && Path.GetFullPath(directory).Equals(_backupDirectory, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal)
            && Path.GetFileName(candidate).Contains($"-{BackupSuffix(original)}-", StringComparison.OrdinalIgnoreCase);
    }

    private void PruneBackupFiles()
    {
        try
        {
            if (!Directory.Exists(_backupDirectory)) return;
            foreach (var group in Directory.EnumerateFiles(_backupDirectory, "*.bak", SearchOption.TopDirectoryOnly)
                .Select(file => (File: file, Name: Path.GetFileName(file).Split('-')))
                .Where(item => item.Name.Length == 4 && item.Name[3].EndsWith(".bak", StringComparison.OrdinalIgnoreCase))
                .GroupBy(item => item.Name[2], StringComparer.OrdinalIgnoreCase))
            {
                foreach (var old in group.OrderByDescending(item => item.Name[0] + "-" + item.Name[1] + "-" + item.Name[3])
                    .Skip(BackupRetentionPerTrack)) TryDelete(old.File);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            LocalAppLog.Shared.Warning("tag-editor", "Could not prune old tag backups.", ex);
        }
    }

    private static DateTime ParseBackupStamp(string path)
    {
        var stamp = Path.GetFileName(path);
        return stamp.Length >= 18 && DateTime.TryParseExact(stamp[..18], "yyyyMMdd-HHmmssfff", CultureInfo.InvariantCulture,
            DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out var created) ? created : System.IO.File.GetLastWriteTimeUtc(path);
    }

    private static void CopyCancellable(string source, string destination, string phase, long totalBytes,
        IProgress<TagEditProgress>? progress, CancellationToken cancellationToken)
    {
        const int bufferSize = 128 * 1024;
        var buffer = ArrayPool<byte>.Shared.Rent(bufferSize);
        long copied = 0;
        try
        {
            using var input = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read, bufferSize, FileOptions.SequentialScan);
            using var output = new FileStream(destination, FileMode.CreateNew, FileAccess.Write, FileShare.None, bufferSize, FileOptions.SequentialScan);
            progress?.Report(new(phase, 0, totalBytes));
            int read;
            while ((read = input.Read(buffer, 0, bufferSize)) > 0)
            {
                cancellationToken.ThrowIfCancellationRequested();
                output.Write(buffer, 0, read);
                copied += read;
                if (copied == totalBytes || copied % (4 * 1024 * 1024) < read) progress?.Report(new(phase, copied, totalBytes));
            }
            cancellationToken.ThrowIfCancellationRequested();
            output.Flush(flushToDisk: true);
        }
        finally { ArrayPool<byte>.Shared.Return(buffer); }
    }

    private static void TryDelete(string path)
    {
        try { if (System.IO.File.Exists(path)) System.IO.File.Delete(path); }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }

    private static string TemporaryPath(string path, string purpose) => Path.Combine(Path.GetDirectoryName(path)!,
        $".musicplayer-{purpose}-{Guid.NewGuid():N}{Path.GetExtension(path)}");

    private static string[] Split(string value) => value.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

    private static string ReadAdditionalField(TagLib.Tag tag, string key) => key switch
    {
        "TITLE_SORT" => tag.TitleSort ?? "", "SUBTITLE" => tag.Subtitle ?? "", "DESCRIPTION" => tag.Description ?? "",
        "ARTIST_SORT" => string.Join("; ", tag.PerformersSort), "ARTIST_ROLE" => string.Join("; ", tag.PerformersRole),
        "ALBUM_ARTIST_SORT" => string.Join("; ", tag.AlbumArtistsSort), "COMPOSERS" => string.Join("; ", tag.Composers),
        "COMPOSER_SORT" => string.Join("; ", tag.ComposersSort), "ALBUM_SORT" => tag.AlbumSort ?? "", "COMMENT" => tag.Comment ?? "",
        "TRACK_COUNT" => Number(tag.TrackCount), "DISC" => Number(tag.Disc), "DISC_COUNT" => Number(tag.DiscCount),
        "GROUPING" => tag.Grouping ?? "", "BPM" => Number(tag.BeatsPerMinute), "CONDUCTOR" => tag.Conductor ?? "", "COPYRIGHT" => tag.Copyright ?? "",
        "MUSICBRAINZ_ARTIST_ID" => tag.MusicBrainzArtistId ?? "", "MUSICBRAINZ_RELEASE_GROUP_ID" => tag.MusicBrainzReleaseGroupId ?? "",
        "MUSICBRAINZ_RELEASE_ID" => tag.MusicBrainzReleaseId ?? "", "MUSICBRAINZ_RELEASE_ARTIST_ID" => tag.MusicBrainzReleaseArtistId ?? "",
        "MUSICBRAINZ_TRACK_ID" => tag.MusicBrainzTrackId ?? "", "MUSICBRAINZ_DISC_ID" => tag.MusicBrainzDiscId ?? "",
        "MUSICIP_ID" => tag.MusicIpId ?? "", "AMAZON_ID" => tag.AmazonId ?? "", "MUSICBRAINZ_RELEASE_STATUS" => tag.MusicBrainzReleaseStatus ?? "",
        "MUSICBRAINZ_RELEASE_TYPE" => tag.MusicBrainzReleaseType ?? "", "MUSICBRAINZ_RELEASE_COUNTRY" => tag.MusicBrainzReleaseCountry ?? "",
        "DATE_TAGGED" => tag.DateTagged?.ToString("O", CultureInfo.InvariantCulture) ?? "",
        "REPLAYGAIN_TRACK_GAIN" => Number(tag.ReplayGainTrackGain), "REPLAYGAIN_TRACK_PEAK" => Number(tag.ReplayGainTrackPeak),
        "REPLAYGAIN_ALBUM_GAIN" => Number(tag.ReplayGainAlbumGain), "REPLAYGAIN_ALBUM_PEAK" => Number(tag.ReplayGainAlbumPeak),
        "INITIAL_KEY" => tag.InitialKey ?? "", "REMIXED_BY" => tag.RemixedBy ?? "", "PUBLISHER" => tag.Publisher ?? "", "ISRC" => tag.ISRC ?? "",
        _ => throw new ArgumentException($"Unknown standard tag field '{key}'.")
    };

    private static void ApplyAdditionalFields(TagLib.Tag tag, IReadOnlyDictionary<string, string>? fields)
    {
        if (fields is null) return;
        foreach (var (rawKey, value) in fields)
        {
            var key = rawKey.Trim().ToUpperInvariant();
            if (!AdditionalStandardFields.Any(field => field.Key == key)) throw new ArgumentException($"Unknown standard tag field '{rawKey}'.");
            switch (key)
            {
                case "TITLE_SORT": tag.TitleSort = value; break;
                case "SUBTITLE": tag.Subtitle = value; break;
                case "DESCRIPTION": tag.Description = value; break;
                case "ARTIST_SORT": tag.PerformersSort = Split(value); break;
                case "ARTIST_ROLE": tag.PerformersRole = Split(value); break;
                case "ALBUM_ARTIST_SORT": tag.AlbumArtistsSort = Split(value); break;
                case "COMPOSERS": tag.Composers = Split(value); break;
                case "COMPOSER_SORT": tag.ComposersSort = Split(value); break;
                case "ALBUM_SORT": tag.AlbumSort = value; break;
                case "COMMENT": tag.Comment = value; break;
                case "TRACK_COUNT": tag.TrackCount = ParseUInt(value, rawKey); break;
                case "DISC": tag.Disc = ParseUInt(value, rawKey); break;
                case "DISC_COUNT": tag.DiscCount = ParseUInt(value, rawKey); break;
                case "GROUPING": tag.Grouping = value; break;
                case "BPM": tag.BeatsPerMinute = ParseUInt(value, rawKey); break;
                case "CONDUCTOR": tag.Conductor = value; break;
                case "COPYRIGHT": tag.Copyright = value; break;
                case "MUSICBRAINZ_ARTIST_ID": tag.MusicBrainzArtistId = value; break;
                case "MUSICBRAINZ_RELEASE_GROUP_ID": tag.MusicBrainzReleaseGroupId = value; break;
                case "MUSICBRAINZ_RELEASE_ID": tag.MusicBrainzReleaseId = value; break;
                case "MUSICBRAINZ_RELEASE_ARTIST_ID": tag.MusicBrainzReleaseArtistId = value; break;
                case "MUSICBRAINZ_TRACK_ID": tag.MusicBrainzTrackId = value; break;
                case "MUSICBRAINZ_DISC_ID": tag.MusicBrainzDiscId = value; break;
                case "MUSICIP_ID": tag.MusicIpId = value; break;
                case "AMAZON_ID": tag.AmazonId = value; break;
                case "MUSICBRAINZ_RELEASE_STATUS": tag.MusicBrainzReleaseStatus = value; break;
                case "MUSICBRAINZ_RELEASE_TYPE": tag.MusicBrainzReleaseType = value; break;
                case "MUSICBRAINZ_RELEASE_COUNTRY": tag.MusicBrainzReleaseCountry = value; break;
                case "DATE_TAGGED": tag.DateTagged = string.IsNullOrWhiteSpace(value) ? null : DateTime.TryParse(value, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var tagged) ? tagged : throw new ArgumentException("'Date tagged' must use an ISO 8601 date and time."); break;
                case "REPLAYGAIN_TRACK_GAIN": tag.ReplayGainTrackGain = ParseDouble(value, rawKey); break;
                case "REPLAYGAIN_TRACK_PEAK": tag.ReplayGainTrackPeak = ParseDouble(value, rawKey); break;
                case "REPLAYGAIN_ALBUM_GAIN": tag.ReplayGainAlbumGain = ParseDouble(value, rawKey); break;
                case "REPLAYGAIN_ALBUM_PEAK": tag.ReplayGainAlbumPeak = ParseDouble(value, rawKey); break;
                case "INITIAL_KEY": tag.InitialKey = value; break;
                case "REMIXED_BY": tag.RemixedBy = value; break;
                case "PUBLISHER": tag.Publisher = value; break;
                case "ISRC": tag.ISRC = value; break;
            }
        }
    }

    private static string Number(uint value) => value == 0 ? "" : value.ToString(CultureInfo.InvariantCulture);
    private static string Number(double value) => double.IsNaN(value) ? "" : value.ToString("R", CultureInfo.InvariantCulture);
    private static uint ParseUInt(string value, string field) => string.IsNullOrWhiteSpace(value) ? 0 : uint.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var parsed) ? parsed : throw new ArgumentException($"'{field}' must be a whole number.");
    private static double ParseDouble(string value, string field) => string.IsNullOrWhiteSpace(value) ? double.NaN : double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed) && double.IsFinite(parsed) ? parsed : throw new ArgumentException($"'{field}' must be a finite number.");

    private static void ApplyCustomFields(TagLib.File media, string path, IReadOnlyDictionary<string, string>? fields)
    {
        if (fields is null) return;
        var format = FormatFor(path);
        if (format == CustomFormat.Unsupported) throw new NotSupportedException("Custom tags are not supported for this file format.");
        var desired = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (rawKey, value) in fields)
        {
            var key = rawKey.Trim();
            if (key.Length == 0 || key.Contains('=') || key.Contains('\n') || key.Contains('\r')) throw new ArgumentException("Custom tag names cannot be empty or contain '=', or line breaks.");
            if (format == CustomFormat.Xiph && key.Any(character => !char.IsAsciiLetterOrDigit(character) && character != '_'))
                throw new ArgumentException("Xiph field names can contain only ASCII letters, digits, and underscores.");
            if (format != CustomFormat.Id3v2 && CommonFields.Contains(key)) throw new ArgumentException($"'{key}' is already edited in a standard tag field.");
            desired[key] = value;
        }

        var existing = ReadCustomFields(media, path);
        foreach (var key in existing.Keys.Where(key => !desired.ContainsKey(key))) SetCustomField(media, format, key, string.Empty);
        foreach (var (key, value) in desired)
        {
            var storedKey = existing.Keys.FirstOrDefault(existingKey => existingKey.Equals(key, StringComparison.OrdinalIgnoreCase)) ?? key;
            SetCustomField(media, format, storedKey, value);
        }
    }

    private static IReadOnlyDictionary<string, string> ReadCustomFields(TagLib.File media, string path)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        switch (FormatFor(path))
        {
            case CustomFormat.Xiph when media.GetTag(TagTypes.Xiph) is TagLib.Ogg.XiphComment xiph:
                foreach (var key in xiph.Where(key => !CommonFields.Contains(key)))
                    result[key] = string.Join("; ", xiph.GetField(key));
                break;
            case CustomFormat.Id3v2 when media.GetTag(TagTypes.Id3v2) is TagLib.Id3v2.Tag id3:
                foreach (var frame in id3.GetFrames<TagLib.Id3v2.TextInformationFrame>())
                {
                    if (frame is TagLib.Id3v2.UserTextInformationFrame user)
                    {
                        if (!string.IsNullOrWhiteSpace(user.Description) && user.Text.Length > 0)
                            result[user.Description] = string.Join("; ", user.Text);
                        continue;
                    }
                    var frameId = Encoding.ASCII.GetString(frame.FrameId.Data);
                    if (!CommonId3Frames.Contains(frameId) && frame.Text.Length > 0)
                        result["ID3:" + frameId] = string.Join("; ", frame.Text);
                }
                break;
            case CustomFormat.Asf when media.GetTag(TagTypes.Asf) is TagLib.Asf.Tag asf:
                foreach (var descriptor in asf.Where(descriptor => !CommonFields.Contains(descriptor.Name) && descriptor.Type == TagLib.Asf.DataType.Unicode))
                    result[descriptor.Name] = descriptor.ToString();
                break;
            case CustomFormat.Ape when media.GetTag(TagTypes.Ape) is TagLib.Ape.Tag ape:
                foreach (var key in ape.Where(key => !CommonFields.Contains(key)))
                    if (ape.GetItem(key) is { Type: TagLib.Ape.ItemType.Text } item)
                        result[key] = string.Join("; ", item.ToStringArray());
                break;
        }
        return result;
    }

    private static void SetCustomField(TagLib.File media, CustomFormat format, string key, string value)
    {
        var values = Split(value);
        switch (format)
        {
            case CustomFormat.Xiph:
                var xiph = media.GetTag(TagTypes.Xiph, true) as TagLib.Ogg.XiphComment
                    ?? throw new NotSupportedException("Xiph comments are not supported by this file format.");
                xiph.SetField(key, values);
                break;
            case CustomFormat.Id3v2:
                var id3 = media.GetTag(TagTypes.Id3v2, true) as TagLib.Id3v2.Tag
                    ?? throw new NotSupportedException("ID3v2 tags are not supported by this file format.");
                if (key.StartsWith("ID3:", StringComparison.OrdinalIgnoreCase))
                {
                    var frameId = key[4..].ToUpperInvariant();
                    if (frameId.Length != 4 || frameId.Any(character => !char.IsAsciiLetterOrDigit(character)) || CommonId3Frames.Contains(frameId) || frameId == "TXXX")
                        throw new ArgumentException("Use ID3: followed by a non-standard four-character text frame identifier.");
                    id3.SetTextFrame(ByteVector.FromString(frameId, StringType.Latin1), values);
                    break;
                }
                var frame = TagLib.Id3v2.UserTextInformationFrame.Get(id3, key, false);
                if (values.Length == 0) { if (frame is not null) id3.RemoveFrame(frame); }
                else (frame ?? TagLib.Id3v2.UserTextInformationFrame.Get(id3, key, true)!).Text = values;
                break;
            case CustomFormat.Asf:
                var asf = media.GetTag(TagTypes.Asf, true) as TagLib.Asf.Tag
                    ?? throw new NotSupportedException("ASF tags are not supported by this file format.");
                asf.SetDescriptorStrings(values, key);
                break;
            case CustomFormat.Ape:
                var ape = media.GetTag(TagTypes.Ape, true) as TagLib.Ape.Tag
                    ?? throw new NotSupportedException("APEv2 tags are not supported by this file format.");
                ape.SetValue(key, values);
                break;
            default:
                throw new NotSupportedException("Custom tags are not supported for this file format.");
        }
    }

    private static CustomFormat FormatFor(string path) => Path.GetExtension(path).ToLowerInvariant() switch
    {
        ".flac" or ".ogg" or ".oga" or ".opus" => CustomFormat.Xiph,
        ".wma" => CustomFormat.Asf,
        ".ape" or ".wv" or ".mpc" => CustomFormat.Ape,
        ".mp3" or ".wav" or ".wave" or ".aif" or ".aiff" or ".aac" or ".tta" or ".dsf" or ".dff" => CustomFormat.Id3v2,
        _ => CustomFormat.Unsupported
    };

    private enum CustomFormat { Unsupported, Xiph, Id3v2, Asf, Ape }
}

public sealed record TagEditProgress(string Phase, long BytesCopied, long TotalBytes);
