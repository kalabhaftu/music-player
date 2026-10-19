using System.Text.Json;
using MusicPlayer.Core;

namespace MusicPlayer.App;

public sealed partial class MainWindow
{
    private async Task<object?> HandleWebUiPlaybackCommandAsync(string name, JsonElement payload)
    {
        switch (name)
        {
            case "playTrack":
            {
                var track = await ResolveTrackForPlaybackAsync(String(payload, "id"));
                if (track is null) return null;
                _libraryQueries.SetContext(payload);
                var paths = _libraryQueries.CurrentTrackPaths();
                _playbackQueue.SelectTrack(paths, track.Path);
                PlayTrack(track, false, _queueIndex);
                PublishQueueStateIfChanged();
                return null;
            }
            case "playView":
            case "shuffleView":
            {
                _libraryQueries.SetContext(payload);
                var paths = _libraryQueries.CurrentTrackPaths();
                if (!_playbackQueue.StartView(paths, name == "shuffleView")) return null;
                var first = _store.GetTrack(_queue[0]);
                if (first is not null) PlayTrack(first, false, 0);
                PublishQueueStateIfChanged();
                return null;
            }
            case "playGroup":
            {
                var value = String(payload, "id");
                var column = _libraryQueries.SetGroupContext(String(payload, "type"), value);
                var paths = _libraryQueries.GroupTrackPaths(column, value);
                if (!_playbackQueue.StartView(paths, shuffleUpcoming: false)) return null;
                if (_store.GetTrack(paths[0]) is { } first) PlayTrack(first, false, 0);
                PublishQueueStateIfChanged();
                return null;
            }
            case "playPause":
                TogglePlayback(StartPlaybackFromCurrentLibraryView);
                return null;
            case "previous": PlayPreviousTrack(); return null;
            case "next": PlayNextTrack(); return null;
            case "toggleShuffle":
            {
                var enabled = ToggleShuffle();
                SaveSession();
                _ = ShowNoticeAsync(enabled ? "Shuffle is on." : "Shuffle is off.");
                return null;
            }
            case "cycleRepeat": CycleRepeatMode(); SaveSession(); return null;
            case "toggleAbRepeat":
            {
                if (ToggleAbRepeat() is { } notice)
                {
                    _ = ShowNoticeAsync(notice);
                    SaveSession();
                }
                return null;
            }
            case "seek":
            {
                CancelCrossfadeAndRestoreQueue();
                var resumeAfterEnd = _playback.HasEnded;
                _playback.Seek((long)(Double(payload, "seconds") * 1000));
                if (resumeAfterEnd && _playback.CurrentTrack is { } resumed)
                {
                    _playbackListening.MarkResumed(_playback.Position);
                    UpdateSystemMediaControls(resumed, true);
                }
                PublishQueueStateIfChanged();
                PublishPlaybackState();
                return null;
            }
            case "setVolume": SetPlaybackVolume(Int(payload, "volume", 75)); return null;
            case "toggleFavorite":
            {
                var track = RequireTrack(payload, "id");
                _store.SetFavorite(track.Path, !track.Favorite);
                var updated = _store.GetTrack(track.Path) ?? track with { Favorite = !track.Favorite };
                if (SameTrack(_playback.CurrentTrack?.Path, updated.Path))
                {
                    _playback.UpdateTrackMetadata(updated);
                }
                _webBridge?.SendEvent("favoriteChanged", new { id = _libraryQueries.TrackId(updated.Path), favorite = updated.Favorite });
                PublishLibraryChanged("favorite"); return null;
            }
            case "setRating":
            {
                var track = RequireTrack(payload, "id");
                _store.SetRating(track.Path, Int(payload, "rating", 0));
                if (SameTrack(_playback.CurrentTrack?.Path, track.Path) && _store.GetTrack(track.Path) is { } updated)
                {
                    _playback.UpdateTrackMetadata(updated);
                }
                PublishLibraryChanged(); return null;
            }
            case "playNext":
            {
                var track = RequireTrack(payload, "id");
                CancelCrossfadeAndRestoreQueue();
                EnsureQueueInitialized();
                _playbackQueue.InsertNext(track.Path, RetainedCurrentPath);
                SaveSession(); PublishQueueStateIfChanged();
                return null;
            }
            case "addToQueue":
            {
                var track = RequireTrack(payload, "id");
                CancelCrossfadeAndRestoreQueue();
                _playbackQueue.InsertNextRange([track.Path], RetainedCurrentPath);
                SaveSession(); PublishQueueStateIfChanged();
                return null;
            }
            case "moveQueue":
            {
                var targetIndex = Int(payload, "toIndex", -1);
                MoveQueueEntry(Int(payload, "index", -1), Int(payload, "direction"), targetIndex >= 0 ? (int?)targetIndex : null);
                return null;
            }
            case "removeQueue":
                RemoveQueueEntry(Int(payload, "index", -1)); return null;
            case "clearQueue": ClearUpcomingQueue(); return null;
            case "playQueueEntry": await PlayQueueEntryAsync(Int(payload, "index", -1)); return null;
            default: throw new InvalidOperationException("This Music Player command is not available in the playback handler.");
        }
    }
}
