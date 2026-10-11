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
    private bool MoveQueueEntry(int index, int direction, int? targetIndex = null)
    {
        var changed = targetIndex is { } destination
            ? _playbackQueue.MoveEntryTo(index, destination)
            : _playbackQueue.MoveEntry(index, direction);
        if (!changed) return false;
        SaveSession();
        PublishQueueStateIfChanged();
        return true;
    }

    private bool RemoveQueueEntry(int index)
    {
        if (!_playbackQueue.RemoveEntry(index)) return false;
        SaveSession();
        PublishQueueStateIfChanged();
        return true;
    }

    private void ClearUpcomingQueue()
    {
        _playbackQueue.ClearUpcoming(_playback.CurrentTrack?.Path);
        SaveSession();
        PublishQueueStateIfChanged();
    }

    private async Task PlayQueueEntryAsync(int index)
    {
        if (index < 0 || index >= _queue.Count)
        {
            ApplyPlaybackCommandResult(_playbackCommands.PlayQueueEntry(index));
            return;
        }

        var track = await ResolveTrackForPlaybackAsync(_libraryQueries.TrackId(_queue[index]));
        if (track is not null) ApplyPlaybackCommandResult(_playbackCommands.StartTrack(track, resetQueue: false, index));
    }

    private void PlayTrack(Track track, bool resetQueue, int? queueIndex = null)
    {
        ApplyPlaybackCommandResult(_playbackCommands.StartTrack(track, resetQueue, queueIndex));
    }

    private void ApplyPlaybackCommandResult(PlaybackCommandResult result)
    {
        switch (result.Kind)
        {
            case PlaybackCommandKind.NoChange:
                return;
            case PlaybackCommandKind.TrackStarted:
            {
                var track = result.Track;
                if (track is null) return;
                if (!_playback.IsVideoMode) CloseVideoPlaybackWindow(pauseVideo: false);
                var availabilityChanged = _trackAvailability.MarkAvailable(track.Path);
                _playbackListening.ResetForTrack();
                _crossfadeInProgress = false;
                _crossfadeFailureSource = null;
                _crossfadeSourceQueueIndex = null;
                UpdateCurrentTrack(track);
                UpdateSystemMediaControls(track, true);
                if (result.PersistSession) SaveSession();
                if (result.QueueChanged) PublishQueueState(force: true);
                else PublishQueueStateIfChanged();
                if (availabilityChanged) PublishLibraryChanged();
                PublishPlaybackState();
                return;
            }
            case PlaybackCommandKind.Paused:
                if (_systemControls is not null) _systemControls.PlaybackStatus = MediaPlaybackStatus.Paused;
                PublishPlaybackState();
                return;
            case PlaybackCommandKind.Resumed:
                _playbackListening.MarkResumed(_playback.Position);
                if (result.Track is { } resumedTrack) UpdateSystemMediaControls(resumedTrack, true);
                PublishPlaybackState();
                return;
            case PlaybackCommandKind.PositionReset:
                PublishPlaybackState();
                return;
            case PlaybackCommandKind.CrossfadeRequested:
                if (result.Crossfade is { } request) StartCrossfade(request);
                return;
            case PlaybackCommandKind.StopPlayback:
                CancelCrossfadeAndRestoreQueue();
                _playback.Stop();
                if (_systemControls is not null) _systemControls.PlaybackStatus = MediaPlaybackStatus.Stopped;
                if (result.QueueChanged) PublishQueueState(force: true);
                PublishPlaybackState();
                return;
            case PlaybackCommandKind.TrackUnavailable:
                if (result.Track is { } missing && !File.Exists(missing.Path))
                {
                    var fileConfirmedMissing = ReconcileMissingTrack(missing.Path);
                    var notice = fileConfirmedMissing
                        ? $"File not found: {Path.GetFileName(missing.Path)}. Its stale library entry was removed; playlist and queue references were kept."
                        : $"The file location is unavailable: {Path.GetFileName(missing.Path)}. Its library and playlist references were kept; reconnect the drive and rescan when it is available.";
                    _ = ShowNoticeAsync(notice, InfoBarSeverity.Warning);
                }
                else _ = ShowNoticeAsync(result.Notice ?? "That track is no longer available.", InfoBarSeverity.Warning);
                return;
            case PlaybackCommandKind.AdvanceFallback:
                AdvanceQueue(false);
                return;
            default:
                throw new InvalidOperationException("Unknown playback command result.");
        }
    }

    public async Task PlayExternalFilesAsync(IReadOnlyList<string> paths)
    {
        await _externalFileActionGate.WaitAsync();
        try
        {
            var tracks = await ReadExternalTracksAsync(paths);
            if (tracks.Count == 0) { await ShowNoticeAsync("No supported audio files could be opened."); return; }

            _store.UpsertTracks(tracks);
            CancelCrossfadeAndRestoreQueue();
            var nextQueue = QueueNavigation.BuildExternalOpenQueue(
                tracks.Select(track => track.Path), _playback.CurrentTrack?.Path, _queue, _queueIndex);
            _playbackQueue.Replace(nextQueue, 0);
            PlayTrack(tracks[0], false, 0);
            PublishLibraryChanged();
            PublishQueueStateIfChanged();
            if (tracks.Count > 1) await ShowNoticeAsync($"Playing {tracks[0].Title}; {tracks.Count - 1} more selected tracks follow, then the previous track and queue.");
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("file-activation", "Could not start the selected audio files.", ex);
            await ShowNoticeAsync("Music Player could not open those audio files. See the local log for details.");
        }
        finally { _externalFileActionGate.Release(); }
    }

    public async Task AddExternalFilesToQueueAsync(IReadOnlyList<string> paths)
    {
        await _externalFileActionGate.WaitAsync();
        try
        {
            var tracks = await ReadExternalTracksAsync(paths);
            if (tracks.Count == 0) { await ShowNoticeAsync("No supported audio files could be added."); return; }

            _store.UpsertTracks(tracks);
            _playbackQueue.Append(tracks.Select(track => track.Path), _playback.CurrentTrack?.Path);
            PublishLibraryChanged();
            SaveSession();
            PublishQueueStateIfChanged();
            await ShowNoticeAsync($"Added {tracks.Count} { (tracks.Count == 1 ? "track" : "tracks") } to the queue.");
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("file-activation", "Could not add the selected audio files to the queue.", ex);
            await ShowNoticeAsync("Music Player could not add those files. See the local log for details.");
        }
        finally { _externalFileActionGate.Release(); }
    }

    public async Task CreatePlaylistFromExternalFilesAsync(IReadOnlyList<string> paths)
    {
        var name = new TextBox { PlaceholderText = "Playlist name", MinWidth = 300, MaxLength = 120 };
        AutomationProperties.SetName(name, "New playlist name");
        var dialog = new ContentDialog
        {
            Title = "Create playlist from files",
            Content = name,
            PrimaryButtonText = "Create playlist",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
            XamlRoot = ShellRoot.XamlRoot
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        if (string.IsNullOrWhiteSpace(name.Text)) { await ShowNoticeAsync("Enter a playlist name first."); return; }

        await _externalFileActionGate.WaitAsync();
        try
        {
            var tracks = await ReadExternalTracksAsync(paths);
            if (tracks.Count == 0) { await ShowNoticeAsync("No supported audio files could be added to the playlist."); return; }
            _store.UpsertTracks(tracks);
            _store.CreatePlaylist(name.Text.Trim(), tracks.Select(track => track.Path));
            PublishLibraryChanged();
            await ShowNoticeAsync($"Created playlist “{name.Text.Trim()}” with {tracks.Count} {(tracks.Count == 1 ? "track" : "tracks")}.");
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("file-activation", "Could not create a playlist from the selected files.", ex);
            await ShowNoticeAsync("Music Player could not create that playlist. See the local log for details.");
        }
        finally { _externalFileActionGate.Release(); }
    }

    private async Task<IReadOnlyList<Track>> ReadExternalTracksAsync(IReadOnlyList<string> paths)
    {
        var validPaths = paths.Where(path => (LibraryScanner.IsSupportedAudioFile(path) ||
            _libraryLocations.IsVideoExtensionEnabled(path)) && File.Exists(path)).ToArray();
        if (validPaths.Length == 0) return [];
        var artworkDirectory = Path.Combine(MusicPlayer.Core.AppDataPaths.Root, "Artwork");
        return await Task.Run(() =>
        {
            var tracks = new List<Track>(validPaths.Length);
            foreach (var path in validPaths)
            {
                try { tracks.Add(TrackReader.Read(path, artworkDirectory)); }
                catch (Exception ex)
                {
                    LocalAppLog.Shared.Warning("file-activation", "Track tags could not be read; opening the file with its name instead.", ex);
                    try
                    {
                        var info = new FileInfo(path);
                        tracks.Add(new Track(Path.GetFullPath(path), Path.GetFileNameWithoutExtension(path), string.Empty, string.Empty,
                            string.Empty, string.Empty, 0, 0, TimeSpan.Zero, info.Length, info.LastWriteTimeUtc, DateTime.UtcNow));
                    }
                    catch (Exception fileError) when (fileError is IOException or UnauthorizedAccessException)
                    {
                        LocalAppLog.Shared.Warning("file-activation", "A selected audio file could not be accessed.", fileError);
                    }
                }
            }
            return tracks;
        });
    }

    private void PublishQueueStateIfChanged() => PublishQueueState();

    private IReadOnlyList<string> GetCurrentQueuePaths()
    {
        return _libraryQueries.CurrentTrackPaths();
    }

    private bool TogglePlayback(Func<bool> startWhenEmpty)
    {
        if (_playback.CurrentTrack is null)
            return startWhenEmpty();

        if (_playback.IsPlaying) CancelCrossfadeAndRestoreQueue();
        var result = _playbackCommands.Toggle();
        ApplyPlaybackCommandResult(result);
        return result.Kind == PlaybackCommandKind.Resumed;
    }

    private bool StartPlaybackFromCurrentLibraryView()
    {
        var firstPath = _libraryQueries.CurrentTrackPaths().FirstOrDefault();
        if (firstPath is null || _store.GetTrack(firstPath) is not { } first) return false;
        _playbackQueue.Replace([firstPath], 0);
        PlayTrack(first, false, 0);
        return true;
    }

    private void PausePlayback()
    {
        CancelCrossfadeAndRestoreQueue();
        ApplyPlaybackCommandResult(_playbackCommands.Pause());
    }

    private void ResumePlayback()
    {
        ApplyPlaybackCommandResult(_playbackCommands.Resume());
    }

    private void PlayPreviousTrack()
    {
        CancelCrossfadeAndRestoreQueue();
        ApplyPlaybackCommandResult(_playbackCommands.Previous());
    }

    private void PlayNextTrack() => AdvanceQueue(false);

    private void AdvanceQueue(bool automatic)
    {
        var queuedCount = _playbackQueue.Entries.Count;
        var currentIndex = _playbackQueue.CurrentIndex;
        var nextIndex = _playbackQueue.NextIndex(automatic);
        var crossfadeSourceIndex = _crossfadeInProgress ? _crossfadeSourceQueueIndex : null;
        var result = _playbackCommands.Advance(automatic, _crossfadeSeconds * 1000,
            SameTrack(_crossfadeFailureSource, _playback.CurrentTrack?.Path), crossfadeSourceIndex);
        LocalAppLog.Shared.Info("playback-queue", $"Advance requested: automatic={automatic}, queuedTracks={queuedCount}, currentIndex={currentIndex}, nextIndex={nextIndex}, repeatMode={_repeatMode}, result={result.Kind}.");
        ApplyPlaybackCommandResult(result);
    }

    private void StartCrossfade(int targetIndex, int durationMilliseconds)
    {
        var sourceIndex = _crossfadeInProgress ? _crossfadeSourceQueueIndex : null;
        ApplyPlaybackCommandResult(_playbackCommands.PrepareCrossfade(targetIndex, durationMilliseconds, sourceIndex));
    }

    private void StartCrossfade(PlaybackCrossfadeRequest request)
    {
        _crossfadeSourceQueueIndex = request.SourceQueueIndex;
        _crossfadeInProgress = true;
        _ = _playback.CrossfadeToAsync(request.Track, request.DurationMilliseconds);
    }

    private void CancelCrossfadeAndRestoreQueue()
    {
        if (!_crossfadeInProgress) return;
        _playback.CancelCrossfade();
        _crossfadeInProgress = false;
        _queueIndex = _crossfadeSourceQueueIndex is { } sourceIndex && sourceIndex >= 0 && sourceIndex < _queue.Count
            ? sourceIndex
            : _playbackQueue.FindIndex(_playback.CurrentTrack?.Path);
        _crossfadeSourceQueueIndex = null;
    }

    private bool ToggleShuffle()
    {
        _playbackQueue.ToggleShuffle(GetCurrentQueuePaths(), _playback.CurrentTrack?.Path);
        EnsureQueueInitialized();
        return _shuffle;
    }

    private void EnsureQueueInitialized()
    {
        if (_queue.Count == 0) _playbackQueue.EnsureQueue(GetCurrentQueuePaths(), _playback.CurrentTrack?.Path);
    }

    private void CycleRepeatMode() => _playbackQueue.CycleRepeatMode();

    private string? ToggleAbRepeat()
    {
        CancelCrossfadeAndRestoreQueue();
        var position = TimeSpan.FromMilliseconds(Math.Max(0, _playback.Position));
        return _playbackQueue.ToggleAbRepeat(position, _playback.CurrentTrack is not null);
    }



    private void SetPlaybackVolume(int value)
    {
        var volume = Math.Clamp(value, 0, 100);
        _playback.Volume = volume;
        if (_windowClosed) return;
        if (volume == _persistedVolume)
        {
            _pendingVolumeSetting = null;
            _volumeSaveDebounce?.Stop();
        }
        else
        {
            _pendingVolumeSetting = volume;
            _volumeSaveDebounce?.Stop();
            _volumeSaveDebounce?.Start();
        }
        PublishPlaybackState();
    }

    private void PersistPendingVolumeSetting()
    {
        if (_pendingVolumeSetting is not int volume || volume == _persistedVolume) return;
        try
        {
            _store.SetSetting("volume", volume.ToString(CultureInfo.InvariantCulture));
            _persistedVolume = volume;
            _pendingVolumeSetting = null;
        }
        catch (Exception ex)
        {
            LocalAppLog.Shared.Error("settings", "Could not save the volume setting.", ex);
            _pendingVolumeSetting = null;
        }
    }

    private int ReadCrossfadeSeconds()
    {
        if (!int.TryParse(_store.GetSetting("crossfade-seconds"), NumberStyles.Integer, CultureInfo.InvariantCulture, out var seconds)) return 0;
        return seconds is 2 or 3 or 5 or 8 or 10 ? seconds : 0;
    }

    private void Clock_Tick(object? sender, object e)
    {
        UpdateWebViewBackgroundState();
        if (_libraryWatcherRescanPending && !_libraryScan.IsRunning) RunPendingLibraryWatcherRescan();
        var position = Math.Max(0, _playback.Position); var duration = Math.Max(0, _playback.Duration);
        if (_playback.IsPlaying && _repeatA is not null && _repeatB is not null && position >= _repeatB.Value.TotalMilliseconds)
        {
            _playback.Seek((long)_repeatA.Value.TotalMilliseconds);
            position = Math.Max(0, _playback.Position);
        }
        if (_systemControls is not null && duration > 0)
        {
            var timeline = new SystemMediaTransportControlsTimelineProperties { StartTime = TimeSpan.Zero, EndTime = TimeSpan.FromMilliseconds(duration), Position = TimeSpan.FromMilliseconds(position) };
            _systemControls.UpdateTimelineProperties(timeline);
        }
        if (_playback.CurrentTrack is { } track && _playbackListening.Observe(true, _playback.IsPlaying, position, duration))
            _store.RecordPlayed(track.Path, DateTime.UtcNow);
        var automaticNext = _playbackQueue.NextIndex(automatic: true);
        if (_playback.IsPlaying && _repeatMode != "Track" && _repeatA is null && !_crossfadeInProgress && !SameTrack(_crossfadeFailureSource, _playback.CurrentTrack?.Path) && automaticNext >= 0 &&
            _crossfadeSeconds > 0 && duration > 0 && duration - position <= _crossfadeSeconds * 1000)
            StartCrossfade(automaticNext, _crossfadeSeconds * 1000);
        if (DateTime.UtcNow - _lastSessionSave > TimeSpan.FromSeconds(5)) SaveSession();
        PublishPlaybackState();
        if (!_playback.IsPlaying && !_libraryScan.IsRunning && DateTime.UtcNow - _lastWalMaintenance > TimeSpan.FromMinutes(1))
        {
            _lastWalMaintenance = DateTime.UtcNow;
            _ = _store.CheckpointWalAsync();
        }
    }

    private DateTime _lastWalMaintenance = DateTime.UtcNow;
    private bool _webViewInBackground;
    private void UpdateWebViewBackgroundState()
    {
        if (_webBridge?.IsReady != true) return;
        var appWindow = AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(WindowNative.GetWindowHandle(this)));
        var background = !appWindow.IsVisible || appWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized };
        if (background == _webViewInBackground) return;
        _webViewInBackground = background;
        _ = _webBridge.SetBackgroundAsync(background);
    }

    private void Playback_TrackEnded(object? sender, EventArgs e)
    {
        if (!DispatcherQueue.TryEnqueue(() =>
        {
            if (!_playback.HasEnded)
            {
                LocalAppLog.Shared.Info("playback", "End notification canceled because playback changed before the UI handled it.");
                return;
            }
            if (_playback.IsVideoMode && _videoWindow?.RepeatCurrent == true && _playback.CurrentTrack is { } videoTrack)
            {
                _playback.Seek(0);
                _playback.PlayLoaded();
                UpdateSystemMediaControls(videoTrack, true);
                PublishPlaybackState();
                return;
            }
            AdvanceQueue(true);
        }))
            LocalAppLog.Shared.Warning("playback", "Could not dispatch the end-of-track notification to the UI thread.");
    }
    private void Playback_CrossfadeCompleted(Track track) => DispatcherQueue.TryEnqueue(() =>
    {
        _crossfadeInProgress = false; _crossfadeFailureSource = null; _crossfadeSourceQueueIndex = null; _playbackListening.ResetForTrack();
        UpdateCurrentTrack(track); UpdateSystemMediaControls(track, true); PublishQueueState(force: true); PublishPlaybackState();
    });

    private static bool SameTrack(string? left, string? right) => left is not null && right is not null && left.Equals(right, StringComparison.OrdinalIgnoreCase);

    private void Playback_CrossfadeFailed(Track track) => DispatcherQueue.TryEnqueue(() =>
    {
        if (!_crossfadeInProgress || _queueIndex < 0 || _queueIndex >= _queue.Count || !SameTrack(_queue[_queueIndex], track.Path)) return;
        _crossfadeInProgress = false;
        _crossfadeFailureSource = _playback.CurrentTrack?.Path;
        _queueIndex = _crossfadeSourceQueueIndex is { } sourceIndex && sourceIndex >= 0 && sourceIndex < _queue.Count
            ? sourceIndex
            : _playbackQueue.FindIndex(_playback.CurrentTrack?.Path);
        _crossfadeSourceQueueIndex = null;
        _ = ShowNoticeAsync($"Could not start {track.Title} during crossfade. Playback will continue; see the local log for details.", InfoBarSeverity.Error);
    });

    private void Playback_Failed(Track track) => DispatcherQueue.TryEnqueue(() =>
    {
        if (!SameTrack(_playback.CurrentTrack?.Path, track.Path)) return;
        _crossfadeInProgress = false;
        if (_systemControls is not null) _systemControls.PlaybackStatus = MediaPlaybackStatus.Stopped;
        if (!File.Exists(track.Path))
        {
            _playback.Stop();
            var fileConfirmedMissing = ReconcileMissingTrack(track.Path);
            PublishPlaybackState();
            var notice = fileConfirmedMissing
                ? $"File not found: {Path.GetFileName(track.Path)}. Its stale library entry was removed; playlist and queue references were kept."
                : $"The file location is unavailable: {Path.GetFileName(track.Path)}. Its library and playlist references were kept; reconnect the drive and rescan when it is available.";
            _ = ShowNoticeAsync(notice, InfoBarSeverity.Warning);
            return;
        }
        _ = ShowNoticeAsync($"Could not play {track.Title}. See Settings → Open log folder for details.", InfoBarSeverity.Error);
    });

    private bool ReconcileMissingTrack(string path)
    {
        var confirmedMissing = _trackAvailability.MarkUnavailable(path);
        _webBridge?.SendEvent("trackAvailabilityChanged", new { id = _libraryQueries.TrackId(path), fileUnavailable = true });
        PublishLibraryChanged();
        PublishQueueState(force: true);
        return confirmedMissing;
    }

    private async Task<Track?> ResolveTrackForPlaybackAsync(string id)
    {
        var track = _libraryQueries.ResolveTrack(id);
        var path = track?.Path ?? _libraryQueries.ResolveTrackPath(id);
        if (path is null) throw new KeyNotFoundException("That track is no longer in the indexed library.");
        if (File.Exists(path))
        {
            if (track is not null) return track;

            var recovered = (await ReadExternalTracksAsync([path])).FirstOrDefault();
            if (recovered is null)
            {
                await ShowNoticeAsync($"Music Player could not read {Path.GetFileName(path)}. Rescan the folder to refresh its library entry.", InfoBarSeverity.Warning);
                return null;
            }

            _trackAvailability.MarkAvailable(path);
            return recovered;
        }

        var fileConfirmedMissing = ReconcileMissingTrack(path);
        var notice = fileConfirmedMissing
            ? $"File not found: {Path.GetFileName(path)}. Its stale library entry was removed; playlist and queue references were kept."
            : $"The file location is unavailable: {Path.GetFileName(path)}. Its library and playlist references were kept; reconnect the drive and rescan when it is available.";
        await ShowNoticeAsync(notice, InfoBarSeverity.Warning);
        return null;
    }

    private void UpdateCurrentTrack(Track track)
    {
        if (_store.GetSetting("accent-mode") == "Artwork") _ = ApplyArtworkAccentAsync(track.ArtworkPath);
        _webBridge?.SendEvent("trackChanged", new { track = _libraryQueries.TrackDto(track, true), playing = _playback.IsPlaying,
            positionSeconds = Math.Max(0, _playback.Position) / 1000d, durationSeconds = Math.Max(0, _playback.Duration) / 1000d,
            volume = _playback.Volume, shuffle = _shuffle, repeat = _repeatMode, repeatA = _repeatA?.TotalSeconds, repeatB = _repeatB?.TotalSeconds,
            isVideo = _playback.IsVideoMode });
    }

    private void ApplyStoredEqualizer()
    {
        var selected = _store.GetSetting("eq-current");
        if (string.IsNullOrWhiteSpace(selected) || selected == "Off") return;
        var saved = _store.GetSetting(selected == "Custom" ? "eq-bands" : "eq-preset:" + selected);
        if (saved is not null)
        {
            _playback.ApplyEqualizer(null, ReadEqualizerBands(saved, PlaybackService.EqualizerBands().Count));
            return;
        }
        if (PlaybackService.EqualizerPresets().Contains(selected, StringComparer.OrdinalIgnoreCase)) _playback.ApplyEqualizer(selected);
    }

    private static float[] ReadEqualizerBands(string? json, int count)
    {
        if (json is null) return new float[count];
        try
        {
            var saved = JsonSerializer.Deserialize<float[]>(json) ?? [];
            return Enumerable.Range(0, count).Select(index => index < saved.Length && float.IsFinite(saved[index])
                ? Math.Clamp(saved[index], -20, 20) : 0).ToArray();
        }
        catch (JsonException ex)
        {
            LocalAppLog.Shared.Warning("equalizer", "Saved equalizer settings were invalid.", ex);
            return new float[count];
        }
    }

    private void RestoreSession()
    {
        var session = _store.LoadSession(); if (session is null) return;
        var current = session.TrackPath is { } savedTrackPath ? _store.GetTrack(savedTrackPath) : null;
        _playbackQueue.Restore(session, current?.Path);
        if (current is not null)
        {
            _playback.LoadPaused(current, session.PositionMilliseconds); _playbackListening.RestorePaused(session.PositionMilliseconds);
            UpdateCurrentTrack(current); UpdateSystemMediaControls(current, false);
        }
    }

    private long _savedQueueRevision = -1;

    private void SaveSession()
    {
        try
        {
            var savedQueueIndex = _crossfadeInProgress ? _crossfadeSourceQueueIndex ?? _queueIndex : _queueIndex;
            var revision = _playbackQueue.ContentRevision;
            var state = _playbackQueue.CreateSession(_playback.CurrentTrack?.Path, Math.Max(0, _playback.Position), savedQueueIndex, includeQueue: false);
            if (_savedQueueRevision != revision || !_store.SaveSessionState(state))
            {
                _store.SaveSession(state with { Queue = _queue.ToArray() });
                _savedQueueRevision = revision;
            }
            PublishQueueStateIfChanged();
            _lastSessionSave = DateTime.UtcNow;
        }
        catch (Exception ex) { LocalAppLog.Shared.Error("session", "Could not save playback state.", ex); }
    }

    private void MainWindow_Closed(object sender, WindowEventArgs args)
    {
        _windowClosed = true;
        LocalAppLog.Shared.Info("app", "Window closed.");
        _volumeSaveDebounce?.Stop();
        StopMountedDriveRefresh();
        PersistPendingVolumeSetting();
        CloseVideoPlaybackWindow(pauseVideo: true);
        SaveSession(); _clock.Stop(); _playback.Dispose(); _libraryScan.Cancel();
        WebUi.Close();
        _libraryFileWatcher.Dispose();
        _tray?.Dispose();
        _taskbarPeekControls?.Dispose();
        foreach (var icon in _windowIconHandles) _ = DestroyIcon(icon);
        _windowIconHandles.Clear();
    }

}
