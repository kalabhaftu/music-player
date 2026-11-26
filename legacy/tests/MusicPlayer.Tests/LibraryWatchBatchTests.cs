using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Tests;

public sealed class LibraryWatchBatchTests
{
    [Fact]
    public void Burst_coalesces_duplicates_and_nested_directories_without_scanning_other_roots()
    {
        var batch = new LibraryWatchBatch();
        var folder = Path.GetFullPath("music");
        batch.AddDirectory(Path.Combine(folder, "album"));
        batch.AddDirectory(folder);
        batch.AddDirectory(folder);
        var result = batch.Drain(DateTimeOffset.UtcNow);
        Assert.Equal(new[] { folder }, result.Directories);
        Assert.False(result.FullScan);
        Assert.Empty(batch.Drain(DateTimeOffset.UtcNow).Directories);
    }

    [Fact]
    public void Repeated_overflow_defers_one_recovery_for_five_minutes()
    {
        var batch = new LibraryWatchBatch();
        var now = DateTimeOffset.UtcNow;
        batch.RequestRecovery();
        Assert.True(batch.Drain(now).FullScan);
        batch.RequestRecovery();
        batch.RequestRecovery();
        var delayed = batch.Drain(now.AddMinutes(1));
        Assert.False(delayed.FullScan);
        Assert.Equal(TimeSpan.FromMinutes(4), delayed.RetryAfter);
        Assert.True(batch.Drain(now.AddMinutes(5)).FullScan);
        Assert.False(batch.Drain(now.AddMinutes(5)).FullScan);
    }

    [Fact]
    public void Excessive_unique_changes_collapse_to_bounded_recovery()
    {
        var batch = new LibraryWatchBatch();
        for (var index = 0; index < 2000; index++) batch.AddDirectory($"album-{index}");
        var result = batch.Drain(DateTimeOffset.UtcNow);
        Assert.True(result.FullScan);
        Assert.Empty(result.Directories);
    }
}
