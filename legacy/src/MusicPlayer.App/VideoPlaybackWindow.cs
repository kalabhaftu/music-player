using System.Runtime.InteropServices;
using MusicPlayer.Core;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.UI;
using WinRT.Interop;
using VirtualKey = Windows.System.VirtualKey;

namespace MusicPlayer.App;

/// <summary>A separate, ordinary Windows window for opted-in video files.</summary>
internal sealed class VideoPlaybackWindow : Window
{
    private readonly PlaybackService _playback;
    private readonly Action _togglePlayback;
    private readonly Action _previous;
    private readonly Action _next;
    private readonly Action<long> _seek;
    private readonly Action<int> _setVolume;
    private readonly Action<string> _notify;
    private readonly Action<VideoPlaybackWindow, bool> _closedCallback;
    private readonly VideoSurfaceHost _surface;
    private readonly Grid _root;
    private readonly StackPanel _details;
    private readonly AppWindow _appWindow;
    private readonly TextBlock _positionText = new() { FontSize = 12, MinWidth = 42, VerticalAlignment = VerticalAlignment.Center };
    private readonly TextBlock _durationText = new() { FontSize = 12, MinWidth = 42, TextAlignment = TextAlignment.Right, VerticalAlignment = VerticalAlignment.Center };
    private readonly Slider _position = new() { Minimum = 0, Maximum = 1, StepFrequency = 1, VerticalAlignment = VerticalAlignment.Center };
    private readonly Slider _volume = new() { Minimum = 0, Maximum = 100, StepFrequency = 1, Value = 75, Width = 70, VerticalAlignment = VerticalAlignment.Center };
    private readonly Button _playPause = CreateIconButton("\uE769", "Pause", 44, 38);
    private readonly Button _repeatButton = CreateIconButton("\uE8EE", "Repeat off", 38, 36);
    private readonly Button _subtitleButton = CreateIconButton("\uE7F6", "Subtitles", 38, 36);
    private readonly Button _snapshotButton = CreateIconButton("\uE722", "Save a screenshot of the current frame", 38, 36);
    private readonly Button _fullScreenButton = CreateIconButton("\uE740", "Enter full screen (F11)", 38, 36);
    private readonly ComboBox _speed = new() { Width = 68, MinHeight = 36, VerticalAlignment = VerticalAlignment.Center };
    private readonly DispatcherQueueTimer _clock;
    private readonly DispatcherQueueTimer _seekDebounce;
    private readonly List<nint> _iconHandles = [];
    private string? _iconPath;
    private bool _syncing;
    private bool _scrubbing;
    private bool _ownerClosing;
    private bool _isFullScreen;
    private bool _repeatCurrent;

    public bool RepeatCurrent => _repeatCurrent;

    public VideoPlaybackWindow(
        Track track,
        PlaybackService playback,
        Action togglePlayback,
        Action previous,
        Action next,
        Action<long> seek,
        Action<int> setVolume,
        Action<string> notify,
        Action<VideoPlaybackWindow, bool> closedCallback)
    {
        _playback = playback;
        _togglePlayback = togglePlayback;
        _previous = previous;
        _next = next;
        _seek = seek;
        _setVolume = setVolume;
        _notify = notify;
        _closedCallback = closedCallback;
        Title = $"Music Player Video — {track.Title}";

        var windowHandle = WindowNative.GetWindowHandle(this);
        _appWindow = AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(windowHandle));
        _appWindow.Title = Title;
        _surface = new VideoSurfaceHost(windowHandle);

        _root = new Grid { Background = new SolidColorBrush(Color.FromArgb(255, 12, 13, 12)), MinWidth = 520, MinHeight = 260 };
        _root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        _root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var stage = new Grid { Background = new SolidColorBrush(Color.FromArgb(255, 0, 0, 0)), MinHeight = 140 };
        stage.SizeChanged += (_, _) => UpdateSurfaceBounds(stage);
        Grid.SetRow(stage, 0);
        _root.Children.Add(stage);

        _details = new StackPanel { Spacing = 4, Padding = new Thickness(12, 7, 12, 7) };

        var timeline = new Grid { ColumnSpacing = 9 };
        timeline.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        timeline.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        timeline.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(_positionText, 0);
        Grid.SetColumn(_position, 1);
        Grid.SetColumn(_durationText, 2);
        timeline.Children.Add(_positionText);
        timeline.Children.Add(_position);
        timeline.Children.Add(_durationText);
        AutomationProperties.SetName(_position, "Video position");
        AutomationProperties.SetName(_volume, "Video volume");
        AutomationProperties.SetName(_speed, "Playback speed");
        AutomationProperties.SetAutomationId(_fullScreenButton, "VideoFullscreen");
        AutomationProperties.SetAutomationId(_playPause, "VideoPlayPause");
        _details.Children.Add(timeline);

        var controls = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Center };
        var previousButton = CreateIconButton("\uE892", "Previous track", 38, 36);
        var nextButton = CreateIconButton("\uE893", "Next track", 38, 36);
        previousButton.Click += (_, _) => _previous();
        nextButton.Click += (_, _) => _next();
        _fullScreenButton.Click += (_, _) => ToggleFullScreen();
        _repeatButton.Click += (_, _) => ToggleRepeatCurrent();
        _subtitleButton.Click += (_, _) => ShowSubtitleOptions();
        _snapshotButton.Click += (_, _) => SaveFrameScreenshot();
        _playPause.Click += (_, _) => _togglePlayback();

        var volumePanel = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, VerticalAlignment = VerticalAlignment.Center };
        var volumeIcon = new FontIcon { Glyph = "\uE767", FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 16 };
        AutomationProperties.SetName(volumeIcon, "Volume");
        volumePanel.Children.Add(volumeIcon);
        volumePanel.Children.Add(_volume);
        ToolTipService.SetToolTip(_volume, "Volume");

        foreach (var rate in new[] { .5f, .75f, 1f, 1.25f, 1.5f, 2f })
        {
            var item = new ComboBoxItem { Content = $"{rate:0.##}×", Tag = rate };
            _speed.Items.Add(item);
            if (Math.Abs(_playback.VideoPlaybackRate - rate) < .01f) _speed.SelectedItem = item;
        }
        ToolTipService.SetToolTip(_speed, "Playback speed");
        if (_speed.SelectedItem is null) _speed.SelectedIndex = 2;
        _speed.SelectionChanged += (_, _) =>
        {
            if (_speed.SelectedItem is not ComboBoxItem { Tag: float rate }) return;
            if (!_playback.SetVideoPlaybackRate(rate)) _notify("The media engine could not apply that playback speed.");
        };

        controls.Children.Add(previousButton);
        controls.Children.Add(_playPause);
        controls.Children.Add(nextButton);
        controls.Children.Add(volumePanel);
        controls.Children.Add(_speed);
        controls.Children.Add(_repeatButton);
        controls.Children.Add(_subtitleButton);
        controls.Children.Add(_snapshotButton);
        controls.Children.Add(_fullScreenButton);
        _details.Children.Add(controls);
        Grid.SetRow(_details, 1);
        _root.Children.Add(_details);
        Content = _root;

        AddKeyboardAccelerators();

        _position.PointerPressed += (_, _) => _scrubbing = true;
        _position.PointerReleased += (_, _) => { _scrubbing = false; CommitSeek(); };
        _position.ValueChanged += Position_ValueChanged;
        _volume.ValueChanged += Volume_ValueChanged;

        var dispatcher = DispatcherQueue.GetForCurrentThread();
        _clock = dispatcher.CreateTimer();
        _clock.Interval = TimeSpan.FromMilliseconds(200);
        _clock.IsRepeating = true;
        _clock.Tick += (_, _) => RefreshPlaybackState();
        _seekDebounce = dispatcher.CreateTimer();
        _seekDebounce.Interval = TimeSpan.FromMilliseconds(160);
        _seekDebounce.IsRepeating = false;
        _seekDebounce.Tick += (_, _) => { if (!_scrubbing) CommitSeek(); };
        _clock.Start();

        Closed += (_, _) =>
        {
            _clock.Stop();
            _seekDebounce.Stop();
            try { _closedCallback(this, !_ownerClosing); }
            catch (Exception ex) { LocalAppLog.Shared.Error("video-window", "Closing the video window failed to update playback state.", ex); }
            finally
            {
                _surface.Dispose();
                foreach (var icon in _iconHandles) _ = DestroyIcon(icon);
                _iconHandles.Clear();
            }
        };
        RefreshPlaybackState();
        _surface.SetBounds(0, 0, 640, 360, visible: true);
    }

    public nint SurfaceHandle => _surface.Handle;

    public void ApplyTheme(ElementTheme requestedTheme, ElementTheme actualTheme, string iconPath)
    {
        _root.RequestedTheme = requestedTheme;
        ApplyNativeChrome(requestedTheme == ElementTheme.Dark ||
            (requestedTheme == ElementTheme.Default && actualTheme == ElementTheme.Dark), iconPath);
    }

    public void SetTrack(Track track)
    {
        UpdateWindowTitle(track);
        foreach (var item in _speed.Items.OfType<ComboBoxItem>())
            if (item.Tag is float rate && Math.Abs(_playback.VideoPlaybackRate - rate) < .01f) _speed.SelectedItem = item;
        RefreshPlaybackState();
    }

    public void CloseFromOwner()
    {
        _ownerClosing = true;
        Close();
    }

    private void UpdateWindowTitle(Track track)
    {
        var artist = string.IsNullOrWhiteSpace(track.Artist) ? "Unknown artist" : track.Artist;
        Title = $"{track.Title} — {artist}";
        if (_appWindow is not null) _appWindow.Title = Title;
    }

    private void ApplyNativeChrome(bool dark, string iconPath)
    {
        try
        {
            if (!string.Equals(_iconPath, iconPath, StringComparison.OrdinalIgnoreCase) && File.Exists(iconPath))
            {
                _appWindow.SetIcon(iconPath);
                _iconPath = iconPath;
                var hwnd = WindowNative.GetWindowHandle(this);
                var small = LoadImage(IntPtr.Zero, iconPath, ImageIcon, 16, 16, LoadFromFile);
                var large = LoadImage(IntPtr.Zero, iconPath, ImageIcon, 32, 32, LoadFromFile);
                if (small != IntPtr.Zero)
                {
                    _iconHandles.Add(small);
                    _ = SendMessage(hwnd, WmSetIcon, new nint(IconSmall), small);
                }
                if (large != IntPtr.Zero)
                {
                    _iconHandles.Add(large);
                    _ = SendMessage(hwnd, WmSetIcon, new nint(IconBig), large);
                }
            }

            var titleBar = _appWindow.TitleBar;
            titleBar.IconShowOptions = IconShowOptions.ShowIconAndSystemMenu;
            if (!AppWindowTitleBar.IsCustomizationSupported()) return;

            var background = dark ? Color.FromArgb(255, 16, 18, 16) : Color.FromArgb(255, 240, 242, 239);
            var foreground = dark ? Color.FromArgb(255, 247, 248, 246) : Color.FromArgb(255, 25, 29, 25);
            var inactiveForeground = dark ? Color.FromArgb(255, 160, 167, 159) : Color.FromArgb(255, 116, 124, 116);
            var hoverBackground = dark ? Color.FromArgb(255, 36, 38, 36) : Color.FromArgb(255, 212, 217, 211);
            var pressedBackground = dark ? Color.FromArgb(255, 48, 52, 48) : Color.FromArgb(255, 200, 206, 199);
            titleBar.BackgroundColor = background;
            titleBar.ForegroundColor = foreground;
            titleBar.ButtonBackgroundColor = background;
            titleBar.ButtonForegroundColor = foreground;
            titleBar.ButtonInactiveBackgroundColor = background;
            titleBar.ButtonInactiveForegroundColor = inactiveForeground;
            titleBar.ButtonHoverBackgroundColor = hoverBackground;
            titleBar.ButtonHoverForegroundColor = foreground;
            titleBar.ButtonPressedBackgroundColor = pressedBackground;
            titleBar.ButtonPressedForegroundColor = foreground;
        }
        catch (Exception ex) when (ex is InvalidOperationException or COMException or ArgumentException or IOException or UnauthorizedAccessException)
        {
            LocalAppLog.Shared.Warning("video-window", "The video window icon or title-bar theme could not be applied.", ex);
        }
    }

    private void AddKeyboardAccelerators()
    {
        var fullScreen = new KeyboardAccelerator { Key = VirtualKey.F11 };
        fullScreen.Invoked += (_, args) =>
        {
            ToggleFullScreen();
            args.Handled = true;
        };
        _root.KeyboardAccelerators.Add(fullScreen);

        var escape = new KeyboardAccelerator { Key = VirtualKey.Escape };
        escape.Invoked += (_, args) =>
        {
            if (!_isFullScreen) return;
            SetFullScreen(false);
            args.Handled = true;
        };
        _root.KeyboardAccelerators.Add(escape);
    }

    private void ToggleFullScreen() => SetFullScreen(!_isFullScreen);

    private void ToggleRepeatCurrent()
    {
        _repeatCurrent = !_repeatCurrent;
        var label = _repeatCurrent ? "Repeat this video" : "Repeat off";
        _repeatButton.Content = new FontIcon
        {
            Glyph = _repeatCurrent ? "\uE8ED" : "\uE8EE",
            FontFamily = new FontFamily("Segoe MDL2 Assets"),
            FontSize = 16
        };
        _repeatButton.Foreground = new SolidColorBrush(_repeatCurrent
            ? Color.FromArgb(255, 153, 255, 50)
            : Color.FromArgb(255, 245, 247, 244));
        AutomationProperties.SetName(_repeatButton, label);
        ToolTipService.SetToolTip(_repeatButton, label);
    }

    private void ShowSubtitleOptions()
    {
        var menu = new MenuFlyout();
        var options = _playback.GetVideoSubtitleOptions();
        var selectedId = _playback.VideoSubtitleId;
        AddSubtitleMenuItem(menu, selectedId == -1 ? "✓ Off" : "Off", -1);
        if (options.Count > 0) menu.Items.Add(new MenuFlyoutSeparator());
        foreach (var option in options)
            AddSubtitleMenuItem(menu, selectedId == option.Id ? $"✓ {option.Name}" : option.Name, option.Id);
        if (options.Count == 0)
        {
            var empty = new MenuFlyoutItem { Text = "No embedded or matching sidecar subtitles" };
            empty.IsEnabled = false;
            menu.Items.Add(empty);
        }
        menu.ShowAt(_subtitleButton);
    }

    private void AddSubtitleMenuItem(MenuFlyout menu, string label, int trackId)
    {
        var item = new MenuFlyoutItem { Text = label };
        item.Click += (_, _) =>
        {
            if (!_playback.SetVideoSubtitle(trackId)) _notify("The media engine could not change the subtitle track.");
        };
        menu.Items.Add(item);
    }

    private void SaveFrameScreenshot()
    {
        try
        {
            var pictures = Environment.GetFolderPath(Environment.SpecialFolder.MyPictures);
            if (string.IsNullOrWhiteSpace(pictures)) pictures = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Pictures");
            var folder = Path.Combine(pictures, "Music Player", "Screenshots");
            Directory.CreateDirectory(folder);
            var track = _playback.CurrentTrack;
            var stem = string.IsNullOrWhiteSpace(track?.Path) ? "video" : Path.GetFileNameWithoutExtension(track.Path);
            var invalidCharacters = Path.GetInvalidFileNameChars();
            var safeStem = new string(stem.Select(character => invalidCharacters.Contains(character) ? '_' : character).ToArray()).Trim().TrimEnd('.');
            if (safeStem.Length > 80) safeStem = safeStem[..80];
            if (string.IsNullOrWhiteSpace(safeStem)) safeStem = "video";
            var screenshotPath = Path.Combine(folder, $"{safeStem}-{DateTime.Now:yyyyMMdd-HHmmss-fff}.png");
            if (!_playback.TakeVideoSnapshot(screenshotPath))
            {
                _notify("The media engine could not capture the current video frame.");
                return;
            }
            _notify($"Screenshot saved in Pictures\\Music Player\\Screenshots: {Path.GetFileName(screenshotPath)}");
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or InvalidOperationException)
        {
            LocalAppLog.Shared.Warning("video-screenshot", "Could not save a screenshot of the current video frame.", ex);
            _notify("Could not save the video screenshot. Check the Pictures folder permissions.");
        }
    }

    private void SetFullScreen(bool enabled)
    {
        if (_isFullScreen == enabled) return;
        try
        {
            _appWindow.SetPresenter(enabled ? AppWindowPresenterKind.FullScreen : AppWindowPresenterKind.Overlapped);
            _isFullScreen = enabled;
            _details.Visibility = enabled ? Visibility.Collapsed : Visibility.Visible;
            // The transport that held focus has just been collapsed. Give the
            // fullscreen layout its own keyboard focus for Escape/F11.
            _root.IsTabStop = enabled;
            var label = enabled ? "Exit full screen (Esc)" : "Enter full screen (F11)";
            AutomationProperties.SetName(_fullScreenButton, label);
            ToolTipService.SetToolTip(_fullScreenButton, label);
            DispatcherQueue.TryEnqueue(() =>
            {
                if (_isFullScreen) _root.Focus(FocusState.Programmatic);
                else _fullScreenButton.Focus(FocusState.Programmatic);
            });
        }
        catch (Exception ex) when (ex is InvalidOperationException or COMException or ArgumentException)
        {
            LocalAppLog.Shared.Warning("video-window", "Could not change the video window full-screen state.", ex);
        }
    }

    private void UpdateSurfaceBounds(Grid stage)
    {
        try { _surface.SetBounds(0, 0, stage.ActualWidth, stage.ActualHeight, visible: true); }
        catch (InvalidOperationException ex) { LocalAppLog.Shared.Warning("video-window", "Could not resize the in-app video surface.", ex); }
    }

    private void Position_ValueChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_syncing) return;
        _positionText.Text = FormatTime(e.NewValue);
        _seekDebounce.Stop();
        _seekDebounce.Start();
    }

    private void Volume_ValueChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (!_syncing) _setVolume((int)Math.Round(e.NewValue));
    }

    private void CommitSeek()
    {
        if (!_playback.IsVideoMode) return;
        _seek(Math.Max(0, (long)(_position.Value * 1000)));
    }

    private void RefreshPlaybackState()
    {
        if (!_playback.IsVideoMode) return;
        var duration = Math.Max(0, _playback.Duration) / 1000d;
        var position = Math.Max(0, _playback.Position) / 1000d;
        _syncing = true;
        _position.Maximum = Math.Max(1, duration);
        if (!_scrubbing && !_seekDebounce.IsRunning) _position.Value = Math.Clamp(position, 0, _position.Maximum);
        _positionText.Text = FormatTime(position);
        _durationText.Text = FormatTime(duration);
        _volume.Value = Math.Clamp(_playback.Volume, 0, 100);
        _playPause.Content = new FontIcon
        {
            Glyph = _playback.IsPlaying ? "\uE769" : "\uE768",
            FontFamily = new FontFamily("Segoe MDL2 Assets"),
            FontSize = 18
        };
        var playPauseName = _playback.IsPlaying ? "Pause" : "Play";
        AutomationProperties.SetName(_playPause, playPauseName);
        ToolTipService.SetToolTip(_playPause, playPauseName);
        _syncing = false;
    }

    private static string FormatTime(double seconds)
    {
        var value = Math.Max(0, (int)Math.Floor(double.IsFinite(seconds) ? seconds : 0));
        return value >= 3600 ? $"{value / 3600}:{value / 60 % 60:00}:{value % 60:00}" : $"{value / 60}:{value % 60:00}";
    }

    private static Button CreateIconButton(string glyph, string label, double width = 46, double height = 42)
    {
        var icon = new FontIcon { Glyph = glyph, FontFamily = new FontFamily("Segoe MDL2 Assets"), FontSize = 17 };
        var button = new Button
        {
            Content = icon,
            Width = width,
            MinWidth = width,
            Height = height,
            Padding = new Thickness(0),
            Background = new SolidColorBrush(Color.FromArgb(255, 38, 40, 38)),
            Foreground = new SolidColorBrush(Color.FromArgb(255, 245, 247, 244))
        };
        AutomationProperties.SetName(button, label);
        ToolTipService.SetToolTip(button, label);
        return button;
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
}
