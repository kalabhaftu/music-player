using System.Text.Json;
using System.Text;
using MusicPlayer.Core;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using Windows.Storage.Streams;

namespace MusicPlayer.App;

/// <summary>Owns the WebView2 boundary and only forwards named, explicitly allowed UI commands.</summary>
internal sealed class WebViewBridge(WebView2 view, WebUiCommandRouter commandRouter)
{
    private const string UiHost = "musicplayer.local";
    private const string ArtworkHost = "artwork.musicplayer.local";
    private const string AssetsHost = "assets.musicplayer.local";
    private const string UiResourcePrefix = "MusicPlayerWebUI/";
    private static readonly IReadOnlyDictionary<string, byte[]> EmbeddedUi = LoadEmbeddedUi();
    private CoreWebView2? _core;
    private readonly SemaphoreSlim _visibilityGate = new(1, 1);
    private bool _backgroundRequested;

    public async Task SetBackgroundAsync(bool background)
    {
        _backgroundRequested = background;
        await _visibilityGate.WaitAsync();
        try
        {
            if (_core is null) return;
            if (_backgroundRequested)
            {
                view.Visibility = Microsoft.UI.Xaml.Visibility.Collapsed;
                // Suspend sets the low memory target itself. Use the memory target
                // only as a fallback when suspension is refused by the runtime.
                try
                {
                    if (!await _core.TrySuspendAsync())
                        _core.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Low;
                }
                catch (InvalidOperationException)
                {
                    // XAML visibility can reach the controller a frame later. Keep
                    // the official low-memory fallback until the next restore.
                    _core.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Low;
                }
            }
            else
            {
                _core.Resume();
                _core.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Normal;
                view.Visibility = Microsoft.UI.Xaml.Visibility.Visible;
                SendEvent("windowRestored", new { });
            }
        }
        catch (Exception ex) when (ex is InvalidOperationException or ObjectDisposedException or System.Runtime.InteropServices.COMException)
        {
            LocalAppLog.Shared.Warning("web-ui", "Could not change WebView background state.", ex);
            if (!_backgroundRequested) view.Visibility = Microsoft.UI.Xaml.Visibility.Visible;
        }
        finally { _visibilityGate.Release(); }
    }

    public bool IsReady => _core is not null;
    public Action<bool>? FullscreenRequested { get; set; }

    public async Task InitializeAsync(string artworkFolder, string assetsFolder, string userDataFolder)
    {
        if (!EmbeddedUi.ContainsKey("index.html")) throw new InvalidOperationException("The embedded Music Player interface is missing.");
        var fullUserDataFolder = Path.GetFullPath(userDataFolder);
        Directory.CreateDirectory(fullUserDataFolder);
        var environment = await CoreWebView2Environment.CreateWithOptionsAsync(
            null, fullUserDataFolder, new CoreWebView2EnvironmentOptions());
        Exception? initializationFailure = null;
        void CaptureInitializationFailure(WebView2 _, CoreWebView2InitializedEventArgs args) => initializationFailure = args.Exception;
        view.CoreWebView2Initialized += CaptureInitializationFailure;
        try { await view.EnsureCoreWebView2Async(environment); }
        finally { view.CoreWebView2Initialized -= CaptureInitializationFailure; }
        _core = view.CoreWebView2 ?? throw new InvalidOperationException(
            "WebView2 initialization completed without a CoreWebView2 instance.", initializationFailure);
        _core.Settings.AreDefaultContextMenusEnabled = false;
        _core.Settings.AreDefaultScriptDialogsEnabled = false;
        _core.Settings.IsStatusBarEnabled = false;
        _core.Settings.IsZoomControlEnabled = false;
        _core.Settings.AreDevToolsEnabled = false;
        _core.AddWebResourceRequestedFilter($"https://{UiHost}/*", CoreWebView2WebResourceContext.All);
        _core.WebResourceRequested += ServeEmbeddedUi;
        if (Directory.Exists(artworkFolder))
            _core.SetVirtualHostNameToFolderMapping(ArtworkHost, Path.GetFullPath(artworkFolder), CoreWebView2HostResourceAccessKind.DenyCors);
        if (Directory.Exists(assetsFolder))
            _core.SetVirtualHostNameToFolderMapping(AssetsHost, Path.GetFullPath(assetsFolder), CoreWebView2HostResourceAccessKind.DenyCors);
        _core.NavigationStarting += NavigationStarting;
        _core.NavigationCompleted += NavigationCompleted;
        _core.NewWindowRequested += (_, args) => args.Handled = true;
        _core.ContainsFullScreenElementChanged += (sender, _) => FullscreenRequested?.Invoke(sender.ContainsFullScreenElement);
        _core.WebMessageReceived += MessageReceived;
        _core.Navigate($"https://{UiHost}/index.html");
    }

    private static IReadOnlyDictionary<string, byte[]> LoadEmbeddedUi()
    {
        var assembly = typeof(WebViewBridge).Assembly;
        var resources = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
        foreach (var resourceName in assembly.GetManifestResourceNames().Where(name => name.StartsWith(UiResourcePrefix, StringComparison.Ordinal)))
        {
            using var resource = assembly.GetManifestResourceStream(resourceName);
            if (resource is null) continue;
            using var content = new MemoryStream();
            resource.CopyTo(content);
            var path = resourceName[UiResourcePrefix.Length..].Replace('\\', '/');
            resources[path] = content.ToArray();
        }
        return resources;
    }

    private void ServeEmbeddedUi(object? sender, CoreWebView2WebResourceRequestedEventArgs args)
    {
        if (_core is null || !Uri.TryCreate(args.Request.Uri, UriKind.Absolute, out var uri) ||
            !string.Equals(uri.Host, UiHost, StringComparison.OrdinalIgnoreCase)) return;

        var path = Uri.UnescapeDataString(uri.AbsolutePath.TrimStart('/')).Replace('\\', '/');
        if (path.Split('/').Any(segment => segment is "." or "..") || !EmbeddedUi.TryGetValue(path, out var content))
        {
            args.Response = _core.Environment.CreateWebResourceResponse(
                CreateRandomAccessStream(Encoding.UTF8.GetBytes("Not found")), 404, "Not Found",
                "Content-Type: text/plain; charset=utf-8\r\nX-Content-Type-Options: nosniff");
            return;
        }

        args.Response = _core.Environment.CreateWebResourceResponse(
            CreateRandomAccessStream(content), 200, "OK",
            $"Content-Type: {ContentType(path)}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-cache");
    }

    private static IRandomAccessStream CreateRandomAccessStream(byte[] content)
    {
        var stream = new InMemoryRandomAccessStream();
        using (var writer = new DataWriter(stream.GetOutputStreamAt(0)))
        {
            writer.WriteBytes(content);
            writer.StoreAsync().AsTask().GetAwaiter().GetResult();
            writer.FlushAsync().AsTask().GetAwaiter().GetResult();
            writer.DetachStream();
        }
        stream.Seek(0);
        return stream;
    }

    private static string ContentType(string path) => Path.GetExtension(path).ToLowerInvariant() switch
    {
        ".html" => "text/html; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".js" => "text/javascript; charset=utf-8",
        ".json" => "application/json; charset=utf-8",
        ".svg" => "image/svg+xml",
        _ => "application/octet-stream"
    };

    public void SendEvent(string name, object? data)
    {
        if (_core is null || _backgroundRequested) return;
        try { _core.PostWebMessageAsJson(JsonSerializer.Serialize(new { type = "event", name, data })); }
        catch (Exception ex) when (ex is InvalidOperationException or ObjectDisposedException) { }
    }

    private static void NavigationStarting(CoreWebView2 sender, CoreWebView2NavigationStartingEventArgs args)
    {
        if (!Uri.TryCreate(args.Uri, UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps ||
            (uri.Host != UiHost && uri.Host != ArtworkHost && uri.Host != AssetsHost)) args.Cancel = true;
    }

    private static void NavigationCompleted(CoreWebView2 sender, CoreWebView2NavigationCompletedEventArgs args)
    {
        if (args.IsSuccess)
            LocalAppLog.Shared.Info("web-ui", $"Loaded local Music Player interface ({sender.DocumentTitle}).");
        else
            LocalAppLog.Shared.Warning("web-ui", $"The local Music Player interface failed to load ({args.WebErrorStatus}).");
    }

    private async void MessageReceived(CoreWebView2 sender, CoreWebView2WebMessageReceivedEventArgs args)
    {
        string? id = null;
        string? safeCommandName = null;
        try
        {
            if (!Uri.TryCreate(args.Source, UriKind.Absolute, out var origin) || origin.Scheme != Uri.UriSchemeHttps || origin.Host != UiHost) return;
            using var document = JsonDocument.Parse(args.WebMessageAsJson);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty("type", out var type) || type.GetString() != "command" ||
                !root.TryGetProperty("id", out var idValue) || idValue.ValueKind != JsonValueKind.String ||
                !root.TryGetProperty("name", out var nameValue) || nameValue.ValueKind != JsonValueKind.String) return;
            id = idValue.GetString();
            var name = nameValue.GetString();
            if (string.IsNullOrWhiteSpace(id) || string.IsNullOrWhiteSpace(name) || !commandRouter.CanRoute(name))
                throw new InvalidOperationException("This UI command is not available.");
            safeCommandName = name;
            var payload = root.TryGetProperty("payload", out var value) ? value.Clone() : JsonDocument.Parse("{}").RootElement.Clone();
            var result = await commandRouter.RouteAsync(name, payload);
            Respond(id, true, result, null);
        }
        catch (Exception ex)
        {
            if (safeCommandName is not null)
                LocalAppLog.Shared.Warning("web-ui-command", $"The '{safeCommandName}' command failed ({ex.GetType().Name}).");
            if (id is not null) Respond(id, false, null, ex.Message);
        }
    }

    private void Respond(string id, bool ok, object? data, string? error)
    {
        if (_core is null) return;
        try { _core.PostWebMessageAsJson(JsonSerializer.Serialize(new { type = "result", id, ok, data, error })); }
        catch (Exception ex) when (ex is InvalidOperationException or ObjectDisposedException) { }
    }
}
