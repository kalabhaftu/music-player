using System.Text.Json;
using Microsoft.Data.Sqlite;

namespace MusicPlayer.Core;

public sealed partial class LibraryStore
{
    private sealed record Visibility(string[] Roots, HashSet<string> Paths)
    {
        public bool Contains(string path) => Paths.Contains(path) || Roots.Any(root => path.StartsWith(root,
            OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal));
    }
    private volatile Visibility _hidden = new([], new(PathComparer));
    private static StringComparer PathComparer => OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
    private readonly object _sessionGate = new();
    private readonly HashSet<string> _purgedPaths = new(PathComparer);
    public bool UseCachedFingerprints { get; set; }

    public void SetBrowsingAvailability(IEnumerable<string> roots, IEnumerable<string> paths)
        => _hidden = new(roots.Select(root => RootPrefix(NormalizeRoot(root))).Distinct(PathComparer).ToArray(),
            paths.Select(Path.GetFullPath).ToHashSet(PathComparer));

    public bool WasPurged(string path) { lock (_sessionGate) return _purgedPaths.Contains(path); }
    public void ReinstateTrack(string path) { lock (_sessionGate) _purgedPaths.Remove(path); }
    public event Action<IReadOnlyList<string>>? TracksPurged;

    /// <summary>Immediately purges all indexed tracks that sit under any of the given excluded directories.</summary>
    public int PurgeTracksUnderExcludedDirectories(IEnumerable<string> excludedPaths)
    {
        var prefixes = excludedPaths
            .Where(p => !string.IsNullOrWhiteSpace(p))
            .Select(p => RootPrefix(NormalizeRoot(p)))
            .Distinct(PathComparer)
            .ToArray();
        if (prefixes.Length == 0) return 0;

        var pathsToPurge = new List<string>();
        lock (_sessionGate)
        {
            using var connection = Open();
            using var command = connection.CreateCommand();
            command.CommandText = "SELECT path FROM tracks";
            using var reader = command.ExecuteReader();
            var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
            while (reader.Read())
            {
                var trackPath = reader.GetString(0);
                if (prefixes.Any(prefix => trackPath.StartsWith(prefix, comparison)))
                {
                    pathsToPurge.Add(trackPath);
                }
            }
        }

        if (pathsToPurge.Count > 0)
        {
            PurgeDeletedTracks(pathsToPurge);
            InvalidateFingerprintState();
            _log.Info("library-store", $"Purged {pathsToPurge.Count} tracks under excluded directories.");
        }
        return pathsToPurge.Count;
    }

    /// <summary>Removes rows that were indexed by older builds under cache, build, or hidden
    /// application folders. New scans never enter those folders, so stale rows would otherwise
    /// remain in library counts and search results forever.</summary>
    public int PurgeTracksInDefaultExcludedDirectories()
    {
        var pathsToPurge = new List<string>();
        lock (_sessionGate)
        {
            using var connection = Open();
            using var command = connection.CreateCommand();
            command.CommandText = "SELECT path FROM tracks";
            using var reader = command.ExecuteReader();
            while (reader.Read())
            {
                var trackPath = reader.GetString(0);
                if (LibraryScanner.IsUnderDefaultExcludedDirectory(trackPath)) pathsToPurge.Add(trackPath);
            }
        }

        if (pathsToPurge.Count == 0) return 0;
        PurgeDeletedTracks(pathsToPurge);
        InvalidateFingerprintState();
        _log.Info("library-store", $"Purged {pathsToPurge.Count} stale tracks indexed under excluded cache or application folders.");
        return pathsToPurge.Count;
    }

    /// <summary>
    /// Scans indexed tracks for active folders on accessible storage that are not covered by any
    /// configured library root or exclusion, adopting them so their files are continuously tracked.
    /// </summary>
    public IReadOnlyList<string> DiscoverUnconfiguredMusicRoots(IEnumerable<string> configuredRoots, IEnumerable<string> ignoredDirectories)
    {
        var rootPrefixes = configuredRoots.Where(r => !string.IsNullOrWhiteSpace(r))
            .Select(r => RootPrefix(NormalizeRoot(r))).Distinct(PathComparer).ToArray();
        var ignoredPrefixes = ignoredDirectories.Where(i => !string.IsNullOrWhiteSpace(i))
            .Select(i => RootPrefix(NormalizeRoot(i))).Distinct(PathComparer).ToArray();
        var ignoredParentDirs = ignoredDirectories.Where(i => !string.IsNullOrWhiteSpace(i)).Select(i => { try { return Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(Path.GetFullPath(i))); } catch { return null; } }).Where(p => !string.IsNullOrEmpty(p)).ToHashSet(PathComparer);

        var candidateFolders = new HashSet<string>(PathComparer);
        lock (_sessionGate)
        {
            using var connection = Open();
            using var command = connection.CreateCommand();
            command.CommandText = "SELECT DISTINCT path FROM tracks";
            using var reader = command.ExecuteReader();
            var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
            while (reader.Read())
            {
                var trackPath = reader.GetString(0);
                if (rootPrefixes.Any(r => trackPath.StartsWith(r, comparison)))
                    continue;
                if (ignoredPrefixes.Any(i => trackPath.StartsWith(i, comparison)))
                    continue;

                var dir = Path.GetDirectoryName(trackPath);
                if (string.IsNullOrWhiteSpace(dir)) continue;

                string? musicRoot = null;
                var current = Path.TrimEndingDirectorySeparator(Path.GetFullPath(dir));
                while (!string.IsNullOrEmpty(current))
                {
                    var parent = Path.GetDirectoryName(current);
                    if (string.IsNullOrEmpty(parent) || string.Equals(parent, current, comparison)) break;
                    var name = Path.GetFileName(current);
                    if (name.Equals("Music", StringComparison.OrdinalIgnoreCase) || ignoredParentDirs.Contains(current))
                    {
                        musicRoot = current;
                        break;
                    }
                    current = Path.TrimEndingDirectorySeparator(parent);
                }

                if (musicRoot is not null && Directory.Exists(musicRoot))
                {
                    candidateFolders.Add(musicRoot);
                }
            }
        }
        return candidateFolders.ToArray();
    }

    /// <summary>
    /// Checks indexed tracks against disk. When a file is missing from an accessible drive or folder,
    /// it is purged immediately so missing tracks do not appear in queries or playback.
    /// </summary>
    public IReadOnlyList<string> ReconcileMissingTracks()
    {
        var pathsToPurge = new List<string>();
        lock (_sessionGate)
        {
            using var connection = Open();
            using var command = connection.CreateCommand();
            command.CommandText = "SELECT path FROM tracks";
            using var reader = command.ExecuteReader();
            while (reader.Read())
            {
                var trackPath = reader.GetString(0);
                if (string.IsNullOrWhiteSpace(trackPath)) continue;
                if (!File.Exists(trackPath))
                {
                    try
                    {
                        var dir = Path.GetDirectoryName(trackPath);
                        if ((!string.IsNullOrEmpty(dir) && Directory.Exists(dir)) ||
                            (Path.GetPathRoot(trackPath) is { } root && Directory.Exists(root)))
                        {
                            pathsToPurge.Add(trackPath);
                        }
                    }
                    catch
                    {
                        // Inaccessible path syntax or permission restriction; skip
                    }
                }
            }
        }

        if (pathsToPurge.Count > 0)
        {
            PurgeDeletedTracks(pathsToPurge);
            _log.Info("library-store", $"Reconciled {pathsToPurge.Count} missing tracks deleted from disk.");
        }
        return pathsToPurge;
    }

    /// <summary>Confirmed filesystem deletions remove user references as well as index rows.
    /// Does not touch media, tag backups, or recovery copies.</summary>
    public void PurgeDeletedTracks(IEnumerable<string> paths)
    {
        var removed = paths.Select(Path.GetFullPath).Distinct(PathComparer).ToArray();
        if (removed.Length == 0) return;
        lock (_sessionGate)
        {
            using var connection = Open();
            using var transaction = connection.BeginTransaction();
            using var command = connection.CreateCommand();
            command.Transaction = transaction;
            command.CommandText = "CREATE TEMP TABLE IF NOT EXISTS purge_paths(path TEXT PRIMARY KEY COLLATE " + (OperatingSystem.IsWindows() ? "NOCASE" : "BINARY") + "); DELETE FROM purge_paths; DROP TABLE IF EXISTS affected_playlists; DROP TABLE IF EXISTS compact_playlists;";
            command.ExecuteNonQuery();
            command.CommandText = "INSERT INTO purge_paths VALUES($path)";
            var parameter = command.Parameters.Add("$path", SqliteType.Text);
            foreach (var path in removed) { parameter.Value = path; command.ExecuteNonQuery(); }
            command.Parameters.Clear();
            command.CommandText = """
                CREATE TEMP TABLE affected_playlists AS SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_path IN (SELECT path FROM purge_paths);
                DELETE FROM tracks WHERE path IN (SELECT path FROM purge_paths);
                DELETE FROM track_interaction_state WHERE path IN (SELECT path FROM purge_paths);
                DELETE FROM track_handles WHERE path IN (SELECT path FROM purge_paths);
                DELETE FROM track_fingerprints WHERE path IN (SELECT path FROM purge_paths);
                DELETE FROM playlist_tracks WHERE track_path IN (SELECT path FROM purge_paths);
                CREATE TEMP TABLE compact_playlists AS
                  SELECT playlist_id,ROW_NUMBER() OVER(PARTITION BY playlist_id ORDER BY position)-1 AS position,track_path
                  FROM playlist_tracks WHERE playlist_id IN (SELECT playlist_id FROM affected_playlists);
                DELETE FROM playlist_tracks WHERE playlist_id IN (SELECT playlist_id FROM affected_playlists);
                INSERT INTO playlist_tracks SELECT playlist_id,position,track_path FROM compact_playlists;
                """;
            if (OperatingSystem.IsWindows()) command.CommandText = command.CommandText.Replace("path IN (", "path COLLATE NOCASE IN (", StringComparison.Ordinal);
            command.ExecuteNonQuery();
            command.CommandText = "SELECT payload FROM playback_session WHERE id=1";
            var payload = command.ExecuteScalar() as string;
            PlaybackSession? session = null;
            if (payload is not null)
            {
                try { session = JsonSerializer.Deserialize<PlaybackSession>(payload); }
                catch (JsonException ex) { _log.Warning("session", "Could not reconcile invalid session JSON; recovery copies remain intact.", ex); }
            }
            command.CommandText = "SELECT track_path FROM playback_queue ORDER BY position";
            var queue = new List<string>();
            using (var reader = command.ExecuteReader()) while (reader.Read()) queue.Add(reader.GetString(0));
            var deleted = removed.ToHashSet(PathComparer);
            if (session is not null)
            {
                session = RemoveSessionPaths(session with { Queue = queue }, deleted);
                ReplaceSessionQueue(connection, transaction, session.Queue);
                WriteSessionState(connection, transaction, session, create: false);
            }
            else ReplaceSessionQueue(connection, transaction, queue.Where(path => !deleted.Contains(path)).ToArray());
            transaction.Commit();
            foreach (var path in removed) _purgedPaths.Add(path);
        }
        InvalidateExactFingerprintSnapshot();
        TracksPurged?.Invoke(removed);
    }

    public IReadOnlyList<string> GetReferencedPaths()
    {
        using var connection = Open(); using var command = connection.CreateCommand();
        command.CommandText = "SELECT path FROM (SELECT track_path AS path FROM playlist_tracks UNION SELECT track_path FROM playback_queue UNION SELECT path FROM track_handles) references_to_tracks WHERE NOT EXISTS(SELECT 1 FROM tracks WHERE tracks.path=references_to_tracks.path)";
        var paths = new List<string>();
        using var reader = command.ExecuteReader();
        while (reader.Read()) paths.Add(reader.GetString(0));
        return paths;
    }

    private PlaybackSession SanitizeSession(PlaybackSession session, bool includeQueue)
        => includeQueue ? RemoveSessionPaths(session, _purgedPaths) :
            session.TrackPath is { } path && _purgedPaths.Contains(path) ? session with { TrackPath = null, PositionMilliseconds = 0 } : session;

    private static PlaybackSession RemoveSessionPaths(PlaybackSession session, ISet<string> deleted)
    {
        var queue = session.Queue.Where(path => !deleted.Contains(path)).ToArray();
        var index = session.Queue.Take(Math.Clamp(session.QueueIndex + 1, 0, session.Queue.Count)).Count(path => !deleted.Contains(path)) - 1;
        var currentDeleted = session.TrackPath is { } path && deleted.Contains(path);
        return session with { Queue = queue, QueueIndex = index, TrackPath = currentDeleted ? null : session.TrackPath,
            PositionMilliseconds = currentDeleted ? 0 : session.PositionMilliseconds,
            RepeatAMilliseconds = currentDeleted ? null : session.RepeatAMilliseconds, RepeatBMilliseconds = currentDeleted ? null : session.RepeatBMilliseconds };
    }
}
