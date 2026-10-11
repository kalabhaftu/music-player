using System.Security;
using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>Distinguishes removed files from temporarily unreachable library locations.</summary>
internal sealed class TrackAvailabilityService(
    LibraryStore store,
    LibraryQueryService queries,
    LibraryLocationService locations,
    string artworkDirectory)
{
    public AvailabilityReconciliation ReconcileIndexedTracks(IEnumerable<string>? unavailableRoots = null, IEnumerable<string>? incompletePaths = null,
        IEnumerable<string>? scanRoots = null)
    {
        const int pageSize = 800;
        var removed = new List<string>();
        var unavailable = 0;
        var recovered = 0;
        var protectedPrefixes = (unavailableRoots ?? Array.Empty<string>())
            .Concat(incompletePaths ?? Array.Empty<string>())
            .Select(TryNormalizePrefix).Where(prefix => prefix is not null).Cast<string>().ToArray();
        var scopes = scanRoots?.Cast<string?>().ToArray() ?? [null];
        foreach (var scope in scopes)
        {
          var offset = 0;
          while (true)
          {
            var paths = store.GetTrackPaths(groupColumn: scope is null ? null : "folder", groupValue: scope, offset: offset, pageSize: pageSize, includeUnavailable: true);
            if (paths.Count == 0) break;
            var removedFromPage = new List<string>();
            foreach (var path in paths)
            {
                if (protectedPrefixes.Any(prefix => IsAncestorDirectory(prefix, path))) { unavailable++; continue; }
                if (File.Exists(path))
                {
                    store.ReinstateTrack(path);
                    if (queries.SetTrackUnavailable(path, false)) recovered++;
                    continue;
                }

                queries.SetTrackUnavailable(path, true);
                if (!protectedPrefixes.Any(prefix => IsAncestorDirectory(prefix, path)) && IsConfirmedMissingFile(path)) removedFromPage.Add(path);
                else unavailable++;
            }

            if (removedFromPage.Count > 0)
            {
                store.PurgeDeletedTracks(removedFromPage);
                removed.AddRange(removedFromPage);
                // Deletions collapse the ordered result set; revisit this page's offset
                // so the shifted tracks are not skipped.
            }
            else
            {
                offset += paths.Count;
                if (paths.Count < pageSize) break;
            }
          }
        }
        // Include saved references whose index row was pruned during scanning.
        var orphaned = store.GetReferencedPaths().Where(path => !File.Exists(path) &&
            !protectedPrefixes.Any(prefix => IsAncestorDirectory(prefix, path)) && IsConfirmedMissingFile(path)).ToArray();
        if (orphaned.Length > 0) { store.PurgeDeletedTracks(orphaned); removed.AddRange(orphaned); }
        if (removed.Count > 0) store.PruneUnreferencedArtwork(artworkDirectory);
        return new(removed, unavailable, recovered);
    }

    public bool MarkUnavailable(string path)
    {
        queries.SetTrackUnavailable(path, true);
        var confirmedMissing = IsConfirmedMissingFile(path);
        if (confirmedMissing)
        {
            store.PurgeDeletedTracks([path]);
            store.PruneUnreferencedArtwork(artworkDirectory);
        }
        return confirmedMissing;
    }

    public bool MarkAvailable(string path)
    {
        store.ReinstateTrack(path);
        return queries.SetTrackUnavailable(path, false);
    }

    public int MarkUnavailableRoots(IEnumerable<string> roots)
    {
        var marked = 0;
        foreach (var root in roots)
            for (var offset = 0; ; offset += 800)
            {
                var paths = store.GetTrackPaths(groupColumn: "folder", groupValue: root, offset: offset, pageSize: 800, includeUnavailable: true);
                marked += paths.Count;
                if (paths.Count < 800) break;
            }
        return marked;
    }

    /// <summary>Reconciles a filesystem deletion event and returns indexed/queued paths confirmed missing.</summary>
    public string[] ReconcileRemovedPaths(IEnumerable<string> deletedPaths)
    {
        var candidates = new HashSet<string>(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
        foreach (var rawPath in deletedPaths)
        {
            string path;
            try { path = Path.GetFullPath(rawPath); }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { continue; }

            // A watcher can report an enclosing directory when a folder is deleted. Resolve
            // its indexed descendants in bounded pages instead of dropping the whole root.
            if (store.GetTrack(path) is not null || LibraryScanner.IsSupportedAudioFile(path))
            {
                candidates.Add(path);
                continue;
            }

            const int pageSize = 800;
            for (var offset = 0; ; offset += pageSize)
            {
                var paths = store.GetTrackPaths(groupColumn: "folder", groupValue: path, offset: offset, pageSize: pageSize, includeUnavailable: true);
                foreach (var trackPath in paths) candidates.Add(trackPath);
                if (paths.Count < pageSize) break;
            }
        }

        var confirmedMissing = new List<string>();
        foreach (var path in candidates)
        {
            // Rename/delete notifications can race a replacement at the same path.
            if (File.Exists(path) || !IsConfirmedMissingFile(path)) continue;
            queries.SetTrackUnavailable(path, true);
            confirmedMissing.Add(path);
        }

        if (confirmedMissing.Count > 0)
        {
            store.PurgeDeletedTracks(confirmedMissing);
            store.PruneUnreferencedArtwork(artworkDirectory);
        }
        return candidates.Where(path => queries.IsTrackUnavailable(path)).ToArray();
    }

    public string[] ClearUnavailableTracksThatExist() => queries.ClearUnavailableTracksThatExist();

    private bool IsConfirmedMissingFile(string path)
    {
        string fullPath;
        try { fullPath = Path.GetFullPath(path); }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { return false; }

        // A ready host volume is not proof that a path below a mounted folder or
        // junction is reachable. Preserve indexed rows behind a broken reparse
        // boundary until its target is available again.
        if (HasUnavailableReparseBoundary(fullPath)) return false;

        var parent = Path.GetDirectoryName(fullPath);
        var parentAvailable = !string.IsNullOrWhiteSpace(parent) && Directory.Exists(parent);
        var accessibleConfiguredRootContainsPath = !parentAvailable && locations.GetLibraryRoots()
            .Any(root => IsAncestorDirectory(root, fullPath) && Directory.Exists(root));
        var storageVolumeAvailable = IsStorageVolumeAvailable(fullPath);
        if (!parentAvailable && !accessibleConfiguredRootContainsPath && !storageVolumeAvailable) return false;

        try { _ = File.GetAttributes(fullPath); return false; }
        catch (FileNotFoundException) { return true; }
        catch (DirectoryNotFoundException) { return parentAvailable || accessibleConfiguredRootContainsPath || storageVolumeAvailable; }
        catch (UnauthorizedAccessException) { return false; }
        catch (SecurityException) { return false; }
        catch (IOException) { return false; }
    }

    private static bool IsStorageVolumeAvailable(string path)
    {
        try
        {
            var root = Path.GetPathRoot(path);
            return !string.IsNullOrWhiteSpace(root) && new DriveInfo(root).IsReady;
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
        {
            return false;
        }
    }

    private static bool HasUnavailableReparseBoundary(string fullPath)
    {
        var directory = Path.GetDirectoryName(fullPath);
        while (!string.IsNullOrWhiteSpace(directory))
        {
            FileAttributes attributes;
            try { attributes = File.GetAttributes(directory); }
            catch (Exception ex) when (ex is FileNotFoundException or DirectoryNotFoundException)
            {
                directory = Path.GetDirectoryName(directory);
                continue;
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException)
            {
                return true;
            }

            if ((attributes & FileAttributes.ReparsePoint) != 0)
            {
                try
                {
                    var target = new DirectoryInfo(directory).ResolveLinkTarget(returnFinalTarget: true);
                    if (target is null || !Directory.Exists(target.FullName)) return true;
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException or ArgumentException or NotSupportedException)
                {
                    return true;
                }
            }

            var parent = Path.GetDirectoryName(directory);
            if (string.Equals(parent, directory, OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal)) break;
            directory = parent;
        }
        return false;
    }

    private static string? TryNormalizePrefix(string path)
    {
        try { return Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)); }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { return null; }
    }

    private static bool IsAncestorDirectory(string root, string path)
    {
        try
        {
            var fullRoot = Path.TrimEndingDirectorySeparator(Path.GetFullPath(root));
            var relative = Path.GetRelativePath(fullRoot, path);
            return !Path.IsPathRooted(relative) && relative != "." &&
                relative != ".." && !relative.StartsWith(".." + Path.DirectorySeparatorChar, StringComparison.Ordinal) &&
                !relative.StartsWith(".." + Path.AltDirectorySeparatorChar, StringComparison.Ordinal);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException) { return false; }
    }
}

internal sealed record AvailabilityReconciliation(IReadOnlyList<string> RemovedPaths, int UnavailableCount, int RecoveredCount);
