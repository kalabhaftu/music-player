namespace MusicPlayer.Core;

/// <summary>Bounds watcher bursts and throttles recovery after lost filesystem events.</summary>
public sealed class LibraryWatchBatch
{
    private readonly HashSet<string> _directories = new(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
    private bool _recoveryNeeded;
    private DateTimeOffset? _lastRecovery;
    public static readonly TimeSpan RecoveryInterval = TimeSpan.FromMinutes(5);

    public void AddDirectory(string path)
    {
        _directories.Add(Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)));
        if (_directories.Count > 512) RequestRecovery();
    }

    public void RequestRecovery()
    {
        _recoveryNeeded = true;
        _directories.Clear();
    }

    public (string[] Directories, bool FullScan, TimeSpan? RetryAfter) Drain(DateTimeOffset now)
    {
        TimeSpan? retryAfter = null;
        if (_recoveryNeeded)
        {
            if (_lastRecovery is { } previous && now - previous < RecoveryInterval)
                retryAfter = RecoveryInterval - (now - previous);
            else
            {
                _recoveryNeeded = false;
                _lastRecovery = now;
                _directories.Clear();
                return ([], true, null);
            }
        }
        var paths = _directories.OrderBy(path => path.Length).ToArray();
        _directories.Clear();
        var roots = new List<string>();
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        foreach (var path in paths)
        {
            if (roots.Any(root => string.Equals(path, root, comparison) ||
                path.StartsWith(Path.EndsInDirectorySeparator(root) ? root : root + Path.DirectorySeparatorChar, comparison))) continue;
            roots.Add(path);
        }
        return (roots.ToArray(), false, retryAfter);
    }
}
