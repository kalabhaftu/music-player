namespace MusicPlayer.Core;

public static class QueueNavigation
{
    public static string CycleRepeatMode(string repeatMode) => repeatMode switch
    {
        "Off" => "Queue",
        "Queue" => "Track",
        _ => "Off"
    };

    public static IReadOnlyList<string> BuildExternalOpenQueue(
        IEnumerable<string> openedPaths,
        string? currentTrackPath,
        IReadOnlyList<string> existingQueue,
        int currentIndex)
    {
        ArgumentNullException.ThrowIfNull(openedPaths);
        ArgumentNullException.ThrowIfNull(existingQueue);
        // The application targets Windows paths, including in the cross-platform Core test job.
        var comparer = StringComparer.OrdinalIgnoreCase;
        var result = openedPaths.Where(path => !string.IsNullOrWhiteSpace(path)).Distinct(comparer).ToList();
        var newlyOpened = result.ToHashSet(comparer);

        if (!string.IsNullOrWhiteSpace(currentTrackPath) && !result.Contains(currentTrackPath, comparer))
            result.Add(currentTrackPath);

        var firstUpcoming = Math.Clamp(currentIndex + 1, 0, existingQueue.Count);
        for (var index = firstUpcoming; index < existingQueue.Count; index++)
            if (!newlyOpened.Contains(existingQueue[index])) result.Add(existingQueue[index]);

        return result;
    }

    public static int NextIndex(int count, int currentIndex, bool automatic, string repeatMode)
    {
        if (count <= 0) return -1;
        if (automatic && string.Equals(repeatMode, "Track", StringComparison.OrdinalIgnoreCase)) return currentIndex >= 0 && currentIndex < count ? currentIndex : 0;
        var next = currentIndex < 0 ? 0 : currentIndex + 1;
        if (next < count) return next;
        return string.Equals(repeatMode, "Queue", StringComparison.OrdinalIgnoreCase) ? 0 : -1;
    }

    public static void ShuffleUpcoming<T>(IList<T> queue, int startIndex)
    {
        ArgumentNullException.ThrowIfNull(queue);
        if (startIndex < 0 || startIndex > queue.Count) throw new ArgumentOutOfRangeException(nameof(startIndex));
        for (var index = queue.Count - 1; index > startIndex; index--)
        {
            var next = Random.Shared.Next(startIndex, index + 1);
            (queue[index], queue[next]) = (queue[next], queue[index]);
        }
    }
}
