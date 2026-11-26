namespace MusicPlayer.Core;

/// <summary>
/// Tracks credited listening time for the current playback item without depending on
/// a UI framework, timer, playback engine, or persistence layer.
/// </summary>
public sealed class PlaybackListeningTracker
{
    private bool _countedCurrentPlay;
    private long _heardMilliseconds;
    private long _lastPositionMilliseconds;

    /// <summary>Starts tracking a newly selected track.</summary>
    public void ResetForTrack()
    {
        _countedCurrentPlay = false;
        _heardMilliseconds = 0;
        _lastPositionMilliseconds = 0;
    }

    /// <summary>Rebases position after resuming so paused time is not counted.</summary>
    public void MarkResumed(long positionMilliseconds) =>
        _lastPositionMilliseconds = Math.Max(0, positionMilliseconds);

    /// <summary>Starts a restored session paused at its saved position.</summary>
    public void RestorePaused(long positionMilliseconds)
    {
        _countedCurrentPlay = false;
        _heardMilliseconds = 0;
        _lastPositionMilliseconds = Math.Max(0, positionMilliseconds);
    }

    /// <summary>
    /// Updates credited listening time and returns true exactly once when this
    /// track reaches the existing half-duration play-count threshold.
    /// </summary>
    public bool Observe(bool hasTrack, bool isPlaying, long positionMilliseconds, long durationMilliseconds)
    {
        var position = Math.Max(0, positionMilliseconds);
        var duration = Math.Max(0, durationMilliseconds);

        if (hasTrack)
        {
            if (isPlaying)
            {
                var heardNow = position - _lastPositionMilliseconds;
                if (heardNow is > 0 and <= 3000) _heardMilliseconds += heardNow;
            }

            _lastPositionMilliseconds = position;
        }

        if (_countedCurrentPlay || !hasTrack ||
            !PlayCompletion.HasReachedHalf(TimeSpan.FromMilliseconds(duration), TimeSpan.FromMilliseconds(_heardMilliseconds)))
            return false;

        _countedCurrentPlay = true;
        return true;
    }
}
