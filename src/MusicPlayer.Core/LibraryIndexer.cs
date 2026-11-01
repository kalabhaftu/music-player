using System.Security;
using System.Diagnostics;

namespace MusicPlayer.Core;

public sealed record IndexResult(int Indexed, int Skipped, int Removed = 0, IReadOnlyList<string>? UnavailableRoots = null, IReadOnlyList<string>? IncompletePaths = null, int FilesFound = 0);

public sealed class LibraryIndexer(LibraryStore store, string artworkCache, LocalAppLog? log = null)
{
    private readonly LocalAppLog _log = log ?? LocalAppLog.Shared;

    public async Task<IndexResult> ScanAsync(
        IEnumerable<string> roots,
        IEnumerable<string> ignoredDirectories,
        ScanControl control,
        IProgress<ScanProgress>? progress = null,
        bool forceRefresh = false,
        IEnumerable<string>? supportedExtensions = null)
    {
        var pending = new List<Track>(64);
        var scanRoots = roots.Select(Path.GetFullPath).Distinct(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal).ToArray();
        // A scan is an explicit request to reconcile the current filesystem. Revalidate
        // cached content hashes as paths are encountered after a drive was unavailable.
        store.InvalidateExactFingerprintSnapshot();
        using var scan = store.BeginScan(scanRoots);
        var ignored = ignoredDirectories.Select(Path.GetFullPath).ToArray();
        var indexed = 0;
        var skipped = 0;
        var filesFound = 0;
        var directoriesVisited = 0;
        var sinceFlush = Stopwatch.StartNew();
        var unavailableRoots = new List<string>();
        var incompletePaths = new List<string>();
        void FlushPending()
        {
            if (pending.Count == 0) return;
            store.UpsertTracks(pending);
            indexed += pending.Count;
            var lastPath = pending[^1].Path;
            pending.Clear();
            sinceFlush.Restart();
            progress?.Report(new(filesFound, directoriesVisited, lastPath, indexed));
        }
        var scanner = new LibraryScanner(_log);
        try
        {
            await scanner.ScanAsync(scanRoots, ignored, control, (path, token) =>
            {
                token.ThrowIfCancellationRequested();
                filesFound++;
                var fullPath = Path.GetFullPath(path);
                try
                {
                    var info = new FileInfo(path);
                    if (!info.Exists)
                    {
                        if (MayStillExist(fullPath)) scan.MarkSeen(fullPath);
                        skipped++;
                        return ValueTask.CompletedTask;
                    }
                    if (!forceRefresh && scan.IsUnchanged(fullPath, info.Length, info.LastWriteTimeUtc, artworkCache))
                    {
                        scan.MarkSeen(fullPath);
                        return ValueTask.CompletedTask;
                    }
                    var track = TrackReader.Read(path, artworkCache);
                    scan.MarkSeen(fullPath);
                    pending.Add(track);
                    if (pending.Count >= 64) FlushPending();
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or TagLib.CorruptFileException or TagLib.UnsupportedFormatException or NotSupportedException or ArgumentException)
                {
                    // Keep an unreadable but still-present enabled media file in the index;
                    // if it disappeared during this scan, leave it unseen so Complete() prunes it.
                    var fallbackIndexed = false;
                    if (MayStillExist(fullPath))
                    {
                        scan.MarkSeen(fullPath);
                        try
                        {
                            var info = new FileInfo(fullPath);
                            pending.Add(new Track(fullPath, Path.GetFileNameWithoutExtension(fullPath), string.Empty, string.Empty,
                                string.Empty, string.Empty, 0, 0, TimeSpan.Zero, info.Length, info.LastWriteTimeUtc, DateTime.UtcNow));
                            if (pending.Count >= 64) FlushPending();
                            fallbackIndexed = true;
                        }
                        catch (Exception fallbackError) when (fallbackError is IOException or UnauthorizedAccessException or SecurityException or ArgumentException or NotSupportedException)
                        {
                            _log.Warning("indexer", "Could not keep basic file information for an enabled media file.", fallbackError);
                        }
                    }
                    if (!fallbackIndexed) skipped++;
                    _log.Warning("indexer", $"Could not read media tags for '{fullPath}'.", ex);
                }
                return ValueTask.CompletedTask;
            }, progress, directoryVisited: _ =>
                {
                    directoriesVisited++;
                    // Make a small newly discovered library usable immediately, then
                    // bound tail writes to once per second while drive discovery continues.
                    if (indexed == 0 || sinceFlush.Elapsed >= TimeSpan.FromSeconds(1)) FlushPending();
                },
                rootCompleted: (root, complete) => { FlushPending(); scan.MarkRootCompleted(root, complete); },
                rootUnavailable: root => unavailableRoots.Add(root),
                pathExcludedWithReason: scan.MarkExcludedPath,
                pathIncomplete: path => { scan.MarkPathIncomplete(path); incompletePaths.Add(path); },
                supportedExtensions: supportedExtensions).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (control.Token.IsCancellationRequested)
        {
            // Preserve the completed metadata reads even when the user cancels before
            // the normal 64-track batch threshold. Cancellation is not a database
            // failure; leaving the tail uncommitted made a fresh scan look empty.
            try { FlushPending(); }
            catch (Exception ex)
            {
                pending.Clear();
                _log.Warning("indexer", "Could not save the completed tracks accumulated before scan cancellation.", ex);
            }
            throw;
        }
        catch
        {
            // Keep the last committed batches. Do not try to flush an uncommitted tail
            // while unwinding: the original failure may have come from SQLite itself,
            // and a second write here would mask it and risk another partial mutation.
            pending.Clear();
            throw;
        }
        FlushPending();
        // Explicit rebuilds also repair already-indexed files that no longer sit under
        // today's configured roots. Normal scans cheaply revisit only broken artwork.
        var repaired = await RefreshIndexedFilesAsync(forceRefresh, scanRoots, control).ConfigureAwait(false);
        indexed += repaired;
        var removed = scan.Complete();
        if (removed > 0) store.InvalidateExactFingerprintSnapshot();
        if (indexed > 0 || removed > 0 || forceRefresh) store.PruneUnreferencedArtwork(artworkCache);
        var comparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
        return new(indexed, skipped, removed, unavailableRoots.Distinct(comparer).ToArray(), incompletePaths.Distinct(comparer).ToArray(), filesFound);
    }

    private async Task<int> RefreshIndexedFilesAsync(bool forceRefresh, IReadOnlyList<string> scanRoots, ScanControl control)
    {
        const int pageSize = 400;
        var changed = 0;
        var batch = new List<Track>(32);
        async Task RefreshOneAsync(Track indexedTrack)
        {
            control.Token.ThrowIfCancellationRequested();
            await control.WaitIfPausedAsync().ConfigureAwait(false);
            if (!File.Exists(indexedTrack.Path)) return;
            try
            {
                batch.Add(await Task.Run(() => TrackReader.Read(indexedTrack.Path, artworkCache), control.Token).ConfigureAwait(false));
                if (batch.Count >= 32)
                {
                    store.UpsertTracks(batch);
                    changed += batch.Count;
                    batch.Clear();
                }
            }
            catch (OperationCanceledException) { throw; }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or TagLib.CorruptFileException or TagLib.UnsupportedFormatException or NotSupportedException or ArgumentException)
            {
                _log.Warning("indexer", "Could not refresh metadata for an existing indexed track.", ex);
            }
        }

        if (forceRefresh)
        {
            // Traverse by immutable path so metadata/title updates do not shift later
            // pages. Keep each query and its metadata objects bounded for large libraries.
            for (var offset = 0; ; offset += pageSize)
            {
                control.Token.ThrowIfCancellationRequested();
                var page = store.GetTracksPage(sort: TrackSort.Path, offset: offset, pageSize: pageSize);
                foreach (var track in page)
                {
                    if (scanRoots.Any(root => IsWithinRoot(root, track.Path))) continue;
                    await RefreshOneAsync(track).ConfigureAwait(false);
                }
                if (page.Count < pageSize) break;
            }
        }
        else
        {
            // Normal scans only query bounded pages and reopen files whose cached cover
            // disappeared. Avoid one database connection per song in large libraries.
            foreach (var root in scanRoots)
            {
                for (var offset = 0; ; offset += pageSize)
                {
                    control.Token.ThrowIfCancellationRequested();
                    // A watcher update to one folder must not page through the whole library.
                    var page = store.GetTracksPage(groupColumn: "folder", groupValue: root,
                        sort: TrackSort.Path, offset: offset, pageSize: pageSize);
                    foreach (var track in page)
                    {
                        if (string.IsNullOrWhiteSpace(track.ArtworkPath) || File.Exists(track.ArtworkPath)) continue;
                        await RefreshOneAsync(track).ConfigureAwait(false);
                    }
                    if (page.Count < pageSize) break;
                }
            }
        }
        if (batch.Count > 0)
        {
            store.UpsertTracks(batch);
            changed += batch.Count;
        }
        return changed;
    }

    private static bool IsWithinRoot(string root, string path)
    {
        try
        {
            var relative = Path.GetRelativePath(root, path);
            return !Path.IsPathRooted(relative) && relative != ".." &&
                !relative.StartsWith(".." + Path.DirectorySeparatorChar, StringComparison.Ordinal) &&
                !relative.StartsWith(".." + Path.AltDirectorySeparatorChar, StringComparison.Ordinal);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { return false; }
    }

    private static bool MayStillExist(string path)
    {
        try { _ = File.GetAttributes(path); return true; }
        catch (Exception ex) when (ex is FileNotFoundException or DirectoryNotFoundException) { return false; }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException) { return true; }
    }
}
