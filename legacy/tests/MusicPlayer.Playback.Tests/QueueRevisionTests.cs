using MusicPlayer.App;
using Xunit;

namespace MusicPlayer.Playback.Tests;

public sealed class QueueRevisionTests
{
    [Fact]
    public void Position_and_modes_do_not_invalidate_contents_but_edits_do()
    {
        var queue = new PlaybackQueueCoordinator();
        queue.Replace(["a", "b", "b", "c"], 0);
        var revision = queue.ContentRevision;
        queue.SetCurrentIndex(1);
        queue.CycleRepeatMode();
        queue.SetShuffle(true);
        Assert.Equal(revision, queue.ContentRevision);
        Assert.False(queue.MoveEntryTo(1, 2));
        Assert.Equal(revision, queue.ContentRevision);
        Assert.True(queue.MoveEntryTo(2, 3));
        Assert.True(queue.ContentRevision > revision);
        revision = queue.ContentRevision;
        queue.RemoveEntry(3);
        Assert.True(queue.ContentRevision > revision);
        revision = queue.ContentRevision;
        queue.Append(["b"], "b");
        Assert.True(queue.ContentRevision > revision);
        Assert.Equal(new[] { "a", "b", "c", "b" }, queue.Entries);
        Assert.Empty(queue.CreateSession("b", 12, includeQueue: false).Queue);
        Assert.Equal(queue.Entries, queue.CreateSession("b", 12).Queue);
    }
}
