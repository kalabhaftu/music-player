namespace MusicPlayer.Core;

/// <summary>Resolves the Music Player per-user data folder.</summary>
public static class AppDataPaths
{
    public static string Root => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "MusicPlayer");
}
