using System.Text.Json;
using System.Diagnostics;
using MusicPlayer.Core;
using Windows.ApplicationModel.DataTransfer;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task<object?> HandleWebUiUtilityCommandAsync(string name, JsonElement payload)
    {
        switch (name)
        {
            case "reportFrontendError":
            {
                var kind = String(payload, "kind");
                if (kind is "script" or "rejection")
                {
                    static string SafeToken(string value) => System.Text.RegularExpressions.Regex.IsMatch(value, @"^[A-Za-z0-9._-]{1,80}$") ? value : "unknown";
                    var errorName = SafeToken(String(payload, "name"));
                    var command = SafeToken(String(payload, "command"));
                    var source = SafeToken(String(payload, "source"));
                    var line = Math.Clamp(Int(payload, "line"), 0, 1_000_000);
                    var column = Math.Clamp(Int(payload, "column"), 0, 1_000_000);
                    var context = kind == "script"
                        ? $"{errorName} at {source}:{line}:{column}"
                        : $"{errorName}" + (command == "unknown" ? "" : $" in '{command}'");
                    LocalAppLog.Shared.Warning("web-ui", $"The local interface reported an unhandled {kind} ({context}).");
                }
                return null;
            }
            case "openLogs": OpenLogsFolder_Click(this, new Microsoft.UI.Xaml.RoutedEventArgs()); return null;
            case "exportLogs": await ExportLogsAsync(); return null;
            case "openRelease": OpenRelease(String(payload, "url")); return null;
            case "openLegalFile":
            {
                var relativePath = String(payload, "name") switch
                {
                    "LICENSE" => "LICENSE",
                    "ThirdPartyNotices.md" => "ThirdPartyNotices.md",
                    "LGPL-2.1.txt" => Path.Combine("ThirdPartyLicenses", "LGPL-2.1.txt"),
                    _ => throw new ArgumentException("That license document is not available.")
                };
                var path = Path.Combine(AppContext.BaseDirectory, relativePath);
                if (!File.Exists(path)) throw new FileNotFoundException("The bundled license document is missing.");
                Process.Start(new ProcessStartInfo(path) { UseShellExecute = true });
                return null;
            }
            case "setPanelMode":
                _store.SetSetting("right-sidebar-mode", String(payload, "mode") == "info" ? "Info" : "Queue"); return null;
            case "setImmersiveMode":
                SetImmersiveMode(payload.TryGetProperty("enabled", out var immersive) && immersive.ValueKind == JsonValueKind.True);
                return null;
            case "openDefaultApps":
                await DefaultAppsService.OpenAsync();
                return null;
            case "copyTrackPath":
            {
                var path = _libraryQueries.ResolveTrackPath(String(payload, "id"));
                if (string.IsNullOrWhiteSpace(path)) throw new KeyNotFoundException("The track location is no longer available.");
                var package = new DataPackage();
                package.SetText(path);
                Clipboard.SetContent(package);
                return null;
            }
            default: throw new InvalidOperationException("This Music Player command is not available in the utility handler.");
        }
    }
}
