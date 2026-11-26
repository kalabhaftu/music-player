using System.Text.Json;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task<object?> HandleWebUiSettingsCommandAsync(string name, JsonElement payload)
    {
        switch (name)
        {
            case "setAudioDevice": await SetAudioDeviceAsync(String(payload, "id")); return null;
            case "setCrossfade": SetCrossfade(Int(payload, "seconds")); return null;
            case "refreshAudioDevices": return await AudioSettingsAsync();
            case "setEqualizerPreset": await SetEqualizerPresetAsync(String(payload, "name")); return null;
            case "setEqualizerBand":
            {
                var bands = CurrentEqualizerBands(10);
                var index = Int(payload, "index");
                if (index < 0 || index >= bands.Length) throw new ArgumentOutOfRangeException(nameof(index));
                bands[index] = (float)Double(payload, "value");
                await SetEqualizerBandsAsync(bands); return null;
            }
            case "setEqualizerBands":
                await SetEqualizerBandsAsync(payload.GetProperty("bands").EnumerateArray().Select(value => value.GetSingle()).ToArray()); return null;
            case "saveEqualizerPreset": await SaveEqualizerPresetAsync(String(payload, "name")); return null;
            case "updateSettings": await UpdateWebSettingsAsync(payload); return null;
            case "resetUiSettings":
            {
                var appearanceChanged = _playerSettings.ResetUiSettings(payload);
                if (appearanceChanged)
                {
                    _webArtworkAccent = null;
                    _webArtworkPalette = null;
                    ApplyStoredAppearance();
                    ApplyTraySetting();
                    ResetAccent();
                    _webBridge?.SendEvent("artworkAccentChanged", new { color = (string?)null, palette = (object?)null });
                }
                PublishSettingsChanged();
                PublishLibraryChanged();
                return WebSettings();
            }
            case "addExclusion": await AddExclusionAsync(); return null;
            case "removeExclusion": RemoveExclusion(String(payload, "path")); return null;
            case "setExtensionEnabled":
            {
                _libraryLocations.SetExtensionEnabled(String(payload, "extension"),
                    payload.TryGetProperty("enabled", out var enabled) && enabled.ValueKind == JsonValueKind.True,
                    String(payload, "kind", "Audio"));
                var videoExtensions = _libraryLocations.GetEnabledVideoExtensions();
                _playback.SetVideoExtensions(videoExtensions);
                _libraryQueries.SetVideoExtensions(videoExtensions);
                if (_playback.IsVideoMode && _playback.CurrentTrack is { } currentVideo &&
                    !_libraryLocations.IsVideoExtensionEnabled(currentVideo.Path))
                    CloseVideoPlaybackWindow(pauseVideo: true);
                var roots = _libraryLocations.GetLibraryRoots();
                _libraryFileWatcher.SetRoots(roots, _libraryLocations.GetScanExclusions(), _libraryLocations.GetEnabledExtensions());
                if (roots.Length > 0) StartScan(roots, forceRefresh: true);
                return null;
            }
            case "checkUpdates": await CheckForUpdatesAsync(true); return null;
            default: throw new InvalidOperationException("This Music Player command is not available in the settings handler.");
        }
    }
}
