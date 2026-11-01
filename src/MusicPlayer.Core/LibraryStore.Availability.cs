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
