using MusicPlayer.Core;

namespace MusicPlayer.App;

internal enum PlaybackCommandKind
{
    NoChange,
    TrackStarted,
    Paused,
    Resumed,
    PositionReset,
    CrossfadeRequested,
    StopPlayback,
    TrackUnavailable,
    AdvanceFallback
}

internal sealed record PlaybackCrossfadeRequest(Track Track, int QueueIndex, int SourceQueueIndex, int DurationMilliseconds);

/// <summary>
/// Coordinates transport commands across authoritative queue state and the playback engine.
/// It returns outcomes for the native shell to persist and publish; it has no UI dependency.
/// </summary>
internal sealed class PlaybackCommandCoordinator(
    PlaybackQueueCoordinator queue,
    PlaybackService playback,
    Func<string, Track?> resolveTrack,
    Func<IReadOnlyList<string>> getCurrentViewPaths)
{
    public PlaybackCommandResult StartTrack(Track track, bool resetQueue, int? queueIndex = null)
    {
        ArgumentNullException.ThrowIfNull(track);
        if (!File.Exists(track.Path))
            return new(PlaybackCommandKind.TrackUnavailable, track,
                Notice: $"File not found at its saved location: {track.Path}");

        if (resetQueue)
        {
            queue.Replace(getCurrentViewPaths());
            var selectedIndex = queueIndex is { } requested && requested >= 0 && requested < queue.Entries.Count &&
                                SameTrack(queue.Entries[requested], track.Path)
                ? requested
                : queue.FindIndex(track.Path);
            queue.SetCurrentIndex(selectedIndex);
            if (queue.Shuffle) queue.ShuffleUpcoming(Math.Clamp(queue.CurrentIndex + 1, 0, queue.Entries.Count));
        }
        else if (queueIndex is { } requestedIndex && requestedIndex >= 0 && requestedIndex < queue.Entries.Count)
        {
            queue.SetCurrentIndex(requestedIndex);
        }
        else if (queue.FindIndex(track.Path) is var matchingIndex && matchingIndex >= 0)
        {
            queue.SetCurrentIndex(matchingIndex);
        }

        playback.Play(track);
        queue.ClearAbRepeat();
        return new(PlaybackCommandKind.TrackStarted, track, QueueChanged: true, PersistSession: true);
    }

    public PlaybackCommandResult Toggle()
    {
        if (playback.CurrentTrack is not { } current) return PlaybackCommandResult.NoChange;
        if (playback.IsPlaying) return Pause();
        if (playback.HasEnded || (playback.Duration > 0 && playback.Position >= playback.Duration - 250))
            return StartTrack(current, resetQueue: false, queue.CurrentIndex >= 0 ? queue.CurrentIndex : null);
        return Resume();
    }

    public PlaybackCommandResult Pause()
    {
        playback.Pause();
        return new(PlaybackCommandKind.Paused, playback.CurrentTrack);
    }

    public PlaybackCommandResult Resume()
    {
        if (playback.CurrentTrack is { } current && !File.Exists(current.Path))
            return new(PlaybackCommandKind.TrackUnavailable, current,
                Notice: $"File not found at its saved location: {current.Path}");
        playback.PlayLoaded();
        return new(PlaybackCommandKind.Resumed, playback.CurrentTrack);
    }

    public PlaybackCommandResult Previous()
    {
        if (playback.CurrentTrack is null) return PlaybackCommandResult.NoChange;
        if (playback.Position > 3000)
        {
            playback.Seek(0);
            return new(PlaybackCommandKind.PositionReset, playback.CurrentTrack);
        }

        for (var index = queue.CurrentIndex - 1; index >= 0; index--)
        {
            if (ResolveQueueTrack(index) is not { } previous) continue;
            return StartTrack(previous, resetQueue: false, index);
        }

        // Previous at the first playable queue entry restarts it instead of doing nothing.
        playback.Seek(0);
        return new(PlaybackCommandKind.PositionReset, playback.CurrentTrack);
    }

    public PlaybackCommandResult Advance(bool automatic, int crossfadeMilliseconds, bool crossfadeBlocked,
        int? existingCrossfadeSourceIndex = null)
    {
        var wasEmpty = queue.Entries.Count == 0;
        if (wasEmpty) queue.EnsureQueue(getCurrentViewPaths(), playback.CurrentTrack?.Path);
        if (wasEmpty && queue.Shuffle)
            queue.ShuffleUpcoming(Math.Clamp(queue.CurrentIndex + 1, 0, queue.Entries.Count));
        if (queue.Entries.Count == 0) return PlaybackCommandResult.NoChange;

        var changedWhileSkipping = false;
        for (var attempt = 0; attempt <= queue.Entries.Count; attempt++)
        {
            var next = queue.NextIndex(automatic && !changedWhileSkipping);
            if (next < 0) break;
            if (ResolveQueueTrack(next) is not { } track)
            {
                if (next == queue.CurrentIndex) { changedWhileSkipping = true; continue; }
                queue.SetCurrentIndex(next);
                changedWhileSkipping = true;
                continue;
            }

            if (automatic && playback.IsPlaying && !crossfadeBlocked && crossfadeMilliseconds > 0)
                return PrepareCrossfade(track, next, crossfadeMilliseconds, existingCrossfadeSourceIndex);

            return StartTrack(track, resetQueue: false, next);
        }

        return new(PlaybackCommandKind.StopPlayback, QueueChanged: changedWhileSkipping);
    }

    public PlaybackCommandResult PrepareCrossfade(int targetIndex, int durationMilliseconds,
        int? existingCrossfadeSourceIndex = null)
    {
        if (ResolveQueueTrack(targetIndex) is not { } target)
            return new(PlaybackCommandKind.AdvanceFallback);
        return PrepareCrossfade(target, targetIndex, durationMilliseconds, existingCrossfadeSourceIndex);
    }

    public PlaybackCommandResult PlayQueueEntry(int index)
    {
        if (ResolveQueueTrack(index) is not { } track)
            return new(PlaybackCommandKind.TrackUnavailable, Notice: "That queued file is no longer available.");
        return StartTrack(track, resetQueue: false, index);
    }

    private PlaybackCommandResult PrepareCrossfade(Track target, int targetIndex, int durationMilliseconds,
        int? existingCrossfadeSourceIndex)
    {
        var sourceIndex = existingCrossfadeSourceIndex ?? queue.CurrentIndex;
        queue.SetCurrentIndex(targetIndex);
        var request = new PlaybackCrossfadeRequest(target, targetIndex, sourceIndex, durationMilliseconds);
        return new(PlaybackCommandKind.CrossfadeRequested, target, Crossfade: request);
    }

    private Track? ResolveQueueTrack(int index) =>
        index >= 0 && index < queue.Entries.Count && File.Exists(queue.Entries[index]) ? resolveTrack(queue.Entries[index]) : null;

    private static bool SameTrack(string? left, string? right) =>
        left is not null && right is not null && left.Equals(right, StringComparison.OrdinalIgnoreCase);
}

internal sealed record PlaybackCommandResult(
    PlaybackCommandKind Kind,
    Track? Track = null,
    bool QueueChanged = false,
    bool PersistSession = false,
    string? Notice = null,
    PlaybackCrossfadeRequest? Crossfade = null)
{
    public static PlaybackCommandResult NoChange { get; } = new(PlaybackCommandKind.NoChange);
}
