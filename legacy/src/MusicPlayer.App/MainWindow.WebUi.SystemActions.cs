using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using MusicPlayer.Core;
using Microsoft.UI.Xaml;
using Windows.Devices.Enumeration;
using Windows.Media.Devices;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task ExportLogsAsync()
    {
        var picker = new FileSavePicker(); picker.FileTypeChoices.Add("ZIP log archive", [".zip"]); picker.SuggestedFileName = "music-player-logs";
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var file = await picker.PickSaveFileAsync(); if (file is null) return;
        LocalAppLog.Shared.ExportTo(file.Path); await ShowNoticeAsync("Logs exported. Review the ZIP for local paths before sharing.");
    }

    private async Task<object?> PickArtworkAsync()
    {
        var picker = new FileOpenPicker();
        foreach (var extension in new[] { ".jpg", ".jpeg", ".png", ".webp", ".bmp" }) picker.FileTypeFilter.Add(extension);
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        var file = await picker.PickSingleFileAsync();
        if (file is null) return null;
        var token = _libraryQueries.OpaqueId(file.Path); _webArtworkPaths[token] = file.Path;
        return new { token, name = file.Name };
    }

    private static void OpenRelease(string url)
    {
        if (GitHubUpdates.IsReleasePage(url) || GitHubUpdates.IsRepositoryInfoPage(url))
            Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
    }

    private void RevealFileInExplorer(string path)
    {
        if (_store.GetTrack(path) is null || !File.Exists(path)) throw new FileNotFoundException("That track is no longer available at this location.");
        var info = new ProcessStartInfo("explorer.exe") { UseShellExecute = true }; info.ArgumentList.Add("/select," + path); Process.Start(info);
    }

    private static string String(JsonElement obj, string key, string fallback = "") => obj.ValueKind == JsonValueKind.Object && obj.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() ?? fallback : fallback;

    private static int Int(JsonElement obj, string key, int fallback = 0) => obj.ValueKind == JsonValueKind.Object && obj.TryGetProperty(key, out var value) && value.TryGetInt32(out var result) ? result : fallback;

    private static double Double(JsonElement obj, string key, double fallback = 0) => obj.ValueKind == JsonValueKind.Object && obj.TryGetProperty(key, out var value) && value.TryGetDouble(out var result) ? result : fallback;

    private static bool Boolean(JsonElement obj, string key) => obj.ValueKind == JsonValueKind.Object && obj.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.True;

    private string PlaylistId(JsonElement payload)
    {
        var id = String(payload, "playlistId");
        if (string.IsNullOrWhiteSpace(id) || !_store.GetPlaylistSummaries().Any(item => item.Id == id)) throw new KeyNotFoundException("Playlist not found.");
        return id;
    }
}
