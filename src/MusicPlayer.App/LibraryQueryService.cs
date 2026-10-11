using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>
/// Owns database-backed browsing queries and the projection/identity mapping used by the
/// WebView library UI. It has no dependency on WinUI controls or window lifetime.
/// </summary>
internal sealed class LibraryQueryService
{
    private static readonly HashSet<string> Views = new(StringComparer.Ordinal)
    {
        "Home", "Search", "Songs", "Albums", "Artists", "Genres", "Folders", "Favorites", "Most Played",
        "Recently Played", "Recently Added", "Playlists", "Playlist", "Album", "Artist", "Genre", "Folder",
        "With Lyrics", "Videos", "Queue", "Audio", "Settings", "Duplicates", "Lyrics", "Now Playing"
    };

    private readonly LibraryStore _store;
    private readonly string _artworkDirectory;
    private volatile HashSet<string> _videoExtensions = new(LibraryScanner.VideoExtensions, StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, string> _trackPaths = new(StringComparer.Ordinal);
    private readonly object _trackPathGate = new();
    private readonly HashSet<string> _unavailableTrackPaths = new(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
    private readonly object _availabilityGate = new();
    private readonly HashSet<string> _unavailableRootPaths = new(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
    private readonly object _rootAvailabilityGate = new();
    private readonly object _folderCacheGate = new();
    private readonly object _searchGate = new();
    private IReadOnlyList<string>? _folderCache;
    private SearchBatch? _activeSearch;
    private long _latestSearchRequestId;
    private ViewContext _context = new("Home", "", null, null, null);
    private TrackSort _sort = TrackSort.Title;
    private bool _descending;

    private sealed class SearchBatch(long requestId)
    {
        public long RequestId { get; } = requestId;
        public CancellationTokenSource Cancellation { get; } = new();
        public int ActiveRequests { get; set; }
        public bool Retired { get; set; }
    }

    public LibraryQueryService(LibraryStore store, string appDataDirectory)
    {
        _store = store;
        _store.UseCachedFingerprints = true;
        _artworkDirectory = Path.GetFullPath(Path.Combine(appDataDirectory, "Artwork"));
    }

    public string CurrentView => _context.View;
    public string CurrentSearch => _context.Search;

    public void SetVideoExtensions(IEnumerable<string> extensions)
    {
        var normalized = extensions.Select(extension => extension.StartsWith('.') ? extension : $".{extension}")
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        _videoExtensions = normalized;
        _store.SetVideoExtensions(normalized);
        if (normalized.Count == 0 && _sort == TrackSort.Type) _sort = TrackSort.Title;
        if (normalized.Count == 0 && _context.View == "Videos")
        {
            _context = new("Playlists", "", null, null, null);
            PersistContext();
        }
    }
    public object? CurrentGroup
    {
        get
        {
            if (string.IsNullOrWhiteSpace(_context.GroupColumn) || string.IsNullOrWhiteSpace(_context.GroupValue)) return null;
            return new { column = _context.GroupColumn, name = _context.GroupValue, id = _context.GroupValue };
        }
    }
    public object? CurrentPlaylist
    {
        get
        {
            if (_context.View != "Playlist" || string.IsNullOrWhiteSpace(_context.PlaylistId)) return null;
            var playlist = _store.GetPlaylistSummaries().FirstOrDefault(item => item.Id == _context.PlaylistId);
            return playlist is null ? null : PlaylistDto(playlist);
        }
    }

    public void RestoreContext(string? serialized)
    {
        if (string.IsNullOrWhiteSpace(serialized) || serialized.Length > 8192) return;
        try
        {
            using var document = JsonDocument.Parse(serialized);
            if (document.RootElement.ValueKind != JsonValueKind.Object) return;
            SetContext(document.RootElement);
            if (_context.View == "Playlist" &&
                (string.IsNullOrWhiteSpace(_context.PlaylistId) || !_store.GetPlaylistSummaries().Any(item => item.Id == _context.PlaylistId)))
                SetContext(JsonDocument.Parse("{\"view\":\"Playlists\"}").RootElement);
        }
        catch (Exception ex) when (ex is JsonException or ArgumentException or InvalidOperationException)
        {
            LocalAppLog.Shared.Warning("navigation", "The saved player view could not be restored; opening Home instead.", ex);
            _context = new("Home", "", null, null, null);
        }
    }

    public object HomeData(int requestedPageSize)
    {
        var pageSize = Math.Clamp(requestedPageSize, 1, 20);
        var stats = _store.GetLibraryStats();
        IReadOnlyList<Track> Page(string? filter, TrackSort sort) => _store.GetTracksPageWithCount(null, sort, true, filter,
            offset: 0, pageSize: pageSize, hideExactDuplicates: HideExactDuplicates).Tracks;
        var quick = Page("favorites", TrackSort.Added);
        if (quick.Count == 0) quick = Page("recent", TrackSort.LastPlayed);
        if (quick.Count == 0) quick = Page(null, TrackSort.Added);
        return new
        {
            totalTracks = stats.TotalTracks,
            totalBytes = stats.TotalBytes,
            quick = quick.Select(track => TrackDto(track)).ToArray(),
            recent = Page("recent", TrackSort.LastPlayed).Select(track => TrackDto(track)).ToArray(),
            mostPlayed = Page("most-played", TrackSort.PlayCount).Select(track => TrackDto(track)).ToArray(),
            recentlyAdded = Page(null, TrackSort.Added).Select(track => TrackDto(track)).ToArray()
        };
    }

    public void BeginSearch(long requestId)
    {
        if (requestId <= 0) throw new ArgumentOutOfRangeException(nameof(requestId), "A search request id is required.");
        lock (_searchGate)
        {
            if (requestId <= _latestSearchRequestId) return;
            _latestSearchRequestId = requestId;
            if (_activeSearch is { } previous)
            {
                previous.Retired = true;
                previous.Cancellation.Cancel();
                DisposeSearchIfFinished(previous);
            }
            _activeSearch = new SearchBatch(requestId);
        }
    }

    public async Task<object> SearchAsync(JsonElement payload)
    {
        var requestId = Int64(payload, "requestId");
        SearchBatch? batch;
        lock (_searchGate)
        {
            batch = _activeSearch is { } current && current.RequestId == requestId ? current : null;
            if (batch is not null) batch.ActiveRequests++;
        }
        if (batch is null) throw new OperationCanceledException("This search was superseded by a newer request.");
        try
        {
            var request = payload.Clone();
            return await Task.Run(() => Search(request, batch.Cancellation.Token), batch.Cancellation.Token).ConfigureAwait(false);
        }
        finally
        {
            lock (_searchGate)
            {
                batch.ActiveRequests--;
                DisposeSearchIfFinished(batch);
            }
        }
    }

    private object Search(JsonElement payload, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var query = String(payload, "query");
        var filter = String(payload, "filter");
        var offset = Math.Max(0, Int(payload, "offset"));
        var size = Math.Clamp(Int(payload, "pageSize", 40), 1, 100);
        var result = filter switch
        {
            "song" => SearchTrackPage(query, offset, size, cancellationToken),
            "album" => SearchGroupPage("album", query, offset, size, cancellationToken),
            "artist" => SearchGroupPage("artist", query, offset, size, cancellationToken),
            "playlist" => SearchPlaylistPage(query, offset, size, cancellationToken),
            _ => throw new ArgumentException("Choose a search result type.")
        };
        cancellationToken.ThrowIfCancellationRequested();
        return result;
    }

    internal PreparedTrackPage PrepareTrackPage(JsonElement payload)
    {
        SetContext(payload);
        var filter = _context.View switch
        {
            "Favorites" => "favorites",
            "Most Played" => "most-played",
            "Recently Played" => "recent",
            "With Lyrics" => "with-lyrics",
            "Videos" => "videos",
            _ => null
        };
        var requestedSort = String(payload, "sort");
        var sortSpecified = payload.TryGetProperty("sort", out var sortValue) && sortValue.ValueKind == JsonValueKind.String &&
                            !string.IsNullOrWhiteSpace(sortValue.GetString());
        var sort = sortSpecified ? ParseSort(requestedSort) : _context.View switch
        {
            "Most Played" => TrackSort.PlayCount,
            "Recently Played" => TrackSort.LastPlayed,
            "Recently Added" => TrackSort.Added,
            _ => TrackSort.Title
        };
        if (sort == TrackSort.Type && _videoExtensions.Count == 0) sort = TrackSort.Title;
        _sort = sort;
        var descendingSpecified = payload.TryGetProperty("descending", out var descendingValue) &&
                                  descendingValue.ValueKind is JsonValueKind.True or JsonValueKind.False;
        _descending = descendingSpecified
            ? Boolean(payload, "descending")
            : _context.View is "Most Played" or "Recently Played" or "Recently Added";
        // Search is a separate destination. A saved/transient search must never make
        // the ordinary Songs library appear empty after navigation or restart.
        var search = _context.View == "Search" ? _context.Search : string.Empty;
        return new(_context.View, search, _context.GroupColumn, _context.GroupValue, sort, _descending,
            filter, Math.Max(0, Int(payload, "offset")), Math.Clamp(Int(payload, "pageSize", 100), 1, 200));
    }

    internal object TrackPage(PreparedTrackPage request)
    {
        TrackPageResult result;
        try
        {
            result = _store.GetTracksPageWithCount(request.Search, request.Sort, request.Descending, request.Filter,
                request.GroupColumn, request.GroupValue, request.Offset, request.PageSize,
                HideExactDuplicates && request.GroupColumn is null);
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex) when (request.View == "Songs")
        {
            LocalAppLog.Shared.Warning("library-query", $"Songs page query failed ({ex.GetType().Name}); retrying directly without duplicate suppression.");
            result = _store.GetTracksPageWithCount(null, request.Sort, request.Descending, offset: request.Offset,
                pageSize: request.PageSize, hideExactDuplicates: false);
        }

        var isPlainSongsPage = request.View == "Songs" && string.IsNullOrWhiteSpace(request.Search) &&
                               request.Filter is null && request.GroupColumn is null;
        if (isPlainSongsPage && result.TotalCount == 0)
        {
            var indexedCount = _store.GetLibraryStats().TotalTracks;
            if (indexedCount > 0)
            {
                LocalAppLog.Shared.Warning("library-query", $"Songs page returned 0 rows while the database has {indexedCount:N0} tracks; retrying without duplicate suppression.");
                result = _store.GetTracksPageWithCount(null, request.Sort, request.Descending, offset: request.Offset,
                    pageSize: request.PageSize, hideExactDuplicates: false);
                if (result.TotalCount > 0)
                    LocalAppLog.Shared.Info("library-query", $"Songs page fallback restored {result.TotalCount:N0} indexed tracks.");
                else
                    LocalAppLog.Shared.Warning("library-query", $"Direct Songs page query also returned 0 rows while the database reports {indexedCount:N0} tracks.");
            }
            else
            {
                LocalAppLog.Shared.Info("library-query", "Songs page returned 0 rows and the active library database also contains 0 tracks.");
            }
        }
        return new { tracks = result.Tracks.Select(track => TrackDto(track)).ToArray(), totalCount = result.TotalCount };
    }

    public object TrackPage(JsonElement payload) => TrackPage(PrepareTrackPage(payload));

    public object GroupPage(JsonElement payload)
    {
        var column = GroupColumnFromType(String(payload, "column"));
        var search = String(payload, "search");
        var offset = Math.Max(0, Int(payload, "offset"));
        var pageSize = Math.Clamp(Int(payload, "pageSize", 60), 1, 200);
        var groups = _store.GetGroupsPage(column, search, offset, pageSize).Select(group => GroupDto(group, column)).ToArray();
        return new { groups, totalCount = _store.CountGroups(column, search) };
    }

    public object FolderData(JsonElement payload, object scanState)
    {
        var roots = ReadJsonSetting("library-roots", Array.Empty<string>());
        var counts = _store.GetRootTrackCounts(roots);
        var rootDtos = roots.Select(path =>
        {
            var normalizedPath = NormalizeRootPath(path);
            bool scanUnavailable;
            lock (_rootAvailabilityGate) scanUnavailable = _unavailableRootPaths.Contains(normalizedPath);
            var available = Directory.Exists(path) && !scanUnavailable;
            return new
            {
                path,
                name = Path.GetFileName(path.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)),
                trackCount = counts.GetValueOrDefault(path),
                available,
                status = available ? "Available" : "Unavailable"
            };
        }).ToArray();
        if (Boolean(payload, "rootsOnly"))
            return new { roots = rootDtos, folders = Array.Empty<object>(), folderCount = 0, folderOffset = 0, scan = scanState };

        IReadOnlyList<string> allFolders;
        lock (_folderCacheGate) allFolders = _folderCache ??= _store.GetFolders();
        var offset = Math.Max(0, Int(payload, "offset"));
        var size = Math.Clamp(Int(payload, "pageSize", 50), 1, 100);
        var folders = allFolders.Skip(offset).Take(size).Select(path => new
        {
            path,
            name = Path.GetFileName(path.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)),
            trackCount = _store.CountTracks(groupColumn: "folder", groupValue: path)
        }).ToArray();
        return new { roots = rootDtos, folders, folderCount = allFolders.Count, folderOffset = offset, scan = scanState };
    }

    public object ArtistAlbums(string artist, int requestedOffset, int requestedPageSize)
    {
        var offset = Math.Max(0, requestedOffset);
        var pageSize = Math.Clamp(requestedPageSize, 1, 60);
        var page = _store.GetArtistAlbumsPage(artist, offset, pageSize + 1);
        return new { groups = page.Take(pageSize).Select(group => GroupDto(group, "album")).ToArray(), hasMore = page.Count > pageSize };
    }

    public object Playlists() => new
    {
        playlists = _store.GetPlaylistSummaries().Select(PlaylistDto).ToArray(),
        videosEnabled = _videoExtensions.Count > 0
    };

    public void SetContext(JsonElement payload)
    {
        var requestedView = String(payload, "view");
        var view = _context.View;
        var search = _context.Search;
        var groupColumn = _context.GroupColumn;
        var groupValue = _context.GroupValue;
        var playlistId = _context.PlaylistId;

        if (!string.IsNullOrWhiteSpace(requestedView))
        {
            if (!Views.Contains(requestedView)) throw new ArgumentException("Unknown Music Player view.");
            view = requestedView;
        }
        if (payload.TryGetProperty("search", out var searchValue))
            search = searchValue.ValueKind == JsonValueKind.String ? searchValue.GetString() ?? "" : "";
        else if (!string.IsNullOrWhiteSpace(requestedView))
            search = "";

        if (payload.TryGetProperty("group", out var group))
        {
            if (group.ValueKind == JsonValueKind.Object)
            {
                groupColumn = String(group, "column");
                groupValue = String(group, "name");
            }
            else
            {
                groupColumn = null;
                groupValue = null;
            }
        }
        else if (payload.TryGetProperty("groupColumn", out var groupColumnValue))
        {
            groupColumn = groupColumnValue.ValueKind == JsonValueKind.String ? groupColumnValue.GetString() : null;
            groupValue = String(payload, "groupValue");
        }
        else if (!string.IsNullOrWhiteSpace(requestedView))
        {
            groupColumn = null;
            groupValue = null;
        }

        if (payload.TryGetProperty("playlistId", out var playlistValue))
            playlistId = playlistValue.ValueKind == JsonValueKind.String ? playlistValue.GetString() : null;
        else if (!string.IsNullOrWhiteSpace(requestedView) && requestedView != "Playlist")
            playlistId = null;

        _context = new(view, search, groupColumn, groupValue, playlistId);
        PersistContext();
    }

    public void SetPlaylistContext(string id)
    {
        _context = _context with { View = "Playlist", PlaylistId = id };
        PersistContext();
    }

    public string SetGroupContext(string type, string value)
    {
        var column = GroupColumnFromType(type);
        var view = column switch { "album" => "Album", "artist" => "Artist", _ => "Genre" };
        _context = new(view, "", column, value, null);
        PersistContext();
        return column;
    }

    private void PersistContext()
    {
        var serialized = JsonSerializer.Serialize(new
        {
            view = _context.View,
            search = _context.Search,
            groupColumn = _context.GroupColumn,
            groupValue = _context.GroupValue,
            playlistId = _context.PlaylistId
        });
        if (!string.Equals(_store.GetSetting("last-view-context"), serialized, StringComparison.Ordinal))
            _store.SetSetting("last-view-context", serialized);
    }

    public IReadOnlyList<string> CurrentTrackPaths()
    {
        var playlistId = _context.PlaylistId;
        if (_context.View == "Playlist" && !string.IsNullOrWhiteSpace(playlistId))
            return _store.GetPlaylistPaths(playlistId);
        var filter = _context.View switch
        {
            "Favorites" => "favorites",
            "Most Played" => "most-played",
            "Recently Played" => "recent",
            "With Lyrics" => "with-lyrics",
            "Videos" => "videos",
            _ => null
        };
        return _store.GetTrackPaths(_context.View == "Search" ? _context.Search : string.Empty, _sort, _descending, filter,
            _context.GroupColumn, _context.GroupValue, hideExactDuplicates: HideExactDuplicates && _context.GroupColumn is null);
    }

    public IReadOnlyList<string> GroupTrackPaths(string column, string value) =>
        _store.GetTrackPaths(groupColumn: column, groupValue: value);

    public Track? ResolveTrack(string? id) =>
        ResolveTrackPath(id) is { } path ? _store.GetTrack(path) : null;

    public string? ResolveTrackPath(string? id) =>
        string.IsNullOrWhiteSpace(id) ? null : ResolveCachedTrackPath(id) ?? _store.ResolveTrackHandle(id);

    private string? ResolveCachedTrackPath(string id)
    {
        lock (_trackPathGate) return _trackPaths.TryGetValue(id, out var path) ? path : null;
    }

    public bool SetTrackUnavailable(string path, bool unavailable)
    {
        var fullPath = Path.GetFullPath(path);
        lock (_availabilityGate)
            {
                var changed = unavailable ? _unavailableTrackPaths.Add(fullPath) : _unavailableTrackPaths.Remove(fullPath);
                if (changed) RefreshBrowsingAvailability();
                return changed;
            }
    }

    public bool IsTrackUnavailable(string path)
    {
        var fullPath = Path.GetFullPath(path);
        lock (_availabilityGate) if (_unavailableTrackPaths.Contains(fullPath)) return true;
        lock (_rootAvailabilityGate) return _unavailableRootPaths.Any(root => fullPath.StartsWith(Path.EndsInDirectorySeparator(root) ? root : root + Path.DirectorySeparatorChar,
            OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal));
    }

    public void SetUnavailableRoots(IEnumerable<string> roots)
    {
        var normalizedRoots = roots.Select(NormalizeRootPath).ToArray();
        lock (_rootAvailabilityGate)
        {
            _unavailableRootPaths.Clear();
            foreach (var root in normalizedRoots) _unavailableRootPaths.Add(root);
        }
        RefreshBrowsingAvailability();
    }

    private void RefreshBrowsingAvailability()
    {
        // Always acquire these locks in the same order.
        lock (_availabilityGate) lock (_rootAvailabilityGate)
            _store.SetBrowsingAvailability(_unavailableRootPaths, _unavailableTrackPaths);
        InvalidateFolders();
    }

    public void ForgetDeletedPaths(IReadOnlyList<string> paths)
    {
        lock (_trackPathGate) foreach (var path in paths) _trackPaths.Remove(LibraryStore.TrackHandleId(path));
        lock (_availabilityGate) foreach (var path in paths) _unavailableTrackPaths.Remove(path);
        RefreshBrowsingAvailability();
    }

    public string[] ClearUnavailableTracksThatExist()
    {
        string[] candidates;
        lock (_availabilityGate) candidates = _unavailableTrackPaths.ToArray();
        var available = candidates.Where(File.Exists).ToArray();
        lock (_availabilityGate)
            foreach (var path in available) _unavailableTrackPaths.Remove(path);
        if (available.Length > 0) RefreshBrowsingAvailability();
        return available;
    }

    public string TrackId(string path)
    {
        var id = LibraryStore.TrackHandleId(path);
        if (_store.WasPurged(path)) return id;
        lock (_trackPathGate)
        {
            if (!_trackPaths.ContainsKey(id) && _trackPaths.Count >= 4096)
            {
                var evicted = _trackPaths.First();
                // Indexed handles already persist. Remember orphaned playlist/queue
                // entries before eviction so commands never depend on the resident page.
                _store.RememberTrackPath(evicted.Value);
                _trackPaths.Remove(evicted.Key);
            }
            _trackPaths[id] = path;
        }
        return id;
    }

    public string OpaqueId(string value) => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(value)))[..24];

    private static string NormalizeRootPath(string path)
    {
        try
        {
            var fullPath = Path.GetFullPath(path);
            return Path.TrimEndingDirectorySeparator(fullPath);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return path.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        }
    }

    public object TrackDto(Track track, bool includePath = false) => new
    {
        id = TrackId(track.Path), title = track.Title, artist = track.Artist, album = track.Album,
        albumArtist = track.AlbumArtist, genre = track.Genre, year = track.Year, trackNumber = track.TrackNumber,
        durationSeconds = track.Duration.TotalSeconds,
        addedDisplay = track.AddedUtc.ToLocalTime().ToString("d", CultureInfo.CurrentCulture),
        lastPlayedDisplay = track.LastPlayedUtc?.ToLocalTime().ToString("d", CultureInfo.CurrentCulture) ?? "",
        favorite = track.Favorite, rating = track.Rating, playCount = track.PlayCount,
        hasLyrics = track.HasLyrics,
        artworkUrl = ArtworkUrl(track.ArtworkPath), format = Path.GetExtension(track.Path).TrimStart('.').ToUpperInvariant(),
        isVideo = _videoExtensions.Contains(Path.GetExtension(track.Path)),
        unavailable = false, fileUnavailable = IsTrackUnavailable(track.Path),
        path = includePath ? track.Path : null
    };

    public object GroupDto(LibraryGroup group, string column) => new
    {
        id = group.Name, name = group.Name, title = group.Name, artist = group.Artist ?? "", albumArtist = group.Artist ?? "",
        trackCount = group.TrackCount, count = group.TrackCount, year = group.Year, column,
        artworkUrl = ArtworkUrl(group.ArtworkPath), meta = column == "album" ? group.Artist : $"{group.TrackCount} tracks"
    };

    public object PlaylistDto(PlaylistSummary playlist) => new
    { id = playlist.Id, name = playlist.Name, title = playlist.Name, trackCount = playlist.TrackCount, count = playlist.TrackCount, meta = $"{playlist.TrackCount} tracks" };

    public string? ArtworkUrl(string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return null;
        try
        {
            var full = Path.GetFullPath(path);
            var root = _artworkDirectory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (!full.StartsWith(root, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal)) return null;
            return $"https://artwork.musicplayer.local/{Uri.EscapeDataString(Path.GetFileName(full))}";
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or NotSupportedException) { return null; }
    }

    public void InvalidateFolders() { lock (_folderCacheGate) _folderCache = null; }

    private bool HideExactDuplicates => _store.GetSetting("hide-exact-duplicates") != "false";

    private object SearchTrackPage(string query, int offset, int size, CancellationToken cancellationToken)
    {
        var page = _store.GetTracksPageWithCount(query, TrackSort.Title, false, hideExactDuplicates: HideExactDuplicates,
            offset: offset, pageSize: size, cancellationToken: cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();
        return new { items = page.Tracks.Select(track => TrackDto(track)).ToArray(), totalCount = page.TotalCount };
    }

    private object SearchGroupPage(string column, string query, int offset, int size, CancellationToken cancellationToken)
    {
        var groups = _store.GetGroupsPage(column, query, offset, size, cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();
        var count = _store.CountGroups(column, query, cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();
        return new { items = groups.Select(group => GroupDto(group, column)).ToArray(), totalCount = count };
    }

    private object SearchPlaylistPage(string query, int offset, int size, CancellationToken cancellationToken)
    {
        var terms = query.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        var playlists = new List<PlaylistSummary>();
        foreach (var playlist in _store.GetPlaylistSummaries())
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (terms.All(term => playlist.Name.Contains(term, StringComparison.OrdinalIgnoreCase))) playlists.Add(playlist);
        }
        return new { items = playlists.Skip(offset).Take(size).Select(PlaylistDto).ToArray(), totalCount = playlists.Count };
    }

    private void DisposeSearchIfFinished(SearchBatch batch)
    {
        if (!batch.Retired || batch.ActiveRequests != 0) return;
        batch.Cancellation.Dispose();
        if (ReferenceEquals(_activeSearch, batch)) _activeSearch = null;
    }

    internal sealed record PreparedTrackPage(string View, string Search, string? GroupColumn, string? GroupValue,
        TrackSort Sort, bool Descending, string? Filter, int Offset, int PageSize);

    private T[] ReadJsonSetting<T>(string key, T[] fallback)
    {
        try { return _store.GetSetting(key) is { } json ? JsonSerializer.Deserialize<T[]>(json) ?? fallback : fallback; }
        catch (JsonException ex)
        {
            LocalAppLog.Shared.Warning("settings", $"Saved setting '{key}' was invalid JSON.", ex);
            return fallback;
        }
    }

    private static string GroupColumnFromType(string value) => value.ToLowerInvariant() switch
    { "album" => "album", "artist" => "artist", "genre" => "genre", "folder" => "folder", _ => throw new ArgumentException("Unknown library group.") };

    private static TrackSort ParseSort(string value) => Enum.TryParse<TrackSort>(value, true, out var sort) ? sort : TrackSort.Title;

    private static string String(JsonElement element, string key) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString() ?? "" : "";

    private static int Int(JsonElement element, string key, int fallback = 0) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(key, out var value) && value.TryGetInt32(out var result)
            ? result : fallback;

    private static long Int64(JsonElement element, string key, long fallback = 0) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(key, out var value) && value.TryGetInt64(out var result)
            ? result : fallback;

    private static bool Boolean(JsonElement element, string key) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.True;

    private sealed record ViewContext(string View, string Search, string? GroupColumn, string? GroupValue, string? PlaylistId);
}
