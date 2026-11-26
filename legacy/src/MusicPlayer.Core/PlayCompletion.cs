namespace MusicPlayer.Core;

public static class PlayCompletion
{
    public static bool HasReachedHalf(TimeSpan duration, TimeSpan position) => duration > TimeSpan.Zero && position >= TimeSpan.FromTicks(duration.Ticks / 2);
}
