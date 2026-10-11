namespace MusicPlayer.Core;

public enum FileActivationAction { Play, AddToQueue, CreatePlaylist }
public sealed record FileActivationRequest(FileActivationAction Action, IReadOnlyList<string> Paths);

public static class FileActivation
{
    // The platform may forward an executable token in Launch.Arguments, or a
    // manifest verb in FileActivatedEventArgs. Normalize both at one boundary.
    public static FileActivationRequest Parse(IEnumerable<string> arguments, string? verb = null,
        IEnumerable<string>? activatedFiles = null)
    {
        var tokens = arguments.ToList();
        if (tokens.Count > 0 && string.Equals(Path.GetFileName(tokens[0]), "MusicPlayer.exe", StringComparison.OrdinalIgnoreCase))
            tokens.RemoveAt(0);
        var action = ActionFor(verb);
        foreach (var token in tokens)
            if (token.StartsWith("--", StringComparison.Ordinal) && ActionFor(token) is { } requested && requested != FileActivationAction.Play) action = requested;
        var paths = activatedFiles ?? tokens.Where(token => !token.StartsWith("--", StringComparison.Ordinal));
        var normalized = new List<string>();
        foreach (var path in paths)
        {
            try
            {
                var fullPath = Path.GetFullPath(path);
                if (File.Exists(fullPath) && (LibraryScanner.IsSupportedAudioFile(fullPath) || LibraryScanner.IsVideoFile(fullPath)))
                    normalized.Add(fullPath);
            }
            catch (Exception ex) when (ex is ArgumentException or IOException or NotSupportedException) { }
        }
        return new(action, normalized);
    }

    private static FileActivationAction ActionFor(string? verb) => verb?.ToLowerInvariant() switch
    {
        "addtoqueue" or "musicplayeraddtoqueue" or "--add-to-queue" => FileActivationAction.AddToQueue,
        "createplaylist" or "musicplayercreateplaylist" or "--create-playlist" => FileActivationAction.CreatePlaylist,
        _ => FileActivationAction.Play
    };
}
