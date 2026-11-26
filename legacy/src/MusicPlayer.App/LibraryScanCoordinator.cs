using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>Owns the active library scan and its pause, resume, and cancellation control.</summary>
internal sealed class LibraryScanCoordinator(LibraryIndexer indexer)
{
    private readonly object _gate = new();
    private ScanControl? _activeScan;
    private bool _paused;
    private bool _cancellationRequested;

    public bool IsRunning
    {
        get { lock (_gate) return _activeScan is not null; }
    }

    public (bool Active, bool Paused, bool Cancelling) GetStatus()
    {
        lock (_gate)
        {
            var active = _activeScan is not null;
            var cancelling = active && _cancellationRequested;
            return (active, active && _paused && !cancelling, cancelling);
        }
    }

    public Task<IndexResult?> StartAsync(
        IEnumerable<string> roots,
        IEnumerable<string> ignoredDirectories,
        IProgress<ScanProgress>? progress = null,
        bool forceRefresh = false,
        IEnumerable<string>? supportedExtensions = null)
    {
        var pathComparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        var scanRoots = roots.Select(Path.GetFullPath).Distinct(pathComparer).ToArray();
        if (scanRoots.Length == 0 && !forceRefresh) return Task.FromResult<IndexResult?>(null);

        ScanControl control;
        lock (_gate)
        {
            if (_activeScan is not null) return Task.FromResult<IndexResult?>(null);
            control = new ScanControl();
            _activeScan = control;
            _paused = false;
            _cancellationRequested = false;
        }

        return RunAsync(scanRoots, ignoredDirectories, control, progress, forceRefresh, supportedExtensions?.ToArray());
    }

    public bool TogglePause()
    {
        lock (_gate)
        {
            if (_activeScan is not { } control || _cancellationRequested) return false;
            if (_paused) control.Resume();
            else control.Pause();
            _paused = !_paused;
            return true;
        }
    }

    public bool Cancel()
    {
        lock (_gate)
        {
            if (_activeScan is not { } control || _cancellationRequested) return false;
            _cancellationRequested = true;
            control.Cancel();
            return true;
        }
    }

    private async Task<IndexResult?> RunAsync(
        string[] roots,
        IEnumerable<string> ignoredDirectories,
        ScanControl control,
        IProgress<ScanProgress>? progress,
        bool forceRefresh,
        IEnumerable<string>? supportedExtensions)
    {
        try
        {
            return await Task.Run(() => indexer.ScanAsync(roots, ignoredDirectories, control, progress, forceRefresh, supportedExtensions)).ConfigureAwait(false);
        }
        finally
        {
            lock (_gate)
            {
                if (ReferenceEquals(_activeScan, control))
                {
                    _activeScan = null;
                    _paused = false;
                    _cancellationRequested = false;
                }
                control.Dispose();
            }
        }
    }
}
