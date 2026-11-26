using System.Reflection;

namespace MusicPlayer.Core;

public static class ApplicationVersion
{
    public static string Resolve(Assembly assembly)
    {
        var text = assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion?.Split('+')[0];
        return ReleaseVersion.TryParse(text, out _) ? text! : assembly.GetName().Version?.ToString(3) ?? "Unknown";
    }

    public static UpdateComparison Compare(string current, string? latest)
    {
        if (!ReleaseVersion.TryParse(current, out var installed)) return UpdateComparison.UnknownVersion;
        if (!ReleaseVersion.TryParse(latest, out var available)) return UpdateComparison.NoEligibleRelease;
        return available.CompareTo(installed) > 0 ? UpdateComparison.NewerRelease : UpdateComparison.CurrentRelease;
    }
}

public enum UpdateComparison { NewerRelease, CurrentRelease, NoEligibleRelease, UnknownVersion }
