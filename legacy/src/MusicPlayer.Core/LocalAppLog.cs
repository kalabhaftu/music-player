using System.IO.Compression;
using System.Text;

namespace MusicPlayer.Core;

public sealed class LocalAppLog
{
    private const string LogPrefix = "music-player-";
    private const long MaxBytes = 4 * 1024 * 1024;
    private const long MaxTotalBytes = 50 * 1024 * 1024;
    private const int RetentionDays = 14;
    private readonly string _folder;
    private readonly object _gate = new();
    private DateOnly? _lastPrunedDayUtc;

    public static LocalAppLog Shared { get; } = new();
    public string FolderPath => _folder;

    public LocalAppLog(string? folder = null)
    {
        _folder = folder ?? Path.Combine(AppDataPaths.Root, "Logs");
    }

    public void Info(string area, string message) => Write("INFO", area, message, null);
    public void Warning(string area, string message, Exception? exception = null) => Write("WARN", area, message, exception);
    public void Error(string area, string message, Exception exception) => Write("ERROR", area, message, exception);

    public void ExportTo(string destination)
    {
        var fullPath = Path.GetFullPath(destination);
        if (!Path.GetExtension(fullPath).Equals(".zip", StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("Diagnostic logs must be exported to a ZIP file.", nameof(destination));
        var directory = Path.GetDirectoryName(fullPath)!;
        lock (_gate)
        {
            string[] files = Directory.Exists(_folder)
                ? Directory.GetFiles(_folder, $"{LogPrefix}*.log*")
                : [];
            if (files.Length == 0) throw new FileNotFoundException("There are no Music Player logs to export.", _folder);
            var pathComparer = OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;
            if (files.Contains(fullPath, pathComparer)) throw new ArgumentException("Choose a ZIP destination outside the log files.", nameof(destination));
            Directory.CreateDirectory(directory);
            var temporary = Path.Combine(directory, $".{LogPrefix}logs-{Guid.NewGuid():N}.tmp");
            try
            {
                using (var archive = ZipFile.Open(temporary, ZipArchiveMode.Create))
                    foreach (var file in files) archive.CreateEntryFromFile(file, Path.GetFileName(file), CompressionLevel.Optimal);
                File.Move(temporary, fullPath, true);
            }
            finally { if (File.Exists(temporary)) File.Delete(temporary); }
        }
    }

    private void Write(string level, string area, string message, Exception? exception)
    {
        var now = DateTime.UtcNow;
        var entry = $"{DateTimeOffset.UtcNow:O} [{level}] [{area}] {message}" +
            (exception is null ? "" : Environment.NewLine + exception) + Environment.NewLine;
        if (Encoding.UTF8.GetByteCount(entry) > MaxBytes) entry = Truncate(entry);
        lock (_gate)
        {
            try
            {
                Directory.CreateDirectory(_folder);
                var day = DateOnly.FromDateTime(now);
                if (_lastPrunedDayUtc != day && PruneLogs(now, null)) _lastPrunedDayUtc = day;
                var path = Path.Combine(_folder, $"{LogPrefix}{now:yyyy-MM-dd}.log");
                if (File.Exists(path) && new FileInfo(path).Length + Encoding.UTF8.GetByteCount(entry) > MaxBytes)
                    File.Move(path, path + ".1", true);
                File.AppendAllText(path, entry, new UTF8Encoding(false));
                EnforceTotalLimit(now, path);
            }
            catch
            {
                // Logging must never turn an ordinary app error into a second failure.
            }
        }
    }

    private bool PruneLogs(DateTime now, string? protectedPath)
    {
        try
        {
            EnforceTotalLimit(now, protectedPath);
            return true;
        }
        catch
        {
            return false;
        }
    }

    private void EnforceTotalLimit(DateTime now, string? protectedPath)
    {
        var files = EnumerateLogFiles()
            .Select(path => new FileInfo(path))
            .Where(file => file.Exists)
            .ToArray();
        var cutoff = now.AddDays(-RetentionDays);
        foreach (var file in files)
        {
            if (file.LastWriteTimeUtc < cutoff && !SamePath(file.FullName, protectedPath))
            {
                try { file.Delete(); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
        }

        files = EnumerateLogFiles()
            .Select(path => new FileInfo(path))
            .Where(file => file.Exists)
            .ToArray();
        var total = files.Sum(file => file.Length);
        foreach (var file in files.OrderBy(file => file.LastWriteTimeUtc))
        {
            if (total <= MaxTotalBytes) break;
            if (SamePath(file.FullName, protectedPath)) continue;
            try
            {
                var length = file.Length;
                file.Delete();
                total -= length;
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }

    private static bool SamePath(string first, string? second) => second is not null &&
        string.Equals(Path.GetFullPath(first), Path.GetFullPath(second), OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);

    private IEnumerable<string> EnumerateLogFiles()
    {
        if (!Directory.Exists(_folder)) return [];
        return Directory.EnumerateFiles(_folder, $"{LogPrefix}*.log*");
    }

    private static string Truncate(string value)
    {
        const string suffix = "\n[log entry truncated]\n";
        var limit = MaxBytes - Encoding.UTF8.GetByteCount(suffix);
        var low = 0; var high = value.Length;
        while (low < high)
        {
            var middle = (low + high + 1) / 2;
            if (Encoding.UTF8.GetByteCount(value.AsSpan(0, middle)) <= limit) low = middle;
            else high = middle - 1;
        }
        return value[..low] + suffix;
    }
}
