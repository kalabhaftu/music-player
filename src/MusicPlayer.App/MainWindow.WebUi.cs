using MusicPlayer.Core;
using Microsoft.UI.Xaml;
using Windows.UI;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private bool _webUiInitializationStarted;
    private bool _webUiActive;
    private bool _webUiBootstrapped;
    private GitHubRelease? _webAvailableRelease;
    private string? _webArtworkAccent;
    private object? _webArtworkPalette;
    private Color? _webArtworkTitleBarColor;
    private long _lastWebQueueRevision = -1;
    private int _lastWebQueueIndex = -1;
    private DateTimeOffset _lastWebScanUpdateUtc;
    private readonly Dictionary<string, string> _webArtworkPaths = new(StringComparer.Ordinal);

    private async void WebUi_Loaded(object sender, RoutedEventArgs e)
    {
        if (_webUiInitializationStarted) return;
        _webUiInitializationStarted = true;
        await InitializeWebUiAsync();
    }

    private async Task InitializeWebUiAsync()
    {
        try
        {
            var commandRouter = new WebUiCommandRouter(
                HandleWebUiDataCommandAsync,
                HandleWebUiPlaybackCommandAsync,
                HandleWebUiPlaylistCommandAsync,
                HandleWebUiFolderCommandAsync,
                HandleWebUiMetadataCommandAsync,
                HandleWebUiSettingsCommandAsync,
                HandleWebUiUtilityCommandAsync);
            _webBridge = new(WebUi, commandRouter);
            await _webBridge.InitializeAsync(Path.Combine(_appData, "Artwork"),
                Path.Combine(AppContext.BaseDirectory, "Assets"),
                Path.Combine(_appData, "WebView2"));
            _webUiActive = true;
            PublishPlaybackState();
            PublishQueueState(force: true);
            PublishScanState();
        }
        catch (Exception ex)
        {
            WebUiLoadingState.Visibility = Visibility.Collapsed;
            WebUi.Visibility = Visibility.Collapsed;
            WebUiFailureMessage.Text = ex.Message;
            WebUiFailureState.Visibility = Visibility.Visible;
            LocalAppLog.Shared.Error("web-ui", "Could not initialize the Music Player interface.", ex);
        }
    }

}
