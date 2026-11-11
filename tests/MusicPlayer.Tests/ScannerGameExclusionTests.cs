using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class ScannerGameExclusionTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-game-scan-tests-" + Guid.NewGuid().ToString("N"));

    [Fact]
    public async Task Default_scan_skips_game_library_names_at_any_location_but_keeps_user_music()
    {
        var gameAudio = CreateFile(Path.Combine(_root, "ExternalDrive", "Libraries", "Games", "Huge Game", "audio.ogg"));
        var steamAudio = CreateFile(Path.Combine(_root, "Users", "Person", "SteamLibrary", "steamapps", "common", "Game", "audio.flac"));
        var musicAudio = CreateFile(Path.Combine(_root, "Users", "Person", "Music", "Albums", "song.mp3"));
        var rockstarUserMusic = CreateFile(Path.Combine(_root, "Users", "Person", "Documents", "Rockstar Games", "GTA V", "User Music", "custom-track.mp3"));
        var discovered = new List<string>();
        var excluded = new List<string>();

        using var control = new ScanControl();
        await NewScanner().ScanAsync(
            [_root], [], control,
            (path, _) => { discovered.Add(Path.GetFullPath(path)); return ValueTask.CompletedTask; },
            pathExcluded: path => excluded.Add(Path.GetFullPath(path)));

        Assert.Equal(new[] { Path.GetFullPath(musicAudio), Path.GetFullPath(rockstarUserMusic) }.Order(StringComparer.OrdinalIgnoreCase),
            discovered.Order(StringComparer.OrdinalIgnoreCase));
        Assert.Contains(Path.GetFullPath(Path.GetDirectoryName(Path.GetDirectoryName(gameAudio)!)!), excluded, StringComparer.OrdinalIgnoreCase);
        Assert.Contains(Path.GetFullPath(Path.Combine(_root, "Users", "Person", "SteamLibrary")), excluded, StringComparer.OrdinalIgnoreCase);
        Assert.DoesNotContain(discovered, path => PathsEqual(path, gameAudio) || PathsEqual(path, steamAudio));
    }

    [Fact]
    public async Task Default_scan_skips_dot_folders_such_as_bun_and_npm_on_all_platforms()
    {
        var bunAudio = CreateFile(Path.Combine(_root, "Users", "Person", ".bun", "install", "cache", "pkg", "audio.mp3"));
        var npmAudio = CreateFile(Path.Combine(_root, "Users", "Person", ".npm", "cache", "audio.mp3"));
        var normalAudio = CreateFile(Path.Combine(_root, "Users", "Person", "Music", "song.mp3"));
        var discovered = new List<string>();

        using var control = new ScanControl();
        await NewScanner().ScanAsync(
            [_root], [], control,
            (path, _) => { discovered.Add(Path.GetFullPath(path)); return ValueTask.CompletedTask; });

        Assert.Contains(Path.GetFullPath(normalAudio), discovered);
        Assert.DoesNotContain(discovered, path => PathsEqual(path, bunAudio) || PathsEqual(path, npmAudio));
    }

    [Theory]
    [InlineData("Games")]
    [InlineData("SteamLibrary")]
    [InlineData("XboxGames")]
    public async Task Explicitly_selected_game_library_root_is_scanned(string rootName)
    {
        var selectedRoot = Path.Combine(_root, "Arbitrary", "Install", rootName);
        var audio = CreateFile(Path.Combine(selectedRoot, "my-audio.mp3"));
        var discovered = new List<string>();

        using var control = new ScanControl();
        await NewScanner().ScanAsync(
            [selectedRoot], [], control,
            (path, _) => { discovered.Add(Path.GetFullPath(path)); return ValueTask.CompletedTask; });

        Assert.Contains(discovered, path => PathsEqual(path, audio));
    }

    private LibraryScanner NewScanner() => new(new LocalAppLog(Path.Combine(_root, "Logs")));

    private static string CreateFile(string path)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllBytes(path, []);
        return path;
    }

    private static bool PathsEqual(string left, string right) => string.Equals(
        Path.GetFullPath(left), Path.GetFullPath(right),
        OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal);

    public void Dispose()
    {
        if (Directory.Exists(_root)) Directory.Delete(_root, true);
    }
}
