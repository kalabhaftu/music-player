using System.Globalization;
using Microsoft.Data.Sqlite;

namespace MusicPlayer.Core;

/// <summary>Database-backed scan state. Unseen tracks are removed only when a complete root is committed.</summary>
public sealed class LibraryScanSession : IDisposable
{
    private const int SeenBatchSize = 256;
    private readonly SqliteConnection _connection;
    private readonly long _runId;
    private readonly StringComparer _pathComparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
    private readonly List<string> _pendingSeen = new(SeenBatchSize);
    private readonly List<(string Path, ScanExcludedPathKind? Kind)> _pendingProtected = new(SeenBatchSize);
    private readonly HashSet<string> _pendingSet;
    private readonly SqliteCommand _stateCommand;
    private readonly SqliteCommand _excludeCommand;
    private readonly SqliteCommand _defaultExcludeCommand;
    private readonly SqliteCommand _incompleteCommand;
    private bool _completed;
    private bool _disposed;

    internal LibraryScanSession(string connectionString, string[] roots)
    {
        _pendingSet = new HashSet<string>(_pathComparer);
        _connection = new SqliteConnection(connectionString);
        _connection.Open();

        using (var transaction = _connection.BeginTransaction())
        {
            using var cleanup = _connection.CreateCommand();
            cleanup.Transaction = transaction;
            cleanup.CommandText = """
                DELETE FROM scan_seen WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < $stale);
                DELETE FROM scan_excluded WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < $stale);
                DELETE FROM scan_default_excluded WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < $stale);
                DELETE FROM scan_incomplete_paths WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < $stale);
                DELETE FROM scan_roots WHERE run_id IN (SELECT id FROM scan_runs WHERE started_utc < $stale);
                DELETE FROM scan_runs WHERE started_utc < $stale;
                """;
            cleanup.Parameters.AddWithValue("$stale", DateTime.UtcNow.AddDays(-1).ToString("O", CultureInfo.InvariantCulture));
            cleanup.ExecuteNonQuery();

            using var insert = _connection.CreateCommand();
            insert.Transaction = transaction;
            insert.CommandText = "INSERT INTO scan_runs(started_utc) VALUES($started); SELECT last_insert_rowid();";
            insert.Parameters.AddWithValue("$started", DateTime.UtcNow.ToString("O", CultureInfo.InvariantCulture));
            _runId = Convert.ToInt64(insert.ExecuteScalar(), CultureInfo.InvariantCulture);

            using var addRoot = _connection.CreateCommand();
            addRoot.Transaction = transaction;
            addRoot.CommandText = "INSERT INTO scan_roots(run_id,root_path,root_prefix) VALUES($run,$root,$prefix)";
            var run = addRoot.Parameters.Add("$run", SqliteType.Integer);
            var root = addRoot.Parameters.Add("$root", SqliteType.Text);
            var prefix = addRoot.Parameters.Add("$prefix", SqliteType.Text);
            foreach (var path in roots)
            {
                run.Value = _runId;
                root.Value = path;
                prefix.Value = RootPrefix(path);
                addRoot.ExecuteNonQuery();
            }
            transaction.Commit();
        }

        _stateCommand = _connection.CreateCommand();
        _stateCommand.CommandText = OperatingSystem.IsWindows()
            ? "SELECT file_size,modified_utc,has_lyrics,artwork_path FROM tracks WHERE path COLLATE NOCASE=$path"
            : "SELECT file_size,modified_utc,has_lyrics,artwork_path FROM tracks WHERE path=$path";
        _stateCommand.Parameters.Add("$path", SqliteType.Text);
        _stateCommand.Prepare();
        _excludeCommand = _connection.CreateCommand();
        _excludeCommand.CommandText = "INSERT OR IGNORE INTO scan_excluded(run_id,path,prefix) VALUES($run,$path,$prefix)";
        _excludeCommand.Parameters.Add("$run", SqliteType.Integer).Value = _runId;
        _excludeCommand.Parameters.Add("$path", SqliteType.Text);
        _excludeCommand.Parameters.Add("$prefix", SqliteType.Text);
        _excludeCommand.Prepare();
        _defaultExcludeCommand = _connection.CreateCommand();
        _defaultExcludeCommand.CommandText = "INSERT OR IGNORE INTO scan_default_excluded(run_id,path,prefix) VALUES($run,$path,$prefix)";
        _defaultExcludeCommand.Parameters.Add("$run", SqliteType.Integer).Value = _runId;
        _defaultExcludeCommand.Parameters.Add("$path", SqliteType.Text);
        _defaultExcludeCommand.Parameters.Add("$prefix", SqliteType.Text);
        _defaultExcludeCommand.Prepare();
        _incompleteCommand = _connection.CreateCommand();
        _incompleteCommand.CommandText = "INSERT OR IGNORE INTO scan_incomplete_paths(run_id,path,prefix) VALUES($run,$path,$prefix)";
        _incompleteCommand.Parameters.Add("$run", SqliteType.Integer).Value = _runId;
        _incompleteCommand.Parameters.Add("$path", SqliteType.Text);
        _incompleteCommand.Parameters.Add("$prefix", SqliteType.Text);
        _incompleteCommand.Prepare();
    }

    public bool IsUnchanged(string path, long length, DateTime modifiedUtc, string artworkCache)
    {
        ThrowIfDisposed();
        var parameter = _stateCommand.Parameters["$path"];
        parameter.Value = Path.GetFullPath(path);
        using var reader = _stateCommand.ExecuteReader();
        if (!reader.Read()) return false;
        if (reader.GetInt64(0) != length ||
            DateTime.Parse(reader.GetString(1), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind) != modifiedUtc ||
            reader.GetInt64(2) < 0) return false;

        if (reader.IsDBNull(3)) return true;
        var artworkPath = reader.GetString(3);
        var cacheRoot = Path.TrimEndingDirectorySeparator(Path.GetFullPath(artworkCache)) + Path.DirectorySeparatorChar;
        var pathComparer = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        return artworkPath.StartsWith(cacheRoot, pathComparer) && File.Exists(artworkPath);
    }

    public void MarkSeen(string path)
    {
        ThrowIfDisposed();
        var fullPath = Path.GetFullPath(path);
        if (_pendingSet.Add(fullPath)) _pendingSeen.Add(fullPath);
        if (_pendingSeen.Count >= SeenBatchSize) FlushSeen();
    }

    public void MarkRootCompleted(string rootPath, bool complete)
    {
        ThrowIfDisposed();
        using var command = _connection.CreateCommand();
        command.CommandText = "UPDATE scan_roots SET completed=$complete WHERE run_id=$run AND root_path=$root";
        command.Parameters.AddWithValue("$complete", complete ? 1 : 0);
        command.Parameters.AddWithValue("$run", _runId);
        command.Parameters.AddWithValue("$root", NormalizeRoot(rootPath));
        command.ExecuteNonQuery();
    }

    /// <summary>Protects only a subtree whose contents could not be fully inspected.</summary>
    public void MarkPathIncomplete(string path)
    {
        ThrowIfDisposed();
        _pendingProtected.Add((NormalizeRoot(path), null));
        if (_pendingProtected.Count >= SeenBatchSize) FlushProtectedPaths();
    }

    /// <summary>Legacy exclusions are conservatively retained until a user removes them.</summary>
    public void MarkExcludedPath(string path) => MarkExcludedPath(path, ScanExcludedPathKind.UserIgnored);

    public void MarkExcludedPath(string path, ScanExcludedPathKind kind)
    {
        ThrowIfDisposed();
        _pendingProtected.Add((NormalizeRoot(path), kind));
        if (_pendingProtected.Count >= SeenBatchSize) FlushProtectedPaths();
    }

    public int Complete()
    {
        ThrowIfDisposed();
        FlushSeen();
        FlushProtectedPaths();
        using var transaction = _connection.BeginTransaction();
        var collation = OperatingSystem.IsWindows() ? " COLLATE NOCASE" : string.Empty;
        using var remove = _connection.CreateCommand();
        remove.Transaction = transaction;
        remove.CommandText = $"""
            DELETE FROM tracks
            WHERE EXISTS (
              SELECT 1 FROM scan_roots r
              WHERE r.run_id=$run AND r.completed=1
                AND (tracks.path{collation}=r.root_path OR
                     substr(tracks.path,1,length(r.root_prefix)){collation}=r.root_prefix))
              AND NOT EXISTS (
                SELECT 1 FROM scan_seen s
                WHERE s.run_id=$run AND tracks.path{collation}=s.path)
              AND NOT EXISTS (
                SELECT 1 FROM scan_excluded e
                WHERE e.run_id=$run AND
                  (tracks.path{collation}=e.path OR substr(tracks.path,1,length(e.prefix)){collation}=e.prefix))
              AND NOT EXISTS (
                SELECT 1 FROM scan_roots incomplete
                WHERE incomplete.run_id=$run AND incomplete.completed=0
                  AND (tracks.path{collation}=incomplete.root_path OR
                       substr(tracks.path,1,length(incomplete.root_prefix)){collation}=incomplete.root_prefix))
              AND NOT EXISTS (
                SELECT 1 FROM scan_incomplete_paths incomplete
                WHERE incomplete.run_id=$run
                  AND (tracks.path{collation}=incomplete.path OR
                       substr(tracks.path,1,length(incomplete.prefix)){collation}=incomplete.prefix))
              AND NOT EXISTS (
                SELECT 1 FROM scan_default_excluded e
                WHERE e.run_id=$run
                  AND (tracks.path{collation}=e.path OR substr(tracks.path,1,length(e.prefix)){collation}=e.prefix)
                  AND EXISTS (
                    SELECT 1 FROM scan_roots overlap
                    WHERE overlap.run_id=$run
                      AND (overlap.root_path{collation}=e.path OR
                           substr(overlap.root_path,1,length(e.prefix)){collation}=e.prefix)
                      AND (tracks.path{collation}=overlap.root_path OR
                           substr(tracks.path,1,length(overlap.root_prefix)){collation}=overlap.root_prefix)))
            """;
        remove.Parameters.AddWithValue("$run", _runId);
        var removed = remove.ExecuteNonQuery();

        using (var clearSeen = _connection.CreateCommand())
        {
            clearSeen.Transaction = transaction;
            clearSeen.CommandText = "DELETE FROM scan_seen WHERE run_id=$run";
            clearSeen.Parameters.AddWithValue("$run", _runId);
            clearSeen.ExecuteNonQuery();
        }
        using (var clearExcluded = _connection.CreateCommand())
        {
            clearExcluded.Transaction = transaction;
            clearExcluded.CommandText = "DELETE FROM scan_excluded WHERE run_id=$run";
            clearExcluded.Parameters.AddWithValue("$run", _runId);
            clearExcluded.ExecuteNonQuery();
        }
        using (var clearDefaultExcluded = _connection.CreateCommand())
        {
            clearDefaultExcluded.Transaction = transaction;
            clearDefaultExcluded.CommandText = "DELETE FROM scan_default_excluded WHERE run_id=$run";
            clearDefaultExcluded.Parameters.AddWithValue("$run", _runId);
            clearDefaultExcluded.ExecuteNonQuery();
        }
        using (var clearIncomplete = _connection.CreateCommand())
        {
            clearIncomplete.Transaction = transaction;
            clearIncomplete.CommandText = "DELETE FROM scan_incomplete_paths WHERE run_id=$run";
            clearIncomplete.Parameters.AddWithValue("$run", _runId);
            clearIncomplete.ExecuteNonQuery();
        }
        using (var clearRoots = _connection.CreateCommand())
        {
            clearRoots.Transaction = transaction;
            clearRoots.CommandText = "DELETE FROM scan_roots WHERE run_id=$run";
            clearRoots.Parameters.AddWithValue("$run", _runId);
            clearRoots.ExecuteNonQuery();
        }
        using (var clearRun = _connection.CreateCommand())
        {
            clearRun.Transaction = transaction;
            clearRun.CommandText = "DELETE FROM scan_runs WHERE id=$run";
            clearRun.Parameters.AddWithValue("$run", _runId);
            clearRun.ExecuteNonQuery();
        }
        transaction.Commit();
        _completed = true;
        return removed;
    }

    private void FlushProtectedPaths()
    {
        if (_pendingProtected.Count == 0) return;
        using var transaction = _connection.BeginTransaction();
        try
        {
            foreach (var item in _pendingProtected)
            {
                var command = item.Kind is null ? _incompleteCommand
                    : item.Kind == ScanExcludedPathKind.DefaultExcluded ? _defaultExcludeCommand : _excludeCommand;
                command.Transaction = transaction;
                command.Parameters["$path"].Value = item.Path;
                command.Parameters["$prefix"].Value = RootPrefix(item.Path);
                command.ExecuteNonQuery();
            }
            transaction.Commit();
            _pendingProtected.Clear();
        }
        finally
        {
            _incompleteCommand.Transaction = null;
            _excludeCommand.Transaction = null;
            _defaultExcludeCommand.Transaction = null;
        }
    }

    public void FlushSeen()
    {
        ThrowIfDisposed();
        if (_pendingSeen.Count == 0) return;
        using var transaction = _connection.BeginTransaction();
        using var command = _connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "INSERT OR IGNORE INTO scan_seen(run_id,path) VALUES($run,$path)";
        var run = command.Parameters.Add("$run", SqliteType.Integer);
        var path = command.Parameters.Add("$path", SqliteType.Text);
        command.Prepare();
        foreach (var item in _pendingSeen)
        {
            run.Value = _runId;
            path.Value = item;
            command.ExecuteNonQuery();
        }
        transaction.Commit();
        _pendingSeen.Clear();
        _pendingSet.Clear();
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _stateCommand.Dispose();
        _excludeCommand.Dispose();
        _defaultExcludeCommand.Dispose();
        _incompleteCommand.Dispose();
        if (!_completed)
        {
            try
            {
                using var transaction = _connection.BeginTransaction();
                foreach (var table in new[] { "scan_seen", "scan_excluded", "scan_default_excluded", "scan_incomplete_paths", "scan_roots", "scan_runs" })
                {
                    using var command = _connection.CreateCommand();
                    command.Transaction = transaction;
                    command.CommandText = table == "scan_runs" ? "DELETE FROM scan_runs WHERE id=$run" : $"DELETE FROM {table} WHERE run_id=$run";
                    command.Parameters.AddWithValue("$run", _runId);
                    command.ExecuteNonQuery();
                }
                transaction.Commit();
            }
            catch (SqliteException)
            {
                // A later scan removes this abandoned generation if cleanup could not complete.
            }
        }
        _connection.Dispose();
    }

    private void ThrowIfDisposed()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (_completed) throw new InvalidOperationException("The scan session is already complete.");
    }

    private static string NormalizeRoot(string path) => Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
    private static string RootPrefix(string root) => Path.EndsInDirectorySeparator(root) ? root : root + Path.DirectorySeparatorChar;
}
