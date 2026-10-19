using MusicPlayer.Core;
using Microsoft.UI.Xaml.Controls;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private readonly SemaphoreSlim _fileWatcherReconcileGate = new(1, 1);

    private async Task HandleLibraryPathsRemovedAsync(IReadOnlyList<string> paths)
    {
        await _fileWatcherReconcileGate.WaitAsync();
        try
        {
            var unavailable = await Task.Run(() => _trackAvailability.ReconcileRemovedPaths(paths));
            if (unavailable.Length == 0 || _windowClosed) return;

            DispatcherQueue.TryEnqueue(() =>
            {
                if (_windowClosed) return;
                foreach (var path in unavailable)
                    _webBridge?.SendEvent("trackAvailabilityChanged", new { id = LibraryStore.TrackHandleId(path), fileUnavailable = true });

                PublishLibraryChanged();
                PublishQueueState(force: true);
                var currentWasRemoved = _playback.CurrentTrack is { } current &&
                    unavailable.Any(path => SameTrack(path, current.Path));
                if (currentWasRemoved)
                {
                    PublishPlaybackState();
                    _ = ShowNoticeAsync("The current song was removed from its folder. Confirmed deletions are removed from saved playlists and the queue.", InfoBarSeverity.Warning);
                }
            });
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Warning("library-watch", "Could not reconcile removed library files. A rescan will retry.", ex);
            RequestLibraryWatcherRescan();
        }
        finally { _fileWatcherReconcileGate.Release(); }
    }

    private readonly HashSet<string> _pendingWatchDirectories = new(StringComparer.OrdinalIgnoreCase);
    private bool _pendingWatchFullScan;
    private DateTimeOffset _nextWatcherRecoveryUtc;
    private DateTimeOffset _nextWatcherScanUtc;
    private DateTimeOffset _watcherScansCancelledUntilUtc;

    private void RequestLibraryWatcherRescan() => RequestLibraryWatcherRescan([], true);

    private void RequestLibraryWatcherRescan(IReadOnlyList<string> directories, bool fullScan)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            if (_windowClosed) return;
            _pendingWatchFullScan |= fullScan;
            if (!_pendingWatchFullScan)
            {
                foreach (var directory in directories)
                {
                    _pendingWatchDirectories.Add(directory);
                    if (_pendingWatchDirectories.Count > 512) { _pendingWatchFullScan = true; break; }
                }
            }
            // A full configured-root scan supersedes these paths. Keep overflow
            // and cancellation delays from retaining an unbounded directory set.
            if (_pendingWatchFullScan) _pendingWatchDirectories.Clear();
            if (_libraryScan.IsRunning || _preparingLibraryScan || _playback.IsPlaying)
            {
                _libraryWatcherRescanPending = true;
                return;
            }
            if (DateTimeOffset.UtcNow < _watcherScansCancelledUntilUtc || DateTimeOffset.UtcNow < _nextWatcherScanUtc ||
                (_pendingWatchFullScan && DateTimeOffset.UtcNow < _nextWatcherRecoveryUtc))
            {
                _libraryWatcherRescanPending = true;
                return;
            }
            var roots = _pendingWatchFullScan ? _libraryLocations.GetLibraryRoots()
                : _pendingWatchDirectories.Where(Directory.Exists).ToArray();
            var reconcileWholeLibrary = _pendingWatchFullScan;
            if (roots.Length > 0) _nextWatcherScanUtc = DateTimeOffset.UtcNow.AddMinutes(5);
            if (reconcileWholeLibrary && roots.Length > 0) _nextWatcherRecoveryUtc = DateTimeOffset.UtcNow.AddMinutes(5);
            _pendingWatchDirectories.Clear();
            _pendingWatchFullScan = false;
            if (roots.Length > 0) StartScan(roots, reconcileWholeLibrary: reconcileWholeLibrary);
        });
    }

    private void RunPendingLibraryWatcherRescan()
    {
        if (!_libraryWatcherRescanPending || _windowClosed || _playback.IsPlaying) return;
        if (DateTimeOffset.UtcNow < _watcherScansCancelledUntilUtc || DateTimeOffset.UtcNow < _nextWatcherScanUtc ||
            (_pendingWatchFullScan && DateTimeOffset.UtcNow < _nextWatcherRecoveryUtc)) return;
        _libraryWatcherRescanPending = false;
        RequestLibraryWatcherRescan([], false);
    }
}
