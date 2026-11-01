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
    private object? _lastWebPlaybackState;
    private int WebQueueIndex => _crossfadeInProgress ? _crossfadeSourceQueueIndex ?? _queueIndex : _queueIndex;
    private void PublishPlaybackState()
    {
        if (_webViewInBackground) return;
        var current = _playback.CurrentTrack;
        var data = new
        {
            playing = _playback.IsPlaying, positionSeconds = Math.Max(0, _playback.Position) / 1000d,
            durationSeconds = Math.Max(0, _playback.Duration) / 1000d, volume = _playback.Volume,
            shuffle = _shuffle, repeat = _repeatMode, repeatA = _repeatA?.TotalSeconds, repeatB = _repeatB?.TotalSeconds,
            isVideo = _playback.IsVideoMode,
            queueIndex = WebQueueIndex, trackId = current is null ? null : _libraryQueries.TrackId(current.Path)
        };
        if (data.Equals(_lastWebPlaybackState)) return;
        _lastWebPlaybackState = data;
        _webBridge?.SendEvent("playbackStateChanged", data);
    }

    private void PublishQueueState(bool force = false)
    {
        _taskbarPeekControls?.Update(_playback.CurrentTrack is not null || _queue.Count > 0, _playback.IsPlaying);
        var revision = _playbackQueue.ContentRevision;
        var index = WebQueueIndex;
        if (!force && revision == _lastWebQueueRevision && index == _lastWebQueueIndex) return;
        var itemsChanged = force || revision != _lastWebQueueRevision;
        _lastWebQueueRevision = revision;
        _lastWebQueueIndex = index;
        _webBridge?.SendEvent("queueChanged", new { entries = QueueDtos(Math.Max(0, index), 30), queueOffset = Math.Max(0, index), queueIndex = index, totalCount = _queue.Count, itemsChanged });
    }

    private object ScanDto()
    {
        var (active, paused, cancelling) = _libraryScan.GetStatus();
        var outcomeVisible = !active && _scanOutcomeUtc is { } finishedAt && DateTimeOffset.UtcNow - finishedAt < TimeSpan.FromSeconds(8);
        return new
        {
            active,
            paused,
            cancelling,
            filesFound = _scanFilesFound,
            directoriesVisited = _scanDirectoriesVisited,
            currentPath = active ? _scanCurrentPath : null,
            outcome = outcomeVisible ? _scanOutcomeKind : null,
            message = outcomeVisible ? _scanOutcomeMessage : null
        };
    }

    private void PublishScanState() => _webBridge?.SendEvent("scanChanged", ScanDto());

    private long _libraryRevision;
    private void PublishLibraryChanged(string? reason = null) { _libraryQueries.InvalidateFolders(); _webBridge?.SendEvent("libraryChanged", new { revision = ++_libraryRevision, reason }); }

    private void PublishSettingsChanged() => _webBridge?.SendEvent("settingsChanged", WebSettings());
}
