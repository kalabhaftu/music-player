using System.Security;

namespace MusicPlayer.Core;

public sealed class ScanControl : IDisposable
{
    private readonly object _gate = new();
    private TaskCompletionSource _resumed = Completed();
    private readonly CancellationTokenSource _cancel = new();
    public CancellationToken Token => _cancel.Token;
    public void Pause() { lock (_gate) _resumed = new(TaskCreationOptions.RunContinuationsAsynchronously); }
    public void Resume() { lock (_gate) _resumed.TrySetResult(); }
    public void Cancel() => _cancel.Cancel();
    internal async ValueTask WaitIfPausedAsync()
    {
        Task wait;
        lock (_gate) wait = _resumed.Task;
        await wait.WaitAsync(Token).ConfigureAwait(false);
    }
    public void Dispose() => _cancel.Dispose();
    private static TaskCompletionSource Completed() { var t = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously); t.SetResult(); return t; }
}

/// <summary>Why a subtree was skipped during scanning, used to distinguish stale built-in exclusions from user data.</summary>
public enum ScanExcludedPathKind
{
    UserIgnored,
    ReparsePoint,
    DefaultExcluded
}

public sealed class LibraryScanner(LocalAppLog? log = null)
{
    private readonly LocalAppLog _log = log ?? LocalAppLog.Shared;
    public static readonly HashSet<string> AudioExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".mp3", ".flac", ".wav", ".wave", ".aif", ".aiff", ".m4a", ".m4b", ".aac", ".ogg", ".oga", ".opus", ".wma", ".ape", ".wv", ".tta", ".mpc", ".dsf", ".dff"
    };

    public static readonly HashSet<string> VideoExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".mp4", ".m4v", ".mkv", ".avi", ".mov", ".wmv", ".webm", ".mpeg", ".mpg", ".mts", ".m2ts", ".3gp", ".ogv"
    };

    public static bool IsVideoFile(string path) => VideoExtensions.Contains(Path.GetExtension(path));

    public static bool IsSupportedAudioFile(string path) => AudioExtensions.Contains(Path.GetExtension(path));

    private static readonly HashSet<string> SystemFolders = new(StringComparer.OrdinalIgnoreCase)
    {
        "Windows", "Windows.old", "Program Files", "Program Files (x86)", "ProgramData", "WindowsApps", "Recovery", "System Volume Information", "$Recycle.Bin", "PerfLogs"
    };

    private static readonly HashSet<string> GeneratedOrApplicationFolders = new(StringComparer.OrdinalIgnoreCase)
    {
        ".git", ".hg", ".svn", ".vs", ".idea", ".vscode",
        "node_modules", "bin", "obj", "build", "dist", "target", "packages", "TestResults",
        "__pycache__", ".pytest_cache", ".mypy_cache", ".tox", ".venv",
        ".cache", ".npm", ".nuget", ".gradle", "Temp", "tmp", "Caches", "CrashDumps",
        "INetCache", "Temporary Internet Files", "MicrosoftEdgeBackups", "Package Cache",
        "Games", "GameLibrary", "GamesLibrary", "SteamLibrary", "steamapps", "XboxGames", "Epic Games"
    };

    public static IEnumerable<string> DefaultRoots()
    {
        foreach (var drive in DriveInfo.GetDrives())
        {
            var include = false;
            try { include = drive.DriveType is DriveType.Fixed or DriveType.Removable && drive.IsReady; }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            catch (SecurityException) { }
            if (include) yield return drive.RootDirectory.FullName;
        }
    }

    public async Task ScanAsync(
        IEnumerable<string> roots,
        IEnumerable<string> ignoredDirectories,
        ScanControl control,
        Func<string, CancellationToken, ValueTask> onAudioFile,
        IProgress<ScanProgress>? progress = null,
        Action<string>? directoryVisited = null,
        Action<string, bool>? rootCompleted = null,
        Action<string>? pathExcluded = null,
        Action<string, ScanExcludedPathKind>? pathExcludedWithReason = null,
        Action<string>? rootUnavailable = null,
        Action<string>? pathIncomplete = null,
        IEnumerable<string>? supportedExtensions = null)
    {
        var extensions = (supportedExtensions ?? AudioExtensions)
            .Select(extension => extension.StartsWith('.') ? extension : $".{extension}")
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        void ReportExcluded(string path, ScanExcludedPathKind kind)
        {
            pathExcluded?.Invoke(path);
            pathExcludedWithReason?.Invoke(path, kind);
        }

        var ignored = ignoredDirectories.Select(path => Path.TrimEndingDirectorySeparator(Path.GetFullPath(path))).ToHashSet(PathComparer);
        var filesFound = 0;
        var directoriesVisited = 0;
        var nextProgress = Environment.TickCount64;
        foreach (var suppliedRoot in roots)
        {
            control.Token.ThrowIfCancellationRequested();
            await control.WaitIfPausedAsync().ConfigureAwait(false);
            var root = Normalize(suppliedRoot);
            // Root completion is deliberately local to the root directory. A blocked
            // child must protect that subtree, not every sibling in a large library.
            var completeRoot = true;
            var rootUnavailableReported = false;
            void ReportRootUnavailable()
            {
                if (rootUnavailableReported) return;
                rootUnavailableReported = true;
                rootUnavailable?.Invoke(root);
            }
            if (!Directory.Exists(root))
            {
                _log.Warning("scanner", $"Could not enumerate missing or unavailable root '{root}'.");
                rootCompleted?.Invoke(root, false);
                ReportRootUnavailable();
                continue;
            }

            var pending = new Stack<string>();
            pending.Push(root);
            while (pending.TryPop(out var directory))
            {
                control.Token.ThrowIfCancellationRequested();
                await control.WaitIfPausedAsync().ConfigureAwait(false);
                if (!TryGetAttributes(directory, out var directoryAttributes))
                {
                    if (PathsEqual(directory, root))
                    {
                        completeRoot = false;
                        ReportRootUnavailable();
                    }
                    else pathIncomplete?.Invoke(directory);
                    continue;
                }
                if ((directoryAttributes & FileAttributes.ReparsePoint) != 0)
                {
                    ReportExcluded(directory, ScanExcludedPathKind.ReparsePoint);
                    continue;
                }
                if (IsIgnoredPath(directory, ignored))
                {
                    ReportExcluded(directory, ScanExcludedPathKind.UserIgnored);
                    continue;
                }
                // A path the user chose explicitly is always a valid scan root, even when its
                // name resembles a system/application folder. Built-in exclusions apply below it.
                if (!PathsEqual(directory, root) && IsDefaultIgnoredDirectory(directory))
                {
                    ReportExcluded(directory, ScanExcludedPathKind.DefaultExcluded);
                    continue;
                }
                IEnumerator<string> entries;
                try { entries = Directory.EnumerateFileSystemEntries(directory).GetEnumerator(); }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException)
                {
                    _log.Warning("scanner", $"Could not enumerate directory '{directory}'.", ex);
                    if (PathsEqual(directory, root))
                    {
                        completeRoot = false;
                        ReportRootUnavailable();
                    }
                    else pathIncomplete?.Invoke(directory);
                    continue;
                }
                var completeDirectory = true;
                using (entries)
                {
                    while (true)
                    {
                        string entry;
                        try
                        {
                            if (!entries.MoveNext()) break;
                            entry = entries.Current;
                        }
                        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException)
                        {
                            _log.Warning("scanner", $"Could not completely enumerate directory '{directory}'.", ex);
                            completeDirectory = false;
                            break;
                        }
                        control.Token.ThrowIfCancellationRequested();
                        await control.WaitIfPausedAsync().ConfigureAwait(false);
                        if (!TryGetAttributes(entry, out var attributes))
                        {
                            completeDirectory = false;
                            continue;
                        }
                        if ((attributes & FileAttributes.ReparsePoint) != 0)
                        {
                            ReportExcluded(entry, ScanExcludedPathKind.ReparsePoint);
                            continue;
                        }
                        if ((attributes & FileAttributes.Directory) != 0)
                        {
                            var normalized = Normalize(entry);
                            if (IsIgnoredPath(normalized, ignored)) ReportExcluded(normalized, ScanExcludedPathKind.UserIgnored);
                            else if (IsDefaultIgnoredDirectory(normalized)) ReportExcluded(normalized, ScanExcludedPathKind.DefaultExcluded);
                            else pending.Push(normalized);
                        }
                        else if (extensions.Contains(Path.GetExtension(entry)))
                        {
                            await onAudioFile(entry, control.Token).ConfigureAwait(false);
                            filesFound++;
                            if (Environment.TickCount64 >= nextProgress)
                            {
                                nextProgress = Environment.TickCount64 + 250;
                                progress?.Report(new(filesFound, directoriesVisited, entry));
                            }
                        }
                    }
                }
                if (!completeDirectory)
                {
                    if (PathsEqual(directory, root))
                    {
                        completeRoot = false;
                        ReportRootUnavailable();
                    }
                    else pathIncomplete?.Invoke(directory);
                    continue;
                }
                directoriesVisited++;
                directoryVisited?.Invoke(directory);
            }
            rootCompleted?.Invoke(root, completeRoot);
        }
        progress?.Report(new(filesFound, directoriesVisited, string.Empty));
    }

    private static bool IsDefaultIgnoredDirectory(string path)
    {
        var name = Path.GetFileName(Path.TrimEndingDirectorySeparator(path));
        if (name.Equals("AppData", StringComparison.OrdinalIgnoreCase) || GeneratedOrApplicationFolders.Contains(name)) return true;
        if (!SystemFolders.Contains(name)) return false;
        var parent = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(path));
        var root = Path.GetPathRoot(path);
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        return parent is not null && root is not null && string.Equals(
            Path.TrimEndingDirectorySeparator(parent), Path.TrimEndingDirectorySeparator(root), comparison);
    }

    public static bool IsDefaultExcludedDirectory(string path) => IsDefaultIgnoredDirectory(path);

    private static bool PathsEqual(string left, string right) => string.Equals(
        Path.TrimEndingDirectorySeparator(Path.GetFullPath(left)),
        Path.TrimEndingDirectorySeparator(Path.GetFullPath(right)),
        OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);

    private static bool IsIgnoredPath(string path, HashSet<string> ignored)
    {
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        var current = Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
        while (!string.IsNullOrEmpty(current))
        {
            if (ignored.Contains(current)) return true;
            var parent = Path.GetDirectoryName(current);
            if (string.IsNullOrEmpty(parent) || string.Equals(parent, current, comparison)) break;
            current = Path.TrimEndingDirectorySeparator(parent);
        }
        return false;
    }

    private bool TryGetAttributes(string path, out FileAttributes attributes)
    {
        try { attributes = File.GetAttributes(path); return true; }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or SecurityException)
        {
            attributes = default;
            _log.Warning("scanner", $"Could not inspect filesystem entry '{path}'.", ex);
            return false;
        }
    }

    private static string Normalize(string path) => Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
    private static StringComparer PathComparer => OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
}
