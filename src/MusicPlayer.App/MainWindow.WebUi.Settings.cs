using System.Diagnostics;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
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
    private bool _needsDefaultSelection = true;
    private bool _refreshingDefaults;

    private async Task RefreshDefaultAppsAsync()
    {
        if (_refreshingDefaults || _windowClosed) return;
        _refreshingDefaults = true;
        try
        {
            _needsDefaultSelection = await Task.Run(DefaultAppsService.NeedsSelection);
            _webBridge?.SendEvent("defaultAppsChanged", new { needsDefaultSelection = _needsDefaultSelection });
        }
        catch (Exception ex) { LocalAppLog.Shared.Warning("associations", "Could not query Windows defaults.", ex); }
        finally { _refreshingDefaults = false; }
    }
    private object WebSettings()
        => _playerSettings.ProjectWebSettings(ShellRoot.ActualTheme == ElementTheme.Light ? "Light" : "Dark", _webArtworkAccent, _webArtworkPalette, _needsDefaultSelection);

    private async Task<object> AudioSettingsAsync()
    {
        var devices = new List<object> { new { id = "", name = "System default" } };
        var availableIds = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var devicesListed = false;
        try
        {
            var listed = await DeviceInformation.FindAllAsync(MediaDevice.GetAudioRenderSelector());
            devicesListed = true;
            var enabled = listed.Where(device => device.IsEnabled && !string.IsNullOrWhiteSpace(device.Id))
                .GroupBy(device => device.Id, StringComparer.OrdinalIgnoreCase).Select(group => group.First())
                .OrderBy(device => device.Name, StringComparer.CurrentCultureIgnoreCase).ToArray();
            foreach (var device in enabled) availableIds.Add(device.Id);
            devices.AddRange(enabled.Select(device => (object)new { id = device.Id, name = device.Name }));
        }
        catch (Exception ex) { LocalAppLog.Shared.Warning("audio-devices", "Could not list Windows audio devices.", ex); }
        var selectedId = _playback.AudioOutputDeviceId;
        var selectedName = _store.GetSetting("audio-output-name") ?? "System default";
        if (devicesListed && !string.IsNullOrWhiteSpace(selectedId) && !availableIds.Contains(selectedId))
        {
            CancelCrossfadeAndRestoreQueue();
            _playback.SelectAudioOutputDevice(null);
            _store.SetSetting("audio-output-device", "");
            _store.SetSetting("audio-output-name", "System default");
            selectedId = "";
        }
        if (selectedId.Length == 0) selectedName = "System default";
        var bands = PlaybackService.EqualizerBands();
        var currentPreset = _store.GetSetting("eq-current") ?? "Off";
        var values = CurrentEqualizerBands(bands.Count);
        var saved = _store.GetSettings("eq-preset:").Keys.Select(key => key["eq-preset:".Length..]);
        var presets = PlaybackService.EqualizerPresets().Concat(saved).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        return new { devices, selectedId, selectedName,
            crossfadeSeconds = _crossfadeSeconds, currentPreset,
            presets, bands = bands.Select((label, index) => new { label, value = values[index] }).ToArray() };
    }

    private async Task SetAudioDeviceAsync(string id)
    {
        var name = "System default";
        if (!string.IsNullOrWhiteSpace(id))
        {
            var devices = await DeviceInformation.FindAllAsync(MediaDevice.GetAudioRenderSelector());
            name = devices.FirstOrDefault(device => device.Id == id)?.Name ?? (_store.GetSetting("audio-output-name") ?? "Saved output");
        }
        CancelCrossfadeAndRestoreQueue();
        _playback.SelectAudioOutputDevice(id);
        id = _playback.AudioOutputDeviceId;
        if (id.Length == 0) name = "System default";
        _store.SetSetting("audio-output-device", id); _store.SetSetting("audio-output-name", name);
        await ShowNoticeAsync($"Audio output: {name}.");
    }

    private void SetCrossfade(int seconds)
    {
        CancelCrossfadeAndRestoreQueue();
        _crossfadeSeconds = seconds is 2 or 3 or 5 or 8 or 10 ? seconds : 0;
        _store.SetSetting("crossfade-seconds", _crossfadeSeconds.ToString(CultureInfo.InvariantCulture));
    }

    private void SetEqualizerPreset(string name)
    {
        if (name == "Off") { _playback.ApplyEqualizer(null); _store.SetSetting("eq-current", "Off"); return; }
        var saved = _store.GetSettings("eq-preset:");
        if (saved.TryGetValue("eq-preset:" + name, out var json)) _playback.ApplyEqualizer(null, ReadEqualizerBands(json, PlaybackService.EqualizerBands().Count));
        else if (PlaybackService.EqualizerPresets().Contains(name, StringComparer.OrdinalIgnoreCase)) _playback.ApplyEqualizer(name);
        else throw new ArgumentException("That equalizer preset is unavailable.");
        _store.SetSetting("eq-current", name);
    }

    private void SetEqualizerBand(int index, float value)
    {
        var labels = PlaybackService.EqualizerBands(); if (index < 0 || index >= labels.Count) throw new ArgumentOutOfRangeException(nameof(index));
        var bands = CurrentEqualizerBands(labels.Count);
        bands[index] = Math.Clamp(value, -20, 20);
        _playback.ApplyEqualizer(null, bands); _store.SetSetting("eq-current", "Custom"); _store.SetSetting("eq-bands", JsonSerializer.Serialize(bands));
    }

    private float[] CurrentEqualizerBands(int count)
    {
        var preset = _store.GetSetting("eq-current") ?? "Off";
        var saved = _store.GetSetting("eq-preset:" + preset);
        if (preset.Equals("Off", StringComparison.OrdinalIgnoreCase)) return new float[count];
        if (saved is not null) return ReadEqualizerBands(saved, count);
        if (PlaybackService.EqualizerPresets().Contains(preset, StringComparer.OrdinalIgnoreCase)) return PlaybackService.EqualizerPresetBands(preset).ToArray();
        return ReadEqualizerBands(_store.GetSetting("eq-bands"), count);
    }

    private void SaveEqualizerPreset(string name)
    {
        name = name.Trim(); if (name.Length is < 1 or > 60 || name.Equals("Off", StringComparison.OrdinalIgnoreCase) || name.Equals("Custom", StringComparison.OrdinalIgnoreCase) || PlaybackService.EqualizerPresets().Contains(name, StringComparer.OrdinalIgnoreCase))
            throw new ArgumentException("Choose a preset name that is different from the built-in presets.");
        var bands = CurrentEqualizerBands(PlaybackService.EqualizerBands().Count);
        var key = "eq-preset:" + name; _store.SetSetting(key, JsonSerializer.Serialize(bands)); _store.SetSetting("eq-bands", JsonSerializer.Serialize(bands)); _store.SetSetting("eq-current", name);
    }

    private async Task UpdateWebSettingsAsync(JsonElement payload)
    {
        var accentSettingsChanged = _playerSettings.PersistWebSettings(payload);
        ApplyStoredAppearance(); ApplyTraySetting();
        if (accentSettingsChanged)
        {
            if (_store.GetSetting("accent-mode") == "Artwork" && _playback.CurrentTrack is { } current)
                await ApplyArtworkAccentAsync(current.ArtworkPath);
            else
            {
                _webArtworkAccent = null;
                _webArtworkPalette = null;
                ResetAccent();
                _webBridge?.SendEvent("artworkAccentChanged", new { color = (string?)null, palette = (object?)null });
            }
        }
        PublishSettingsChanged();
    }

}
