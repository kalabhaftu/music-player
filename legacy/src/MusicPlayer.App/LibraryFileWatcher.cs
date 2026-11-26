using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>Watches configured music folders for changes so the index follows the filesystem.</summary>
internal sealed class LibraryFileWatcher : IDisposable
{
    private readonly object _gate = new();
    private readonly HashSet<string> _pendingRemoved = new(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
    private readonly List<FileSystemWatcher> _watchers = [];
    private readonly Timer _flushTimer;
    private readonly Timer _rescanTimer;
    private readonly LibraryWatchBatch _changes = new();
    private string[] _ignoredDirectories = [];
    private bool _disposed;

    public event Action<IReadOnlyList<string>>? PathsRemoved;
    public event Action<IReadOnlyList<string>, bool>? RescanRequested;

    public LibraryFileWatcher()
    {
        _flushTimer = new Timer(_ => FlushRemoved(), null, Timeout.Infinite, Timeout.Infinite);
        _rescanTimer = new Timer(_ => FlushChanges(), null, Timeout.Infinite, Timeout.Infinite);
    }

    public void SetRoots(IEnumerable<string> roots, IEnumerable<string>? ignoredDirectories = null, IEnumerable<string>? supportedExtensions = null)
    {
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        var mediaExtensions = (supportedExtensions ?? LibraryScanner.AudioExtensions).ToHashSet(StringComparer.OrdinalIgnoreCase);
        bool IsSupportedFile(string path) => mediaExtensions.Contains(Path.GetExtension(path));
        var ignored = (ignoredDirectories ?? Array.Empty<string>())
            .Select(path => Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)))
            .Distinct(comparer)
            .ToArray();
        lock (_gate)
        {
            if (_disposed) return;
            _ignoredDirectories = ignored;
            foreach (var watcher in _watchers) watcher.Dispose();
            _watchers.Clear();

            foreach (var root in roots.Select(Path.GetFullPath).Distinct(comparer))
            {
                if (!Directory.Exists(root)) continue;
                try
                {
                    var watcher = new FileSystemWatcher(root)
                    {
                        IncludeSubdirectories = true,
                        NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName | NotifyFilters.LastWrite | NotifyFilters.Size,
                        InternalBufferSize = 32 * 1024,
                        EnableRaisingEvents = false
                    };
                    watcher.Deleted += (_, args) =>
                    {
                        if (!IsExcludedPath(args.FullPath, root)) QueueRemoved(args.FullPath);
                    };
                    watcher.Created += (_, args) =>
                    {
                        // Moving or copying a populated directory does not emit a
                        // Created event for every file in its subtree.
                        if (!IsExcludedPath(args.FullPath, root) &&
                            (IsSupportedFile(args.FullPath) || Directory.Exists(args.FullPath))) QueueRescan(args.FullPath);
                    };
                    watcher.Changed += (_, args) =>
                    {
                        if (!IsExcludedPath(args.FullPath, root) && IsSupportedFile(args.FullPath)) QueueRescan(args.FullPath);
                    };
                    watcher.Renamed += (_, args) =>
                    {
                        if (!IsExcludedPath(args.OldFullPath, root)) QueueRemoved(args.OldFullPath);
                        if (!IsExcludedPath(args.FullPath, root) &&
                            (IsSupportedFile(args.FullPath) || Directory.Exists(args.FullPath))) QueueRescan(args.FullPath);
                    };
                    watcher.Error += (_, args) =>
                    {
                        LocalAppLog.Shared.Warning("library-watch", $"A library folder change could not be observed under '{root}'.", args.GetException());
                        QueueRescan();
                    };
                    watcher.EnableRaisingEvents = true;
                    _watchers.Add(watcher);
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
                {
                    LocalAppLog.Shared.Warning("library-watch", $"Could not watch library folder '{root}'. Startup and manual scans still reconcile it.", ex);
                }
            }
        }
    }

    private bool IsExcludedPath(string path, string watchRoot)
    {
        string fullPath;
        try { fullPath = Path.GetFullPath(path); }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { return true; }

        var current = Directory.Exists(fullPath) ? fullPath : Path.GetDirectoryName(fullPath);
        var root = Path.TrimEndingDirectorySeparator(Path.GetFullPath(watchRoot));
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        while (!string.IsNullOrWhiteSpace(current))
        {
            var normalized = Path.TrimEndingDirectorySeparator(Path.GetFullPath(current));
            if (_ignoredDirectories.Contains(normalized, OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal)) return true;
            if (!string.Equals(normalized, root, comparison) && IsWithinRoot(normalized, root) &&
                LibraryScanner.IsDefaultExcludedDirectory(normalized)) return true;
            if (string.Equals(normalized, root, comparison)) break;
            var parent = Path.GetDirectoryName(normalized);
            if (string.IsNullOrEmpty(parent) || string.Equals(parent, normalized, comparison)) break;
            current = parent;
        }
        return false;
    }

    private static bool IsWithinRoot(string path, string root)
    {
        var prefix = Path.EndsInDirectorySeparator(root) ? root : root + Path.DirectorySeparatorChar;
        return path.StartsWith(prefix, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);
    }

    private void QueueRemoved(string path)
    {
        lock (_gate)
        {
            if (_disposed) return;
            try { _pendingRemoved.Add(Path.GetFullPath(path)); }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { return; }
            _flushTimer.Change(450, Timeout.Infinite);
        }
    }

    private void QueueRescan(string? path = null)
    {
        lock (_gate)
        {
            if (_disposed) return;
            if (path is null) _changes.RequestRecovery();
            else _changes.AddDirectory(Directory.Exists(path) ? path : Path.GetDirectoryName(path)!);
            _rescanTimer.Change(30_000, Timeout.Infinite);
        }
    }

    private void FlushChanges()
    {
        (string[] Directories, bool FullScan, TimeSpan? RetryAfter) batch;
        lock (_gate)
        {
            if (_disposed) return;
            batch = _changes.Drain(DateTimeOffset.UtcNow);
            if (batch.RetryAfter is { } delay) _rescanTimer.Change(delay, Timeout.InfiniteTimeSpan);
        }
        if (batch.FullScan || batch.Directories.Length > 0) RescanRequested?.Invoke(batch.Directories, batch.FullScan);
    }

    private void FlushRemoved()
    {
        string[] paths;
        lock (_gate)
        {
            if (_disposed || _pendingRemoved.Count == 0) return;
            paths = _pendingRemoved.ToArray();
            _pendingRemoved.Clear();
        }
        PathsRemoved?.Invoke(paths);
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
            _flushTimer.Dispose();
            _rescanTimer.Dispose();
            foreach (var watcher in _watchers) watcher.Dispose();
            _watchers.Clear();
            _pendingRemoved.Clear();
        }
    }
}
