using MusicPlayer.Core;

namespace MusicPlayer.App;

/// <summary>
/// Owns the authoritative in-memory playback queue and its navigation modes.
/// It deliberately has no WinUI dependency; the window persists snapshots and
/// translates state changes into player, SMTC, and presentation updates.
/// </summary>
internal sealed class PlaybackQueueCoordinator
{
    private readonly List<string> _entries = [];
    private bool _currentEntryRemoved;

    public IReadOnlyList<string> Entries => _entries;
    public long ContentRevision { get; private set; }
    public int CurrentIndex { get; private set; } = -1;
    public bool Shuffle { get; private set; }
    public string RepeatMode { get; private set; } = "Off";
    public TimeSpan? RepeatA { get; private set; }
    public TimeSpan? RepeatB { get; private set; }

    public void SetShuffle(bool enabled) => Shuffle = enabled;
    public void SetRepeatMode(string mode) => RepeatMode = mode;
    public void SetAbRepeatPoints(TimeSpan? pointA, TimeSpan? pointB)
    {
        RepeatA = pointA;
        RepeatB = pointB;
    }

    public void Replace(IEnumerable<string> paths, int currentIndex = -1)
    {
        ArgumentNullException.ThrowIfNull(paths);
        _entries.Clear();
        _entries.AddRange(paths);
        ContentRevision++;
        SetCurrentIndex(currentIndex);
    }

    public void SelectTrack(IEnumerable<string> contextPaths, string trackPath)
    {
        Replace(contextPaths);
        CurrentIndex = FindIndex(trackPath);
        if (CurrentIndex < 0)
        {
            _entries.Insert(0, trackPath);
            CurrentIndex = 0;
        }
        if (Shuffle) ShuffleUpcoming(Math.Clamp(CurrentIndex + 1, 0, _entries.Count));
    }

    public bool StartView(IEnumerable<string> paths, bool shuffleUpcoming)
    {
        Replace(paths);
        if (_entries.Count == 0) return false;
        CurrentIndex = 0;
        if (shuffleUpcoming) ShuffleUpcoming(0);
        return true;
    }

    public void Restore(PlaybackSession session, string? currentTrackPath)
    {
        ArgumentNullException.ThrowIfNull(session);
        Shuffle = session.Shuffle;
        RepeatMode = session.RepeatMode;
        RepeatA = null;
        RepeatB = null;
        if (session.RepeatAMilliseconds is long repeatA && repeatA >= 0 &&
            session.RepeatBMilliseconds is long repeatB && repeatB > repeatA)
        {
            RepeatA = TimeSpan.FromMilliseconds(repeatA);
            RepeatB = TimeSpan.FromMilliseconds(repeatB);
        }

        Replace(session.Queue);
        if (string.IsNullOrWhiteSpace(currentTrackPath)) return;
        if (_entries.Count == 0) _entries.Add(currentTrackPath);

        var savedIndex = session.QueueIndex;
        var occurrence = savedIndex >= 0 && savedIndex < session.Queue.Count &&
                         SamePath(session.Queue[savedIndex], currentTrackPath)
            ? session.Queue.Take(savedIndex + 1).Count(path => SamePath(path, currentTrackPath)) - 1
            : 0;
        CurrentIndex = FindPathOccurrence(currentTrackPath, Math.Max(0, occurrence));
        if (CurrentIndex < 0) CurrentIndex = FindIndex(currentTrackPath);
        if (CurrentIndex < 0)
        {
            _entries.Insert(0, currentTrackPath);
            CurrentIndex = 0;
        }
    }

    public int FindIndex(string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return -1;
        return _entries.FindIndex(entry => SamePath(entry, path));
    }

    public void SetCurrentIndex(int index)
    {
        CurrentIndex = index >= 0 && index < _entries.Count ? index : -1;
        _currentEntryRemoved = false;
    }

    public void ShuffleUpcoming(int startIndex)
    {
        QueueNavigation.ShuffleUpcoming(_entries, startIndex);
        if (_entries.Count - Math.Clamp(startIndex, 0, _entries.Count) > 1) ContentRevision++;
    }

    public bool MoveEntry(int index, int direction)
    {
        return MoveEntryTo(index, index + direction);
    }

    public bool MoveEntryTo(int index, int targetIndex)
    {
        var firstUpcomingIndex = Math.Max(0, CurrentIndex + 1);
        if (index < firstUpcomingIndex || index >= _entries.Count ||
            targetIndex < firstUpcomingIndex || targetIndex >= _entries.Count || targetIndex == index) return false;

        var entry = _entries[index];
        _entries.RemoveAt(index);
        _entries.Insert(targetIndex, entry);
        ContentRevision++;
        return true;
    }

    public bool RemovePaths(IReadOnlyList<string> paths)
    {
        var removed = paths.ToHashSet(StringComparer.OrdinalIgnoreCase);
        var oldIndex = CurrentIndex;
        _currentEntryRemoved |= oldIndex >= 0 && oldIndex < _entries.Count && removed.Contains(_entries[oldIndex]);
        var retainedBeforeCurrent = _entries.Take(Math.Clamp(oldIndex + 1, 0, _entries.Count)).Count(path => !removed.Contains(path));
        if (_entries.RemoveAll(path => removed.Contains(path)) == 0) return false;
        CurrentIndex = retainedBeforeCurrent - 1;
        ContentRevision++;
        return true;
    }

    public bool RemoveEntry(int index)
    {
        if (index < 0 || index >= _entries.Count || index == CurrentIndex) return false;
        _entries.RemoveAt(index);
        if (index < CurrentIndex) CurrentIndex--;
        ContentRevision++;
        return true;
    }

    public void ClearUpcoming(string? currentTrackPath)
    {
        _entries.Clear();
        ContentRevision++;
        if (string.IsNullOrWhiteSpace(currentTrackPath))
        {
            CurrentIndex = -1;
            return;
        }

        _entries.Add(currentTrackPath);
        CurrentIndex = 0;
    }

    public void EnsureCurrentTrack(string? currentTrackPath)
    {
        if (string.IsNullOrWhiteSpace(currentTrackPath)) return;
        if (_entries.Count == 0)
        {
            _entries.Add(currentTrackPath);
            ContentRevision++;
            CurrentIndex = 0;
            return;
        }

        if (CurrentIndex >= 0 && CurrentIndex < _entries.Count && SamePath(_entries[CurrentIndex], currentTrackPath)) return;
        var existingIndex = FindIndex(currentTrackPath);
        if (existingIndex >= 0)
        {
            CurrentIndex = existingIndex;
            return;
        }

        var insertAt = Math.Clamp(CurrentIndex + 1, 0, _entries.Count);
        _entries.Insert(insertAt, currentTrackPath);
        ContentRevision++;
        CurrentIndex = insertAt;
    }

    public int InsertNext(string path, string? currentTrackPath)
    {
        if (_entries.Count == 0 && !string.IsNullOrWhiteSpace(currentTrackPath))
        {
            _entries.Add(currentTrackPath);
            CurrentIndex = 0;
        }

        var insertAt = Math.Clamp(CurrentIndex + 1, 0, _entries.Count);
        _entries.Insert(insertAt, path);
        ContentRevision++;
        if (CurrentIndex >= 0 && insertAt <= CurrentIndex) CurrentIndex++;
        if (Shuffle) ShuffleUpcoming(insertAt + 1);
        return insertAt;
    }

    public void Append(IEnumerable<string> paths, string? currentTrackPath)
    {
        EnsureCurrentTrack(currentTrackPath);
        var previousCount = _entries.Count;
        _entries.AddRange(paths);
        if (_entries.Count != previousCount) ContentRevision++;
    }

    public void InsertNextRange(IEnumerable<string> paths, string? currentTrackPath)
    {
        var selected = paths.ToArray();
        if (selected.Length == 0) return;
        EnsureCurrentTrack(currentTrackPath);
        _entries.InsertRange(Math.Clamp(CurrentIndex + 1, 0, _entries.Count), selected);
        ContentRevision++;
    }

    public void ToggleShuffle(IEnumerable<string> currentViewPaths, string? currentTrackPath)
    {
        Shuffle = !Shuffle;
        if (_entries.Count == 0)
        {
            Replace(currentViewPaths);
            if (!string.IsNullOrWhiteSpace(currentTrackPath))
            {
                CurrentIndex = FindIndex(currentTrackPath);
                if (CurrentIndex < 0)
                {
                    _entries.Insert(0, currentTrackPath);
                    CurrentIndex = 0;
                }
            }
        }
        if (Shuffle) ShuffleUpcoming(Math.Clamp(CurrentIndex + 1, 0, _entries.Count));
    }

    public void EnsureQueue(IEnumerable<string> currentViewPaths, string? currentTrackPath)
    {
        if (_entries.Count > 0) return;
        Replace(currentViewPaths);
        if (string.IsNullOrWhiteSpace(currentTrackPath)) return;
        CurrentIndex = FindIndex(currentTrackPath);
        if (CurrentIndex < 0)
        {
            _entries.Insert(0, currentTrackPath);
            CurrentIndex = 0;
        }
    }

    public int NextIndex(bool automatic) =>
        QueueNavigation.NextIndex(_entries.Count, CurrentIndex, automatic && !_currentEntryRemoved, RepeatMode);

    public void CycleRepeatMode() => RepeatMode = QueueNavigation.CycleRepeatMode(RepeatMode);

    public string? ToggleAbRepeat(TimeSpan position, bool hasCurrentTrack)
    {
        if (!hasCurrentTrack) return null;
        if (RepeatA is null)
        {
            RepeatA = position;
            RepeatB = null;
            return "A–B repeat: mark B at the end of the passage.";
        }
        if (RepeatB is null && position > RepeatA)
        {
            RepeatB = position;
            return "A–B repeat is set. Press again to clear.";
        }

        RepeatA = null;
        RepeatB = null;
        return "A–B repeat cleared.";
    }

    public void ClearAbRepeat()
    {
        RepeatA = null;
        RepeatB = null;
    }

    public PlaybackSession CreateSession(string? currentTrackPath, long positionMilliseconds, int? currentIndexOverride = null,
        bool includeQueue = true) =>
        new(currentTrackPath, positionMilliseconds, includeQueue ? _entries.ToArray() : Array.Empty<string>(), Shuffle, RepeatMode,
            RepeatA is { } repeatA ? (long)repeatA.TotalMilliseconds : null,
            RepeatB is { } repeatB ? (long)repeatB.TotalMilliseconds : null,
            currentIndexOverride ?? CurrentIndex);

    private int FindPathOccurrence(string path, int occurrence)
    {
        var found = 0;
        for (var index = 0; index < _entries.Count; index++)
        {
            if (!SamePath(_entries[index], path)) continue;
            if (found == occurrence) return index;
            found++;
        }
        return -1;
    }

    private static bool SamePath(string? left, string? right) =>
        left is not null && right is not null && left.Equals(right, StringComparison.OrdinalIgnoreCase);
}
