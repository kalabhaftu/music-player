using System.Diagnostics;
using System.Globalization;
using System.Numerics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json;
using MusicPlayer.Core;
using Microsoft.UI.Composition;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Windowing;
using Windows.Graphics.Imaging;
using Windows.Media;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.Storage.Streams;
using Windows.UI;
using Windows.UI.ViewManagement;
using WinRT.Interop;
using Microsoft.Data.Sqlite;

namespace MusicPlayer.App;

public sealed partial class MainWindow : Window
{
    private const string LibraryRootScopeSetting = "library-root-scope-version";
    private const string LibraryRootScopeVersion = "all-mounted-local-drives-v1";
    private readonly object _mountedDriveGate = new();
    private readonly HashSet<string> _knownMountedDriveRoots = new(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
    private Timer? _mountedDriveRefreshTimer;

    private int _duplicatePreparationActive;
    private async Task PrepareDuplicatesInBackgroundAsync()
    {
        if (Interlocked.Exchange(ref _duplicatePreparationActive, 1) != 0) return;
        try
        {
            await Task.Delay(1000);
            if (_windowClosed || _libraryScan.IsRunning) return;
            await Task.Run(() => _store.PrepareExactFingerprints());
            if (!_windowClosed) PublishLibraryChanged();
        }
        catch (Exception ex) { LocalAppLog.Shared.Warning("duplicates", "Background duplicate verification will retry after the next library scan.", ex); }
        finally { Volatile.Write(ref _duplicatePreparationActive, 0); }
    }

    private void StartStartupScan()
    {
        var roots = _libraryLocations.GetLibraryRoots().ToList();
        var rootsWereConfigured = _store.GetSetting("library-roots-configured") == "true";
        var rootsNeedExpansion = _store.GetSetting(LibraryRootScopeSetting) != LibraryRootScopeVersion;
        if (!rootsWereConfigured)
        {
            roots = LibraryScanner.DefaultRoots().ToList();
        }
        else if (rootsNeedExpansion)
        {
            // Older installs watched only the Music folder. Expand them once to every
            // mounted local drive, while keeping any explicitly configured external roots.
            roots.AddRange(LibraryScanner.DefaultRoots());
        }

        roots = NormalizeRootSet(roots).ToList();
        if (!rootsWereConfigured || rootsNeedExpansion)
        {
            _store.SetSetting("library-roots", JsonSerializer.Serialize(roots));
            _store.SetSetting("library-roots-configured", "true");
            _store.SetSetting(LibraryRootScopeSetting, LibraryRootScopeVersion);
        }
        int? libraryTracks = null;
        try { libraryTracks = _store.GetLibraryStats().TotalTracks; }
        catch (SqliteException ex) when (LibraryStore.IsDatabaseCorruption(ex))
        { LocalAppLog.Shared.Error("database", "Could not read the index count before the startup scan; the scan will rebuild the damaged index if needed.", ex); }
        LocalAppLog.Shared.Info("scanner", $"Startup scan prepared: scope=all-mounted-local-drives, configuredRoots={roots.Count}, rootsPreviouslyConfigured={rootsWereConfigured}, indexedTracks={libraryTracks?.ToString(CultureInfo.InvariantCulture) ?? "unavailable"}, roots=[{string.Join(", ", roots.Select(DescribeRoot))}].");
        _libraryFileWatcher.SetRoots(roots, _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
        StartMountedDriveRefresh(roots);
        if (roots.Count > 0) StartScan(roots);
        else _ = ReconcileIndexedLibraryAtStartupAsync();
    }

    private void StartMountedDriveRefresh(IEnumerable<string> initialRoots)
    {
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        lock (_mountedDriveGate)
        {
            _knownMountedDriveRoots.Clear();
            foreach (var root in LibraryScanner.DefaultRoots()) _knownMountedDriveRoots.Add(Path.GetFullPath(root));
            foreach (var root in initialRoots)
                if (Path.GetPathRoot(root) is { } driveRoot && comparer.Equals(root, driveRoot)) _knownMountedDriveRoots.Add(Path.GetFullPath(root));
        }

        _mountedDriveRefreshTimer?.Dispose();
        _mountedDriveRefreshTimer = new Timer(_ => DispatcherQueue.TryEnqueue(RefreshMountedDriveRoots), null,
            TimeSpan.FromSeconds(15), TimeSpan.FromSeconds(15));
    }

    private void RefreshMountedDriveRoots()
    {
        if (_windowClosed || _store.GetSetting(LibraryRootScopeSetting) != LibraryRootScopeVersion) return;
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        var available = LibraryScanner.DefaultRoots().Select(Path.GetFullPath).ToArray();
        string[] newlyMounted;
        bool mountsChanged;
        lock (_mountedDriveGate)
        {
            mountsChanged = !_knownMountedDriveRoots.SetEquals(available);
            newlyMounted = available.Where(root => !_knownMountedDriveRoots.Contains(root)).ToArray();
            _knownMountedDriveRoots.Clear();
            foreach (var root in available) _knownMountedDriveRoots.Add(root);
        }
        if (mountsChanged)
        {
            _libraryQueries.SetUnavailableRoots(_libraryLocations.GetLibraryRoots().Where(root => !Directory.Exists(root)));
            PublishLibraryChanged();
        }
        if (newlyMounted.Length == 0) return;

        var savedRoots = _libraryLocations.GetLibraryRoots();
        var roots = NormalizeRootSet(savedRoots.Concat(newlyMounted));
        if (!savedRoots.OrderBy(path => path, comparer).SequenceEqual(roots.OrderBy(path => path, comparer), comparer))
            _store.SetSetting("library-roots", JsonSerializer.Serialize(roots));
        _libraryFileWatcher.SetRoots(roots, _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
        LocalAppLog.Shared.Info("scanner", $"New local storage mounted: roots=[{string.Join(", ", newlyMounted.Select(DescribeRoot))}]; starting an automatic scan.");
        if (_libraryScan.IsRunning) RequestLibraryWatcherRescan(newlyMounted, false);
        else StartScan(newlyMounted);
    }

    private void StopMountedDriveRefresh()
    {
        _mountedDriveRefreshTimer?.Dispose();
        _mountedDriveRefreshTimer = null;
    }

    private async Task ReconcileIndexedLibraryAtStartupAsync()
    {
        try
        {
            var result = await Task.Run(() => _trackAvailability.ReconcileIndexedTracks());
            if (_windowClosed) return;
            DispatcherQueue.TryEnqueue(() =>
            {
                if (_windowClosed) return;
                foreach (var path in result.RemovedPaths)
                    _webBridge?.SendEvent("trackAvailabilityChanged", new { id = LibraryStore.TrackHandleId(path), fileUnavailable = true });
                PublishLibraryChanged();
            });
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Warning("library-availability", "Startup could not reconcile indexed file availability.", ex);
        }
    }

    private static string[] NormalizeRootSet(IEnumerable<string> suppliedRoots)
    {
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        var roots = new List<string>();
        foreach (var suppliedRoot in suppliedRoots)
        {
            try
            {
                var normalized = Path.GetFullPath(suppliedRoot);
                if (!roots.Contains(normalized, comparer)) roots.Add(normalized);
            }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
            {
                LocalAppLog.Shared.Warning("library-roots", "Ignored an invalid saved scan root.", ex);
            }
        }

        // Scanning a drive root already covers any saved child roots beneath it.
        return roots.Where(candidate => !roots.Any(parent =>
            !comparer.Equals(candidate, parent) && IsWithinRoot(candidate, parent))).ToArray();
    }

    private static bool IsWithinRoot(string path, string root)
    {
        var normalizedRoot = Path.GetFullPath(root);
        var prefix = Path.EndsInDirectorySeparator(normalizedRoot)
            ? normalizedRoot
            : normalizedRoot + Path.DirectorySeparatorChar;
        return path.StartsWith(prefix, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);
    }


    private async void StartScan(IEnumerable<string> roots, bool forceRefresh = false, bool databaseRecoveryAttempted = false,
        bool reconcileWholeLibrary = true)
    {
        if (_libraryScan.IsRunning)
        {
            LocalAppLog.Shared.Info("scanner", "Ignored a scan request because another library scan is already running.");
            return;
        }
        var scanRoots = roots.Select(Path.GetFullPath).Distinct(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal).ToArray();
        // An explicit rebuild with no currently configured roots still refreshes
        // the indexed paths. Ordinary scans continue to require a selected root.
        if (scanRoots.Length == 0 && !forceRefresh) return;
        _scanStartedUtc = DateTimeOffset.UtcNow;
        ClearScanOutcome();
        _scanCurrentPath = "Preparing scan…";
        _scanFilesFound = 0;
        _scanDirectoriesVisited = 0;
        try
        {
            var indexedBefore = _store.GetLibraryStats().TotalTracks;
            LocalAppLog.Shared.Info("scanner", $"Starting {(forceRefresh ? "index rebuild" : "library scan")}: roots={scanRoots.Length}, indexedTracksBefore={indexedBefore}, roots=[{string.Join(", ", scanRoots.Select(DescribeRoot))}].");
            var ignored = _libraryLocations.GetScanExclusions();
            var publishedIndexed = 0;
            var committedIndexed = 0;
            var nextLibraryRefreshUtc = DateTimeOffset.MinValue;
            var progress = new Progress<ScanProgress>(value =>
            {
                if (_windowClosed) return;
                _scanFilesFound = value.FilesFound;
                _scanDirectoriesVisited = value.DirectoriesVisited;
                if (!string.IsNullOrWhiteSpace(value.CurrentPath)) _scanCurrentPath = value.CurrentPath;
                if (DateTimeOffset.UtcNow - _lastWebScanUpdateUtc >= TimeSpan.FromMilliseconds(350))
                { _lastWebScanUpdateUtc = DateTimeOffset.UtcNow; PublishScanState(); }
                committedIndexed = Math.Max(committedIndexed, value.IndexedTracks);
                if (committedIndexed > publishedIndexed && DateTimeOffset.UtcNow >= nextLibraryRefreshUtc)
                {
                    PublishLibraryChanged();
                    publishedIndexed = committedIndexed;
                    nextLibraryRefreshUtc = DateTimeOffset.UtcNow.AddSeconds(1);
                }
            });
            var scanTask = _libraryScan.StartAsync(scanRoots, ignored, progress, forceRefresh, _libraryLocations.GetEnabledExtensions());
            if (!_libraryScan.IsRunning) return;
            PublishScanState();
            var result = await scanTask;
            if (!_windowClosed)
            {
                if (result is not null)
                {
                    _scanCurrentPath = "Scan complete";
                    var unavailableRoots = result.UnavailableRoots ?? Array.Empty<string>();
                    if (reconcileWholeLibrary) _libraryQueries.SetUnavailableRoots(
                        _libraryLocations.GetLibraryRoots().Where(root => !Directory.Exists(root))
                            .Concat(unavailableRoots).Concat(result.IncompletePaths ?? Array.Empty<string>()));
                    if (unavailableRoots.Count > 0)
                    {
                        _trackAvailability.MarkUnavailableRoots(unavailableRoots);
                        var unavailableIds = new HashSet<string>(StringComparer.Ordinal);
                        if (_playback.CurrentTrack is { } current && _libraryQueries.IsTrackUnavailable(current.Path))
                            unavailableIds.Add(_libraryQueries.TrackId(current.Path));
                        foreach (var path in _queue)
                            if (_libraryQueries.IsTrackUnavailable(path)) unavailableIds.Add(_libraryQueries.TrackId(path));
                        foreach (var id in unavailableIds)
                            _webBridge?.SendEvent("trackAvailabilityChanged", new { id, fileUnavailable = true });
                    }
                    foreach (var path in _trackAvailability.ClearUnavailableTracksThatExist())
                        _webBridge?.SendEvent("trackAvailabilityChanged", new { id = LibraryStore.TrackHandleId(path), fileUnavailable = false });

                    // Reconcile every indexed path after a successful scan. This also
                    // catches files deleted before the watcher started or outside the
                    // currently selected roots, while the availability service retains
                    // references whose containing drive or folder cannot be reached.
                    var reconciliation = await Task.Run(() => _trackAvailability.ReconcileIndexedTracks(
                        unavailableRoots, result.IncompletePaths ?? Array.Empty<string>(), reconcileWholeLibrary ? null : scanRoots));
                    foreach (var path in reconciliation.RemovedPaths)
                        _webBridge?.SendEvent("trackAvailabilityChanged", new { id = LibraryStore.TrackHandleId(path), fileUnavailable = true });
                    foreach (var path in _trackAvailability.ClearUnavailableTracksThatExist())
                        _webBridge?.SendEvent("trackAvailabilityChanged", new { id = LibraryStore.TrackHandleId(path), fileUnavailable = false });

                    var heading = forceRefresh ? "Index rebuilt" : "Scan complete";
                    var removedCount = result.Removed + reconciliation.RemovedPaths.Count;
                    var message = $"{heading} · {result.Indexed:N0} tracks updated · {removedCount:N0} missing tracks removed · {result.Skipped:N0} files skipped";
                    var indexedAfter = _store.GetLibraryStats().TotalTracks;
                    LocalAppLog.Shared.Info("scanner", $"Completed {(forceRefresh ? "index rebuild" : "library scan")}: roots={scanRoots.Length}, supportedMediaFilesFound={result.FilesFound}, tracksUpdated={result.Indexed}, tracksRemoved={removedCount}, filesSkipped={result.Skipped}, indexedTracksAfter={indexedAfter}, unavailableRoots={unavailableRoots.Count}, incompleteFolders={result.IncompletePaths?.Count ?? 0}.");
                    if (result.FilesFound == 0 && scanRoots.Length > 0)
                    {
                        message += indexedAfter > 0
                            ? $" · No enabled media files were found in the selected folders; {indexedAfter:N0} indexed tracks were preserved. Check the selected file extensions and folders."
                            : " · No enabled media files were found in the selected folders. Check the selected file extensions and folders.";
                    }
                    if (reconciliation.UnavailableCount > 0)
                        message += $" · {reconciliation.UnavailableCount:N0} tracks retained because their locations are unavailable";
                    if (unavailableRoots.Count > 0)
                        message += $" · unavailable folders (tracks kept): {string.Join("; ", unavailableRoots)}";
                    if (result.IncompletePaths is { Count: > 0 } incompletePaths)
                        message += $" · {incompletePaths.Count:N0} folders could not be checked; their indexed tracks were kept";
                    if (reconciliation.RemovedPaths.Count > 0)
                        LocalAppLog.Shared.Info("library-availability", $"Reconciled {reconciliation.RemovedPaths.Count:N0} confirmed missing indexed track(s); retained {reconciliation.UnavailableCount:N0} unreachable track(s).");
                    SetScanOutcome("complete", message);
                }
                PublishLibraryChanged(); PublishScanState();
                _ = PrepareDuplicatesInBackgroundAsync();
            }
        }
        catch (OperationCanceledException)
        {
            LocalAppLog.Shared.Info("scanner", "Library scan cancelled; completed tracks were retained.");
            SetScanOutcome("cancelled", "Scan cancelled · tracks already indexed were kept");
            if (!_windowClosed) { _scanCurrentPath = "Scan cancelled"; PublishScanState(); }
        }
        catch (SqliteException ex) when (LibraryStore.IsDatabaseCorruption(ex))
        {
            LocalAppLog.Shared.Error("scanner", "The library database became unreadable during scanning.", ex);
            if (databaseRecoveryAttempted)
            {
                SetScanOutcome("error", "The library database failed again during its automatic rebuild. Diagnostic details were saved to the app log.");
                if (!_windowClosed)
                {
                    _scanCurrentPath = "Database recovery stopped";
                    PublishScanState();
                    _ = ShowNoticeAsync("The library database failed again while rebuilding. Your previous database copy is saved in the Music Player Recovery folder; see the app log for details.", InfoBarSeverity.Error);
                }
            }
            else
            {
                try
                {
                    if (!await Task.Run(() => _store.RepairCorruptDatabase(force: true)))
                        throw new InvalidDataException("The damaged library database could not be replaced with a clean index.");
                    LocalAppLog.Shared.Warning("database", "Recreated the damaged index from a clean schema; saved settings and playlists were salvaged when readable.");
                    if (!_windowClosed)
                    {
                        SetScanOutcome("repairing", "Library database repaired · rebuilding the index from your music folders");
                        var recoveryRoots = NormalizeRootSet(_libraryLocations.GetLibraryRoots().Concat(LibraryScanner.DefaultRoots()));
                        StartScan(recoveryRoots, forceRefresh: true, databaseRecoveryAttempted: true);
                        return;
                    }
                }
                catch (Exception recoveryError)
                {
                    LocalAppLog.Shared.Error("database", "Automatic library database recovery failed.", recoveryError);
                    SetScanOutcome("error", "The library database could not be rebuilt. Diagnostic details were saved to the app log.");
                    if (!_windowClosed)
                    {
                        _scanCurrentPath = "Database recovery failed";
                        PublishScanState();
                        _ = ShowNoticeAsync("Music Player could not rebuild the damaged library database. The original database was preserved in the Music Player Recovery folder; see the app log for details.", InfoBarSeverity.Error);
                    }
                }
            }
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("scanner", "Library scan failed.", ex);
            SetScanOutcome("error", $"Scan failed · {ex.Message}");
            if (!_windowClosed) { _scanCurrentPath = "Scan stopped"; PublishScanState(); _ = ShowNoticeAsync($"The library scan failed: {ex.Message}", InfoBarSeverity.Error); }
        }
        finally
        {
            _ = _store.CheckpointWalAsync();
            if (!_windowClosed)
            {
                PublishLibraryChanged(); PublishScanState();
                RunPendingLibraryWatcherRescan();
            }
        }
    }

    private void ClearScanOutcome()
    {
        _scanOutcomeKind = null;
        _scanOutcomeMessage = null;
        _scanOutcomeUtc = null;
    }

    private void SetScanOutcome(string kind, string message)
    {
        _scanOutcomeKind = kind;
        _scanOutcomeMessage = message;
        _scanOutcomeUtc = DateTimeOffset.UtcNow;
    }

    private static string DescribeRoot(string path)
    {
        var label = Path.GetFileName(Path.TrimEndingDirectorySeparator(path));
        if (string.IsNullOrWhiteSpace(label)) label = Path.GetPathRoot(path) ?? "root";
        return $"{label} ({(Directory.Exists(path) ? "available" : "missing")})";
    }
}
