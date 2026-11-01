using System.Text.Json;
using System.Diagnostics;
using MusicPlayer.Core;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics;
using WinRT.Interop;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private RectInt32? _queuePanelOriginalBounds;

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
            case "expandWindowForQueue":
                return ExpandWindowForQueue(Int(payload, "minimumWidth"));
            case "restoreWindowAfterQueue":
                RestoreWindowAfterQueue(); return null;
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

    private object ExpandWindowForQueue(int minimumWidthDip)
    {
        if (_immersiveMode) return new { expanded = false };
        var hwnd = WindowNative.GetWindowHandle(this);
        var windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
        var appWindow = AppWindow.GetFromWindowId(windowId);
        if (appWindow.Presenter.Kind != AppWindowPresenterKind.Overlapped) return new { expanded = false };

        var display = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
        var scale = Math.Max(1, ShellRoot.XamlRoot?.RasterizationScale ?? 1);
        var requestedWidth = (int)Math.Ceiling(Math.Clamp(minimumWidthDip, 560, 1800) * scale);
        var current = appWindow.Size;
        var targetWidth = Math.Min(display.WorkArea.Width, Math.Max(current.Width, requestedWidth));
        var targetHeight = Math.Min(current.Height, display.WorkArea.Height);
        if (targetWidth <= current.Width && targetHeight >= current.Height) return new { expanded = false };

        _queuePanelOriginalBounds ??= new RectInt32(appWindow.Position.X, appWindow.Position.Y, current.Width, current.Height);
        var x = Math.Clamp(appWindow.Position.X, display.WorkArea.X, display.WorkArea.X + display.WorkArea.Width - targetWidth);
        var y = Math.Clamp(appWindow.Position.Y, display.WorkArea.Y, display.WorkArea.Y + display.WorkArea.Height - targetHeight);
        appWindow.MoveAndResize(new RectInt32(x, y, targetWidth, targetHeight), display);
        return new { expanded = true };
    }

    private void RestoreWindowAfterQueue()
    {
        if (_queuePanelOriginalBounds is not { } original) return;
        _queuePanelOriginalBounds = null;
        if (_immersiveMode) return;
        var hwnd = WindowNative.GetWindowHandle(this);
        var windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
        var appWindow = AppWindow.GetFromWindowId(windowId);
        if (appWindow.Presenter.Kind != AppWindowPresenterKind.Overlapped) return;
        var display = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
        var width = Math.Min(original.Width, display.WorkArea.Width);
        var height = Math.Min(original.Height, display.WorkArea.Height);
        var x = Math.Clamp(original.X, display.WorkArea.X, display.WorkArea.X + display.WorkArea.Width - width);
        var y = Math.Clamp(original.Y, display.WorkArea.Y, display.WorkArea.Y + display.WorkArea.Height - height);
        appWindow.MoveAndResize(new RectInt32(x, y, width, height), display);
    }
}
