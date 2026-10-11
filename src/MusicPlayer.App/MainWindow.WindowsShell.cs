using System.Diagnostics;
using System.Globalization;
using System.Numerics;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json;
using MusicPlayer.Core;
using Microsoft.UI.Composition;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Windowing;
using Windows.Graphics.Imaging;
using Windows.Media;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.Storage.Streams;
using Windows.UI;
using Windows.UI.ViewManagement;
using WinRT.Interop;

namespace MusicPlayer.App;

public sealed partial class MainWindow : Window
{
    private readonly object _updateCheckGate = new();
    private int _updateCheckActive;
    private TaskCompletionSource<bool>? _updateCheckCompletion;

    private void OpenLogsFolder_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var info = new ProcessStartInfo("explorer.exe") { UseShellExecute = true };
            info.ArgumentList.Add(LocalAppLog.Shared.FolderPath);
            Process.Start(info);
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("logs", "Could not open the log folder.", ex);
            _ = ShowNoticeAsync($"Logs are stored at {LocalAppLog.Shared.FolderPath}");
        }
    }

    private void ApplyStoredAppearance()
    {
        ShellRoot.RequestedTheme = _store.GetSetting("theme") switch { "Light" => ElementTheme.Light, "Dark" => ElementTheme.Dark, _ => ElementTheme.Default };
        SystemBackdrop = _store.GetSetting("transparent-window") == "true" ? new DesktopAcrylicBackdrop() : null;
        if (_store.GetSetting("accent-manual") == "true" && TryParseColor(_store.GetSetting("accent-color"), out var color)) ApplyAccent(color);
        else if (_store.GetSetting("accent-mode") == "Artwork" && TryParseColor(_webArtworkAccent, out var artworkAccent)) ApplyAccent(artworkAccent);
        else ResetAccent();
        ApplyNativeWindowChrome();
    }

    private void ApplyNativeWindowChrome()
    {
        try
        {
            var hwnd = WindowNative.GetWindowHandle(this);
            var windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = AppWindow.GetFromWindowId(windowId);
            appWindow.Title = string.Empty;
            var iconPath = ActiveProductIconPath();
            if (File.Exists(iconPath))
            {
                appWindow.SetIcon(iconPath);
                ApplyNativeWindowIcon(hwnd, iconPath);
            }

            _videoWindow?.ApplyTheme(ShellRoot.RequestedTheme, ShellRoot.ActualTheme, iconPath);
            var titleBar = appWindow.TitleBar;
            titleBar.IconShowOptions = IconShowOptions.HideIconAndSystemMenu;
            if (!AppWindowTitleBar.IsCustomizationSupported()) return;
            var artworkSurface = _store.GetSetting("accent-mode") == "Artwork" && _webArtworkTitleBarColor is { };
            var dark = artworkSurface || ShellRoot.RequestedTheme == ElementTheme.Dark ||
                (ShellRoot.RequestedTheme == ElementTheme.Default && ShellRoot.ActualTheme == ElementTheme.Dark);
            var background = artworkSurface ? _webArtworkTitleBarColor!.Value :
                dark ? Color.FromArgb(255, 16, 18, 16) : Color.FromArgb(255, 240, 242, 239);
            var foreground = dark ? Color.FromArgb(255, 247, 248, 246) : Color.FromArgb(255, 25, 29, 25);
            static Color Shift(Color color, int amount) => Color.FromArgb(255,
                (byte)Math.Clamp(color.R + amount, 0, 255),
                (byte)Math.Clamp(color.G + amount, 0, 255),
                (byte)Math.Clamp(color.B + amount, 0, 255));
            var inactiveForeground = dark ? Color.FromArgb(255, 160, 167, 159) : Color.FromArgb(255, 116, 124, 116);
            var hoverBackground = artworkSurface ? Shift(background, 15) : dark ? Color.FromArgb(255, 36, 38, 36) : Color.FromArgb(255, 212, 217, 211);
            var pressedBackground = artworkSurface ? Shift(background, 24) : dark ? Color.FromArgb(255, 48, 52, 48) : Color.FromArgb(255, 200, 206, 199);
            titleBar.BackgroundColor = background;
            titleBar.InactiveBackgroundColor = background;
            titleBar.ForegroundColor = foreground;
            titleBar.InactiveForegroundColor = foreground;
            titleBar.ButtonBackgroundColor = background;
            titleBar.ButtonForegroundColor = foreground;
            titleBar.ButtonInactiveBackgroundColor = background;
            titleBar.ButtonInactiveForegroundColor = inactiveForeground;
            titleBar.ButtonHoverBackgroundColor = hoverBackground;
            titleBar.ButtonHoverForegroundColor = foreground;
            titleBar.ButtonPressedBackgroundColor = pressedBackground;
            titleBar.ButtonPressedForegroundColor = foreground;
        }
        catch (Exception ex) when (ex is InvalidOperationException or COMException or ArgumentException)
        {
            LocalAppLog.Shared.Warning("window", "The native title bar icon or theme colors could not be applied.", ex);
        }
    }

    private async Task<bool> ApplyArtworkAccentAsync(string? artworkPath)
    {
        if (string.IsNullOrWhiteSpace(artworkPath) || !System.IO.File.Exists(artworkPath)) { _webArtworkAccent = null; _webArtworkPalette = null; _webArtworkTitleBarColor = null; ResetAccent(); ApplyNativeWindowChrome(); _webBridge?.SendEvent("artworkAccentChanged", new { color = (string?)null, palette = (object?)null }); return false; }
        try
        {
            var file = await StorageFile.GetFileFromPathAsync(artworkPath);
            using var stream = await file.OpenAsync(FileAccessMode.Read);
            var decoder = await BitmapDecoder.CreateAsync(stream);
            var data = await decoder.GetPixelDataAsync(BitmapPixelFormat.Rgba8, BitmapAlphaMode.Straight,
                new BitmapTransform { ScaledWidth = 32, ScaledHeight = 32 }, ExifOrientationMode.IgnoreExifOrientation, ColorManagementMode.ColorManageToSRgb);
            var pixels = data.DetachPixelData();
            if (!SameTrack(_playback.CurrentTrack?.ArtworkPath, artworkPath) || _store.GetSetting("accent-mode") != "Artwork") return false;
            if (pixels.Length < 4) { _webArtworkAccent = null; _webArtworkPalette = null; _webArtworkTitleBarColor = null; ResetAccent(); ApplyNativeWindowChrome(); _webBridge?.SendEvent("artworkAccentChanged", new { color = (string?)null, palette = (object?)null }); return false; }
            var artwork = ArtworkColors.FromRgba(pixels);
            var accent = Color.FromArgb(255, artwork.Accent.R, artwork.Accent.G, artwork.Accent.B);
            _webArtworkAccent = $"#{accent.R:X2}{accent.G:X2}{accent.B:X2}";
            var selectedPalette = SelectArtworkPalette(artwork);
            _webArtworkPalette = selectedPalette.Palette;
            _webArtworkTitleBarColor = selectedPalette.MainSurface;
            if (_store.GetSetting("accent-manual") != "true") ApplyAccent(accent);
            ApplyNativeWindowChrome();
            _webBridge?.SendEvent("artworkAccentChanged", new { color = _webArtworkAccent, palette = _webArtworkPalette });
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or InvalidOperationException or System.Runtime.InteropServices.COMException)
        { LocalAppLog.Shared.Warning("artwork-accent", $"Could not read artwork '{artworkPath}'.", ex); if (!SameTrack(_playback.CurrentTrack?.ArtworkPath, artworkPath) || _store.GetSetting("accent-mode") != "Artwork") return false; _webArtworkAccent = null; _webArtworkPalette = null; _webArtworkTitleBarColor = null; ResetAccent(); ApplyNativeWindowChrome(); _webBridge?.SendEvent("artworkAccentChanged", new { color = (string?)null, palette = (object?)null }); return false; }
    }

    private static (object Palette, Color MainSurface) SelectArtworkPalette(ArtworkPalette artwork)
    {
        var palette = new
        {
            background = artwork.Background.Hex, sidebar = artwork.Sidebar.Hex, main = artwork.Main.Hex,
            panel = artwork.Panel.Hex, raised = artwork.Raised.Hex, hover = artwork.Hover.Hex,
            control = artwork.Control.Hex, accent = artwork.Accent.Hex
        };
        return (palette, Color.FromArgb(255, artwork.Main.R, artwork.Main.G, artwork.Main.B));
    }

    private static void ApplyAccent(Color color)
    {
        Application.Current.Resources["MusicAccentColor"] = color;
        Application.Current.Resources["MusicAccentBrush"] = new SolidColorBrush(color);
    }

    private static void ResetAccent()
    {
        foreach (var key in new[] { "MusicAccentColor", "MusicAccentBrush" }) Application.Current.Resources.Remove(key);
    }

    private static bool TryParseColor(string? value, out Color color)
    {
        color = Colors.ForestGreen;
        if (string.IsNullOrWhiteSpace(value)) return false;
        var hex = value.Trim().TrimStart('#');
        if (hex.Length != 6 || !byte.TryParse(hex[..2], System.Globalization.NumberStyles.HexNumber, null, out var r) ||
            !byte.TryParse(hex[2..4], System.Globalization.NumberStyles.HexNumber, null, out var g) ||
            !byte.TryParse(hex[4..], System.Globalization.NumberStyles.HexNumber, null, out var b)) return false;
        color = Color.FromArgb(255, r, g, b); return true;
    }

    private async Task CheckForUpdatesAsync(bool force)
    {
        if (!force && _store.GetSetting("check-updates") == "false") return;
        if (!force && DateTime.TryParse(_store.GetSetting("last-update-check"), out var last) && DateTime.UtcNow - last.ToUniversalTime() < TimeSpan.FromHours(6)) return;
        // Startup and the Settings command can race. Share a pending request with
        // a manual caller so clicks never create parallel GitHub requests.
        TaskCompletionSource<bool>? completion = null;
        Task? pending = null;
        lock (_updateCheckGate)
        {
            if (_updateCheckActive != 0)
                pending = _updateCheckCompletion?.Task;
            else
            {
                completion = new(TaskCreationOptions.RunContinuationsAsynchronously);
                _updateCheckCompletion = completion;
                _updateCheckActive = 1;
            }
        }
        if (completion is null)
        {
            if (force && pending is not null) await pending;
            return;
        }

        _webBridge?.SendEvent("updateCheckState", new { checking = true });
        try
        {
            var assembly = typeof(MainWindow).Assembly;
            var currentVersionText = assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion
                ?? assembly.GetName().Version?.ToString(3);
            var current = ReleaseVersion.TryParse(currentVersionText, out var parsedCurrent) ? parsedCurrent : new ReleaseVersion(0, 1, 0, null);
            var channel = current.IsPrerelease ? "preview" : "stable";
            var sameChannel = string.Equals(_store.GetSetting("update-release-channel"), channel, StringComparison.Ordinal);
            var cachedRelease = sameChannel
                ? GitHubUpdates.ReadCachedRelease(_store.GetSetting("update-release-tag"), _store.GetSetting("update-release-url"))
                : null;
            var result = await GitHubUpdates.GetLatestAsync(current.IsPrerelease,
                sameChannel ? _store.GetSetting("update-etag") : null, cachedRelease);
            // Only record a completed response. Offline startup must not suppress retries for six hours.
            _store.SetSetting("last-update-check", DateTime.UtcNow.ToString("O", System.Globalization.CultureInfo.InvariantCulture));
            _store.SetSetting("update-etag", result.ETag ?? "");
            _store.SetSetting("update-release-tag", result.Release?.Tag ?? "");
            _store.SetSetting("update-release-url", result.Release?.Url ?? "");
            _store.SetSetting("update-release-channel", channel);
            _webAvailableRelease = null;
            var release = result.Release;
            _webBridge?.SendEvent("latestReleaseChanged", new { release = release is null ? null : new { tag = release.Tag, url = release.Url } });
            if (release is null || !ReleaseVersion.TryParse(release.Tag, out var latest) || latest.CompareTo(current) <= 0)
            {
                if (force) await ShowNoticeAsync("You are using the latest available release.");
                return;
            }
            _webAvailableRelease = release;
            if (_webUiActive && _webUiBootstrapped) PublishWebUpdateAvailable();
        }
        catch (HttpRequestException ex)
        {
            LocalAppLog.Shared.Warning("updates", "GitHub release check failed.", ex);
            var limited = ex.StatusCode is System.Net.HttpStatusCode.Forbidden or System.Net.HttpStatusCode.TooManyRequests;
            if (force) await ShowNoticeAsync(limited
                ? "GitHub is limiting update checks right now. Try again later."
                : "Could not reach GitHub. Check your connection and try again.", InfoBarSeverity.Warning);
        }
        catch (TaskCanceledException ex) { LocalAppLog.Shared.Warning("updates", "GitHub release check timed out.", ex); if (force) await ShowNoticeAsync("The GitHub update check timed out.", InfoBarSeverity.Warning); }
        catch (System.Text.Json.JsonException ex) { LocalAppLog.Shared.Warning("updates", "GitHub returned invalid release data.", ex); if (force) await ShowNoticeAsync("GitHub returned an update response Music Player could not read.", InfoBarSeverity.Warning); }
        catch (Exception ex) { LocalAppLog.Shared.Error("updates", "Could not complete the GitHub release check.", ex); if (force) await ShowNoticeAsync("The update check failed. See the local log for details.", InfoBarSeverity.Error); }
        finally
        {
            lock (_updateCheckGate)
            {
                _updateCheckActive = 0;
                if (ReferenceEquals(_updateCheckCompletion, completion)) _updateCheckCompletion = null;
            }
            completion.TrySetResult(true);
            _webBridge?.SendEvent("updateCheckState", new { checking = false });
        }
    }

    private void ApplyNativeWindowIcon()
    {
        try
        {
            var path = ActiveProductIconPath();
            if (!File.Exists(path)) return;
            var hwnd = WindowNative.GetWindowHandle(this);
            var windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
            AppWindow.GetFromWindowId(windowId).SetIcon(path);
            ApplyNativeWindowIcon(hwnd, path);
        }
        catch (Exception ex) when (ex is InvalidOperationException or COMException or ArgumentException)
        {
            LocalAppLog.Shared.Warning("window", "The Music Player taskbar icon could not be applied.", ex);
        }
    }

    private void ApplyNativeWindowIcon(nint hwnd, string iconPath)
    {
        if (string.Equals(_nativeWindowIconPath, iconPath, StringComparison.OrdinalIgnoreCase)) return;
        // AppWindow.SetIcon normally updates both surfaces. Set the Win32 small and large
        // icons as well so Windows taskbar grouping and title-bar rendering agree for a
        // directly launched portable executable.
        var small = LoadImage(IntPtr.Zero, iconPath, ImageIcon, 16, 16, LoadFromFile);
        var large = LoadImage(IntPtr.Zero, iconPath, ImageIcon, 32, 32, LoadFromFile);
        if (small != IntPtr.Zero)
        {
            _windowIconHandles.Add(small);
            _ = SendMessage(hwnd, WmSetIcon, new nint(IconSmall), small);
        }
        if (large != IntPtr.Zero)
        {
            _windowIconHandles.Add(large);
            _ = SendMessage(hwnd, WmSetIcon, new nint(IconBig), large);
        }
        if (small != IntPtr.Zero || large != IntPtr.Zero) _nativeWindowIconPath = iconPath;
    }

    private const uint WmSetIcon = 0x0080;
    private const int IconSmall = 0;
    private const int IconBig = 1;
    private const uint ImageIcon = 1;
    private const uint LoadFromFile = 0x0010;

    [DllImport("user32.dll", EntryPoint = "LoadImageW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern nint LoadImage(nint instance, string name, uint type, int width, int height, uint loadFlags);

    [DllImport("user32.dll", EntryPoint = "SendMessageW", SetLastError = true)]
    private static extern nint SendMessage(nint hwnd, uint message, nint wParam, nint lParam);

    [DllImport("user32.dll", EntryPoint = "DestroyIcon", SetLastError = true)]
    private static extern bool DestroyIcon(nint icon);

    private void PublishWebUpdateAvailable()
    {
        if (_webAvailableRelease is { } release)
            _webBridge?.SendEvent("updateAvailable", new { tag = release.Tag, url = release.Url });
    }


    private async Task RefreshEditedTrackAsync(string path)
    {
        var track = await Task.Run(() =>
        {
            var scanned = TrackReader.Read(path, Path.Combine(_appData, "Artwork"));
            _store.UpsertTrack(scanned);
            return scanned;
        });
        if (SameTrack(_playback.CurrentTrack?.Path, track.Path))
        {
            _playback.UpdateTrackMetadata(track);
            UpdateCurrentTrack(track);
            UpdateSystemMediaControls(track, _playback.IsPlaying);
        }
        PublishLibraryChanged();
    }

    private void InitializeSystemMediaControls()
    {
        try
        {
            _systemControls = SystemMediaTransportControlsInterop.GetForWindow(WindowNative.GetWindowHandle(this));
            _systemControls.IsEnabled = true; _systemControls.IsPlayEnabled = true; _systemControls.IsPauseEnabled = true;
            _systemControls.IsNextEnabled = true; _systemControls.IsPreviousEnabled = true;
            _systemControls.ButtonPressed += SystemControls_ButtonPressed;
        }
        catch (Exception ex) when (ex is InvalidOperationException or System.Runtime.InteropServices.COMException) { LocalAppLog.Shared.Warning("system-media-controls", "Could not initialize Windows media controls.", ex); }
    }

    private void InitializeTaskbarPeekControls()
    {
        try
        {
            _taskbarPeekControls = new TaskbarPeekControls(
                WindowNative.GetWindowHandle(this), DispatcherQueue,
                PlayPreviousTrack,
                () =>
                {
                    if (_playback.IsPlaying) PausePlayback();
                    else if (_playback.CurrentTrack is null && GetCurrentQueuePaths().FirstOrDefault() is { } path && _store.GetTrack(path) is { } firstTrack) PlayTrack(firstTrack, true);
                    else ResumePlayback();
                },
                () => AdvanceQueue(false));
            _taskbarPeekControls.Update(_playback.CurrentTrack is not null, _playback.IsPlaying);
        }
        catch (Exception ex) when (ex is COMException or InvalidOperationException)
        { LocalAppLog.Shared.Warning("taskbar-controls", "Could not initialize taskbar preview playback controls.", ex); }
    }

    private bool _immersiveMode;
    private bool _immersiveWasMaximized;

    private void SetImmersiveMode(bool enabled)
    {
        if (_immersiveMode == enabled) return;
        var hwnd = WindowNative.GetWindowHandle(this);
        var appWindow = AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd));
        if (enabled)
        {
            _immersiveWasMaximized = IsZoomed(hwnd);
            appWindow.SetPresenter(AppWindowPresenterKind.FullScreen);
            _immersiveMode = true;
            return;
        }

        appWindow.SetPresenter(AppWindowPresenterKind.Overlapped);
        if (_immersiveWasMaximized) ShowWindow(hwnd, SwMaximize);
        _immersiveMode = false;
    }

    private const int SwMaximize = 3;
    [DllImport("user32.dll", EntryPoint = "ShowWindow")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ShowWindow(nint hwnd, int command);

    [DllImport("user32.dll", EntryPoint = "IsZoomed")]
    private static extern bool IsZoomed(nint hwnd);

    private void SystemControls_ButtonPressed(SystemMediaTransportControls sender, SystemMediaTransportControlsButtonPressedEventArgs args)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            switch (args.Button)
            {
                case SystemMediaTransportControlsButton.Play:
                    if (_playback.CurrentTrack is null && GetCurrentQueuePaths().FirstOrDefault() is { } firstPath && _store.GetTrack(firstPath) is { } firstTrack)
                        PlayTrack(firstTrack, true);
                    else ResumePlayback();
                    break;
                case SystemMediaTransportControlsButton.Pause: PausePlayback(); break;
                case SystemMediaTransportControlsButton.Next: AdvanceQueue(false); break;
                case SystemMediaTransportControlsButton.Previous: PlayPreviousTrack(); break;
            }
        });
    }

    private void UpdateSystemMediaControls(Track? track, bool playing)
    {
        _taskbarPeekControls?.Update(track is not null || _queue.Count > 0, playing);
        if (_systemControls is null || track is null) return;
        _systemControls.IsEnabled = true;
        _systemControls.PlaybackStatus = playing ? MediaPlaybackStatus.Playing : MediaPlaybackStatus.Paused;
        var updater = _systemControls.DisplayUpdater; updater.Type = MediaPlaybackType.Music;
        updater.MusicProperties.Title = track.Title; updater.MusicProperties.Artist = track.Artist; updater.MusicProperties.AlbumTitle = track.Album;
        updater.Thumbnail = null;
        updater.Update();
        if (!string.IsNullOrWhiteSpace(track.ArtworkPath) && System.IO.File.Exists(track.ArtworkPath)) _ = UpdateMediaThumbnailAsync(track.ArtworkPath);
    }

    private async Task UpdateMediaThumbnailAsync(string path)
    {
        try
        {
            var file = await StorageFile.GetFileFromPathAsync(path);
            if (_systemControls is null || _playback.CurrentTrack?.ArtworkPath != path) return;
            var updater = _systemControls.DisplayUpdater;
            updater.Thumbnail = RandomAccessStreamReference.CreateFromFile(file);
            updater.Update();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or InvalidOperationException or System.Runtime.InteropServices.COMException)
        { LocalAppLog.Shared.Warning("media-artwork", $"Could not load artwork: {path}", ex); }
    }

    private void ApplyTraySetting()
    {
        try
        {
            if (_store.GetSetting("minimize-to-tray") == "true")
            {
                if (_tray is not null) { _tray.UpdateIcon(ActiveProductIconPath()); return; }
                _tray = new TrayIconService(WindowNative.GetWindowHandle(this), ActiveProductIconPath(),
                    () => DispatcherQueue.TryEnqueue(() => { var hwnd = WindowNative.GetWindowHandle(this); TrayIconService.RestoreWindow(hwnd); Activate(); }),
                    () => DispatcherQueue.TryEnqueue(() => TogglePlayback(StartPlaybackFromCurrentLibraryView)),
                    () => DispatcherQueue.TryEnqueue(PlayPreviousTrack),
                    () => DispatcherQueue.TryEnqueue(PlayNextTrack),
                    () => DispatcherQueue.TryEnqueue(ExitMusicPlayer));
            }
            else { _tray?.Dispose(); _tray = null; }
        }
        catch (System.ComponentModel.Win32Exception ex)
        {
            // A saved tray preference must not prevent startup when the Windows
            // notification area is unavailable. Keep normal window minimization.
            _tray?.Dispose(); _tray = null;
            LocalAppLog.Shared.Warning("tray", "The Windows notification area is unavailable; the player window remains accessible.", ex);
            _ = ShowNoticeAsync("The Windows tray is unavailable. Music Player will minimize to the taskbar.", InfoBarSeverity.Warning);
        }
    }

    private string ActiveProductIconPath()
    {
        var theme = _store.GetSetting("theme");
        var light = theme == "Light" || (theme != "Dark" && ShellRoot.ActualTheme == ElementTheme.Light);
        var suffix = light ? "light" : _store.GetSetting("accent-mode") == "Artwork" ? "accent" : "dark";
        return Path.Combine(AppContext.BaseDirectory, "Assets", $"MusicPlayer-{suffix}.ico");
    }

    private void ExitMusicPlayer()
    {
        var tray = _tray;
        _tray = null;
        tray?.Dispose();
        Close();
    }

    private Task ShowNoticeAsync(string message, InfoBarSeverity severity = InfoBarSeverity.Informational)
    {
        _webBridge?.SendEvent("notification", new { message, severity = severity.ToString() });
        if (_webUiActive) return Task.CompletedTask;
        _noticeDismissTimer?.Stop();
        AppNotice.Message = message;
        AppNotice.Severity = severity;
        AppNotice.IsOpen = true;
        if (severity is InfoBarSeverity.Informational or InfoBarSeverity.Success) _noticeDismissTimer?.Start();
        return Task.CompletedTask;
    }

    private void WebUiFailureClose_Click(object sender, RoutedEventArgs e) => ExitMusicPlayer();
}
