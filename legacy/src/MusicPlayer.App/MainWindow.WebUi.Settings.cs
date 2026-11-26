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
        var currentPreset = _liveEqualizerPreset ?? _store.GetSetting("eq-current") ?? "Off";
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

    private readonly SemaphoreSlim _equalizerUpdateGate = new(1, 1);
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? _equalizerSaveTimer;
    private string? _liveEqualizerPreset;
    private float[]? _liveEqualizerBands;
    private float _liveEqualizerPreamp = PlaybackService.NeutralEqualizerPreamp;
    private bool _equalizerSettingsDirty;

    private async Task SetEqualizerPresetAsync(string name)
    {
        await _equalizerUpdateGate.WaitAsync();
        try
        {
            var bands = await Task.Run(() =>
            {
                if (name == "Off") { _playback.ApplyEqualizer(null); return new float[10]; }
                var saved = _store.GetSetting("eq-preset:" + name);
                float[] values;
                if (saved is not null)
                {
                    values = ReadEqualizerBands(saved, 10);
                    _playback.ApplyEqualizer(null, values, ReadEqualizerPreamp(_store.GetSetting("eq-preset-preamp:" + name)));
                }
                else if (PlaybackService.EqualizerPresets().Contains(name, StringComparer.OrdinalIgnoreCase))
                {
                    values = PlaybackService.EqualizerPresetBands(name).ToArray();
                    _playback.ApplyEqualizer(name);
                }
                else throw new ArgumentException("That equalizer preset is unavailable.");
                return values;
            });
            _liveEqualizerPreset = name; _liveEqualizerBands = bands;
            _liveEqualizerPreamp = _playback.EqualizerPreamp;
            ScheduleEqualizerSave();
        }
        finally { _equalizerUpdateGate.Release(); }
    }

    private async Task SetEqualizerBandsAsync(float[] values)
    {
        if (values.Length != 10 || values.Any(value => !float.IsFinite(value))) throw new ArgumentException("Expected ten finite equalizer gains.");
        var bands = values.Select(value => Math.Clamp(value, -20, 20)).ToArray();
        await _equalizerUpdateGate.WaitAsync();
        try
        {
            if (_windowClosed) return;
            _liveEqualizerPreset = "Custom"; _liveEqualizerBands = bands;
            _liveEqualizerPreamp = _playback.EqualizerPreamp;
            ScheduleEqualizerSave();
            await Task.Run(() => _playback.ApplyEqualizer(null, bands));
        }
        finally { _equalizerUpdateGate.Release(); }
    }

    private float[] CurrentEqualizerBands(int count)
    {
        if (_liveEqualizerBands is not null) return (float[])_liveEqualizerBands.Clone();
        var preset = _liveEqualizerPreset = _store.GetSetting("eq-current") ?? "Off";
        if (preset.Equals("Off", StringComparison.OrdinalIgnoreCase)) _liveEqualizerBands = new float[count];
        else if (_store.GetSetting("eq-preset:" + preset) is { } saved) _liveEqualizerBands = ReadEqualizerBands(saved, count);
        else if (PlaybackService.EqualizerPresets().Contains(preset, StringComparer.OrdinalIgnoreCase)) _liveEqualizerBands = PlaybackService.EqualizerPresetBands(preset).ToArray();
        else _liveEqualizerBands = ReadEqualizerBands(_store.GetSetting("eq-bands"), count);
        return (float[])_liveEqualizerBands.Clone();
    }

    private void ScheduleEqualizerSave()
    {
        _equalizerSettingsDirty = true;
        if (_windowClosed) { PersistEqualizerOnClose(); return; }
        if (_equalizerSaveTimer is null)
        {
            _equalizerSaveTimer = DispatcherQueue.CreateTimer();
            _equalizerSaveTimer.Interval = TimeSpan.FromMilliseconds(700);
            _equalizerSaveTimer.IsRepeating = false;
            _equalizerSaveTimer.Tick += async (_, _) =>
            {
                await _equalizerUpdateGate.WaitAsync();
                try
                {
                    if (_windowClosed || !_equalizerSettingsDirty) return;
                    var settings = EqualizerSettingsSnapshot();
                    await Task.Run(() => _store.SetSettings(settings));
                    _equalizerSettingsDirty = false;
                }
                catch (Exception ex) { LocalAppLog.Shared.Warning("equalizer", "Could not save the equalizer curve.", ex); }
                finally { _equalizerUpdateGate.Release(); }
            };
        }
        _equalizerSaveTimer.Stop(); _equalizerSaveTimer.Start();
    }

    private Dictionary<string, string> EqualizerSettingsSnapshot() => new()
    {
        ["eq-current"] = _liveEqualizerPreset ?? "Off",
        ["eq-bands"] = JsonSerializer.Serialize(_liveEqualizerBands ?? new float[10]),
        ["eq-preamp"] = _liveEqualizerPreamp.ToString(CultureInfo.InvariantCulture)
    };

    private void PersistEqualizerOnClose()
    {
        _equalizerSaveTimer?.Stop();
        if (!_equalizerSettingsDirty) return;
        try { _store.SetSettings(EqualizerSettingsSnapshot()); _equalizerSettingsDirty = false; }
        catch (Exception ex) { LocalAppLog.Shared.Warning("equalizer", "Could not save the final equalizer curve.", ex); }
    }

    private async Task SaveEqualizerPresetAsync(string name)
    {
        name = name.Trim();
        if (name.Length is < 1 or > 60 || name.Equals("Off", StringComparison.OrdinalIgnoreCase) || name.Equals("Custom", StringComparison.OrdinalIgnoreCase) || PlaybackService.EqualizerPresets().Contains(name, StringComparer.OrdinalIgnoreCase))
            throw new ArgumentException("Choose a preset name that is different from the built-in presets.");
        await _equalizerUpdateGate.WaitAsync();
        try
        {
            var bands = CurrentEqualizerBands(10);
            var settings = new Dictionary<string, string>
            {
                ["eq-preset:" + name] = JsonSerializer.Serialize(bands), ["eq-bands"] = JsonSerializer.Serialize(bands), ["eq-current"] = name,
                ["eq-preamp"] = _playback.EqualizerPreamp.ToString(CultureInfo.InvariantCulture),
                ["eq-preset-preamp:" + name] = _playback.EqualizerPreamp.ToString(CultureInfo.InvariantCulture)
            };
            await Task.Run(() => _store.SetSettings(settings));
            _liveEqualizerPreset = name; _liveEqualizerBands = bands; _equalizerSettingsDirty = false;
            _liveEqualizerPreamp = _playback.EqualizerPreamp;
            _equalizerSaveTimer?.Stop();
        }
        finally { _equalizerUpdateGate.Release(); }
    }

    private static float ReadEqualizerPreamp(string? value) =>
        float.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var gain) && float.IsFinite(gain)
            ? Math.Clamp(gain, -20, 20) : PlaybackService.NeutralEqualizerPreamp;

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
