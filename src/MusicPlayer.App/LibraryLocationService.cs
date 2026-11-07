using System.Text.Json;
using System.Text.RegularExpressions;
using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>Validates and persists library-root and scan-exclusion preferences.</summary>
internal sealed class LibraryLocationService(LibraryStore store)
{
    public string[] GetLibraryRoots() => ReadStringArray("library-roots");

    public string[] GetScanExclusions() => ReadStringArray("ignored-directories");

    public LibraryExtensionOption[] GetExtensionOptions()
    {
        var rules = ReadBooleanMap("file-extension-enabled");
        var types = ReadStringMap("file-extension-types");
        var extensions = LibraryScanner.AudioExtensions.Concat(LibraryScanner.VideoExtensions)
            .Concat(rules.Keys).Distinct(StringComparer.OrdinalIgnoreCase).Order(StringComparer.OrdinalIgnoreCase);
        return extensions.Select(extension =>
        {
            var kind = LibraryScanner.AudioExtensions.Contains(extension) ? "Audio"
                : LibraryScanner.VideoExtensions.Contains(extension) ? "Video"
                : types.GetValueOrDefault(extension, "Audio");
            var enabled = rules.TryGetValue(extension, out var configured)
                ? configured
                : LibraryScanner.AudioExtensions.Contains(extension);
            return new LibraryExtensionOption(extension, kind, enabled);
        }).ToArray();
    }

    public string[] GetEnabledExtensions() => GetExtensionOptions().Where(option => option.Enabled).Select(option => option.Extension).ToArray();

    public string[] GetEnabledVideoExtensions() => GetExtensionOptions()
        .Where(option => option.Enabled && option.Kind == "Video").Select(option => option.Extension).ToArray();

    public bool IsVideoExtensionEnabled(string path) => GetExtensionOptions().Any(option =>
        option.Enabled && option.Kind == "Video" && string.Equals(option.Extension, Path.GetExtension(path), StringComparison.OrdinalIgnoreCase));

    public void SetExtensionEnabled(string rawExtension, bool enabled, string kind = "Audio")
    {
        var extension = NormalizeExtension(rawExtension);
        var isAudio = LibraryScanner.AudioExtensions.Contains(extension);
        var isVideo = LibraryScanner.VideoExtensions.Contains(extension);
        kind = isAudio ? "Audio" : isVideo ? "Video" : kind.Equals("Video", StringComparison.OrdinalIgnoreCase) ? "Video" : "Audio";
        var rules = ReadBooleanMap("file-extension-enabled");
        rules[extension] = enabled;
        store.SetSetting("file-extension-enabled", JsonSerializer.Serialize(rules));
        if (!isAudio && !isVideo)
        {
            var types = ReadStringMap("file-extension-types");
            types[extension] = kind;
            store.SetSetting("file-extension-types", JsonSerializer.Serialize(types));
        }
    }

    private static string NormalizeExtension(string rawExtension)
    {
        var extension = rawExtension.Trim().ToLowerInvariant();
        if (!extension.StartsWith('.')) extension = "." + extension;
        if (!Regex.IsMatch(extension, @"^\.[a-z0-9]{1,12}$")) throw new ArgumentException("Enter a file extension such as .mp4.");
        return extension;
    }

    private Dictionary<string, bool> ReadBooleanMap(string key)
    {
        try { return store.GetSetting(key) is { } json ? JsonSerializer.Deserialize<Dictionary<string, bool>>(json) ?? new(StringComparer.OrdinalIgnoreCase) : new(StringComparer.OrdinalIgnoreCase); }
        catch (JsonException ex) { LocalAppLog.Shared.Warning("settings", $"Saved setting '{key}' was invalid JSON.", ex); return new(StringComparer.OrdinalIgnoreCase); }
    }

    private Dictionary<string, string> ReadStringMap(string key)
    {
        try { return store.GetSetting(key) is { } json ? JsonSerializer.Deserialize<Dictionary<string, string>>(json) ?? new(StringComparer.OrdinalIgnoreCase) : new(StringComparer.OrdinalIgnoreCase); }
        catch (JsonException ex) { LocalAppLog.Shared.Warning("settings", $"Saved setting '{key}' was invalid JSON.", ex); return new(StringComparer.OrdinalIgnoreCase); }
    }

    public void SetLibraryRoots(IEnumerable<string> newRoots)
    {
        var roots = newRoots.Select(NormalizeRoot).Where(root => root is not null).Cast<string>().Distinct(PathComparer).ToArray();
        store.SetSetting("library-roots", JsonSerializer.Serialize(roots));
        store.SetSetting("library-roots-configured", "true");
    }

    public string AddLibraryRoot(string rawPath)
    {
        var path = NormalizeRoot(rawPath) ?? throw new ArgumentException("That folder path is not valid.");
        var roots = ReadStringArray("library-roots").ToList();
        var comparer = PathComparer;
        var exists = roots.Any(root => NormalizeRoot(root) is { } saved && comparer.Equals(saved, path));
        if (!exists) roots.Add(path);
        store.SetSetting("library-roots", JsonSerializer.Serialize(roots));
        store.SetSetting("library-roots-configured", "true");
        return path;
    }

    public LibraryRootRemoval? PrepareLibraryRootRemoval(string rawPath)
    {
        var path = NormalizeRoot(rawPath) ?? throw new ArgumentException("That folder path is not valid.");
        var comparer = PathComparer;
        var roots = ReadStringArray("library-roots").Select(NormalizeRoot).Where(root => root is not null).Cast<string>().ToList();
        var remaining = roots.Where(root => !comparer.Equals(root, path)).ToArray();
        var removed = roots.Where(root => comparer.Equals(root, path)).ToArray();
        return removed.Length == 0 ? null : new LibraryRootRemoval(removed, remaining);
    }

    public void PersistLibraryRootRemoval(LibraryRootRemoval removal)
    {
        store.SetSetting("library-roots", JsonSerializer.Serialize(removal.RemainingRoots));
        store.SetSetting("library-roots-configured", "true");
    }

    public void AddScanExclusion(string path)
    {
        var paths = ReadStringArray("ignored-directories").ToList();
        if (!paths.Contains(path, PathComparer)) paths.Add(path);
        store.SetSetting("ignored-directories", JsonSerializer.Serialize(paths));
    }

    public void RemoveScanExclusion(string path)
    {
        var paths = ReadStringArray("ignored-directories").Where(item => !PathComparer.Equals(item, path)).ToArray();
        store.SetSetting("ignored-directories", JsonSerializer.Serialize(paths));
    }

    private string[] ReadStringArray(string key)
    {
        try { return store.GetSetting(key) is { } json ? JsonSerializer.Deserialize<string[]>(json) ?? Array.Empty<string>() : Array.Empty<string>(); }
        catch (JsonException ex)
        {
            LocalAppLog.Shared.Warning("settings", $"Saved setting '{key}' was invalid JSON.", ex);
            return Array.Empty<string>();
        }
    }

    private static StringComparer PathComparer => OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;

    private static string? NormalizeRoot(string path)
    {
        try { return Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)); }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            LocalAppLog.Shared.Warning("library-roots", $"Ignored invalid saved library folder '{path}'.", ex);
            return null;
        }
    }
}

internal sealed record LibraryRootRemoval(string[] RemovedRoots, string[] RemainingRoots);
internal sealed record LibraryExtensionOption(string Extension, string Kind, bool Enabled);
