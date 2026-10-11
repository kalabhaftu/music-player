using System.Diagnostics;
using System.Text;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Playback.Tests;

// These tests exercise the shipped native LibVLC bindings, including event sender
// identity and decoder readiness. Silent WAV files need no external media or device.
public sealed class PlaybackIntegrationTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-playback-" + Guid.NewGuid().ToString("N"));

    public PlaybackIntegrationTests() => Directory.CreateDirectory(_root);

    [Fact]
    public async Task Enqueue_preserves_playing_position_paused_state_duplicates_and_idle_state()
    {
        var first = Wave("enqueue-playing", 12); var second = Wave("enqueue-next", 4);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        var queue = new PlaybackQueueCoordinator();
        queue.Append([first.Path, first.Path], null);
        Assert.Null(playback.CurrentTrack); Assert.False(playback.IsPlaying); Assert.Equal(-1, queue.CurrentIndex);
        queue.SetCurrentIndex(1); playback.Play(first);
        await WaitFor(() => playback.IsPlaying && playback.Position > 900);
        var before = playback.Position;
        queue.Append([second.Path, first.Path], first.Path);
        await Task.Delay(200);
        Assert.True(playback.IsPlaying); Assert.Equal(first.Path, playback.CurrentTrack!.Path);
        Assert.InRange(playback.Position, before, before + 1500);
        Assert.Equal(1, queue.CurrentIndex);
        Assert.Equal(new[] { first.Path, first.Path, second.Path, first.Path }, queue.Entries);
        playback.Pause(); await WaitFor(() => !playback.IsPlaying);
        before = playback.Position; queue.Append([second.Path], first.Path); await Task.Delay(200);
        Assert.False(playback.IsPlaying); Assert.InRange(playback.Position, before - 100, before + 100);
        queue.RemovePaths([first.Path]);
        Assert.Equal(-1, queue.CurrentIndex);
        Assert.Equal(new[] { second.Path, second.Path }, queue.Entries);
    }

    [Fact]
    public async Task Advancement_and_crossfade_skip_missing_entries_without_losing_duplicate_occurrences()
    {
        var first = Wave("skip-first", 8); var next = Wave("skip-next", 8);
        var missing = Path.Combine(_root, "deleted.wav");
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        var queue = new PlaybackQueueCoordinator();
        queue.Replace([first.Path, missing, next.Path, next.Path], 0);
        var commands = new PlaybackCommandCoordinator(queue, playback,
            path => path == first.Path ? first : path == next.Path ? next : first with { Path = path }, () => queue.Entries);
        commands.StartTrack(first, false, 0); await WaitFor(() => playback.IsPlaying);
        Assert.Equal(PlaybackCommandKind.AdvanceFallback, commands.PrepareCrossfade(1, 500).Kind);
        var result = commands.Advance(false, 500, false);
        Assert.Equal(PlaybackCommandKind.CrossfadeRequested, result.Kind);
        Assert.Equal(2, queue.CurrentIndex); Assert.Equal(next.Path, result.Track!.Path);
    }

    [Fact]
    public async Task Natural_end_advances_the_queue_and_reports_each_track_once()
    {
        var first = Wave("first", 1);
        var second = Wave("second", 1);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        playback.Volume = 0;
        var queue = new PlaybackQueueCoordinator();
        queue.Replace([first.Path, second.Path], 0);
        var commands = new PlaybackCommandCoordinator(queue, playback,
            path => path == first.Path ? first : path == second.Path ? second : null, () => queue.Entries);
        var notifications = System.Threading.Channels.Channel.CreateUnbounded<bool>();
        playback.TrackEnded += (_, _) => notifications.Writer.TryWrite(true);
        commands.StartTrack(first, resetQueue: false, 0);

        await notifications.Reader.ReadAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
        Assert.True(playback.HasEnded);
        Assert.Equal(PlaybackCommandKind.TrackStarted, commands.Advance(true, 0, false).Kind);
        Assert.Equal(1, queue.CurrentIndex);
        Assert.Equal(second.Path, playback.CurrentTrack?.Path);
        Assert.False(playback.HasEnded);

        await notifications.Reader.ReadAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(PlaybackCommandKind.StopPlayback, commands.Advance(true, 0, false).Kind);
        Assert.False(notifications.Reader.TryRead(out _));
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task Seeking_back_after_natural_end_resumes_at_the_requested_position(bool queueStopped)
    {
        var track = Wave("seek-ended", 3);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        playback.Volume = 0;
        playback.Play(track);
        await WaitFor(() => playback.IsPlaying);
        await WaitFor(() => playback.HasEnded);
        if (queueStopped) playback.Stop();
        playback.Seek(1000);
        await WaitFor(() => playback.IsPlaying && playback.Position > 1100 && playback.Position < 2200);
        Assert.False(playback.HasEnded);
    }

    [Fact]
    public async Task Seeking_a_restored_paused_track_keeps_it_paused_and_restores_the_new_position()
    {
        var track = Wave("restored", 5);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        playback.Volume = 0;
        playback.LoadPaused(track, 3000);
        playback.Seek(1000);
        Assert.False(playback.IsPlaying);
        Assert.Equal(1000, playback.Position);
        playback.PlayLoaded();
        await WaitFor(() => playback.IsPlaying && playback.Position > 1100 && playback.Position < 2300);
    }

    [Fact]
    public async Task Seeking_while_paused_does_not_resume_until_play_is_requested()
    {
        var track = Wave("paused", 5);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        playback.Play(track);
        await WaitFor(() => playback.IsPlaying && playback.Position > 100);
        playback.Pause();
        await WaitFor(() => !playback.IsPlaying);
        playback.Seek(2000);
        await Task.Delay(150);
        Assert.False(playback.IsPlaying);
        playback.PlayLoaded();
        await WaitFor(() => playback.IsPlaying && playback.Position > 2100 && playback.Position < 3500);
    }

    [Fact]
    public async Task Crossfade_promoted_player_advances_to_the_following_queue_entry()
    {
        var first = Wave("fade-first", 3);
        var second = Wave("fade-second", 2);
        var third = Wave("fade-third", 3);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        var queue = new PlaybackQueueCoordinator();
        queue.Replace([first.Path, second.Path, third.Path], 0);
        var tracks = new[] { first, second, third }.ToDictionary(track => track.Path);
        var commands = new PlaybackCommandCoordinator(queue, playback, path => tracks.GetValueOrDefault(path), () => queue.Entries);
        var ended = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        playback.TrackEnded += (_, _) => ended.TrySetResult();
        commands.StartTrack(first, false, 0);
        await WaitFor(() => playback.IsPlaying);
        var fade = commands.PrepareCrossfade(1, 120).Crossfade!;
        await playback.CrossfadeToAsync(fade.Track, fade.DurationMilliseconds).WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(second.Path, playback.CurrentTrack?.Path);
        await ended.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(PlaybackCommandKind.TrackStarted, commands.Advance(true, 0, false).Kind);
        Assert.Equal(2, queue.CurrentIndex);
        Assert.Equal(third.Path, playback.CurrentTrack?.Path);
    }

    [Fact]
    public async Task Repeat_track_receives_a_new_end_event_on_each_replay()
    {
        var track = Wave("repeat", 1);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        var queue = new PlaybackQueueCoordinator();
        queue.Replace([track.Path], 0);
        queue.SetRepeatMode("Track");
        var commands = new PlaybackCommandCoordinator(queue, playback, _ => track, () => queue.Entries);
        var ended = System.Threading.Channels.Channel.CreateUnbounded<bool>();
        playback.TrackEnded += (_, _) => ended.Writer.TryWrite(true);
        commands.StartTrack(track, false, 0);
        for (var replay = 0; replay < 2; replay++)
        {
            await ended.Reader.ReadAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
            Assert.Equal(PlaybackCommandKind.TrackStarted, commands.Advance(true, 0, false).Kind);
            Assert.Equal(0, queue.CurrentIndex);
            Assert.False(playback.HasEnded);
        }
        Assert.False(ended.Reader.TryRead(out _));
    }

    [Fact]
    public async Task Failed_crossfade_keeps_the_outgoing_track_end_notification()
    {
        var first = Wave("fade-survivor", 3);
        var missing = Wave("fade-missing", 1);
        File.Delete(missing.Path);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        var failed = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var ended = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        playback.CrossfadeFailed += _ => failed.TrySetResult();
        playback.TrackEnded += (_, _) => ended.TrySetResult();
        playback.Play(first);
        await WaitFor(() => playback.IsPlaying);
        var fade = playback.CrossfadeToAsync(missing, 1200);
        await failed.Task.WaitAsync(TimeSpan.FromSeconds(5));
        await fade.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(first.Path, playback.CurrentTrack?.Path);
        await ended.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.True(playback.HasEnded);
    }

    [Fact]
    public async Task Native_open_error_is_reported_for_the_active_track()
    {
        var broken = Wave("broken", 1);
        File.Delete(broken.Path);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show");
        var failed = new TaskCompletionSource<Track>(TaskCreationOptions.RunContinuationsAsynchronously);
        playback.PlaybackFailed += track => failed.TrySetResult(track);
        playback.Play(broken);
        Assert.Equal(broken.Path, (await failed.Task.WaitAsync(TimeSpan.FromSeconds(5))).Path);
    }

    private Track Wave(string name, int seconds)
    {
        var path = Path.Combine(_root, name + ".wav");
        const int rate = 44100;
        var bytes = rate * seconds * 2;
        using (var writer = new BinaryWriter(File.Create(path)))
        {
            writer.Write(Encoding.ASCII.GetBytes("RIFF")); writer.Write(36 + bytes);
            writer.Write(Encoding.ASCII.GetBytes("WAVEfmt ")); writer.Write(16);
            writer.Write((short)1); writer.Write((short)1); writer.Write(rate); writer.Write(rate * 2);
            writer.Write((short)2); writer.Write((short)16);
            writer.Write(Encoding.ASCII.GetBytes("data")); writer.Write(bytes); writer.Write(new byte[bytes]);
        }
        return new Track(path, name, "", "", "", "", 0, 0, TimeSpan.FromSeconds(seconds), bytes,
            DateTime.UtcNow, DateTime.UtcNow);
    }

    private static async Task WaitFor(Func<bool> condition)
    {
        var deadline = Stopwatch.StartNew();
        while (!condition())
        {
            Assert.True(deadline.Elapsed < TimeSpan.FromSeconds(6), "Native playback did not reach the expected state.");
            await Task.Delay(25);
        }
    }

    public void Dispose() => Directory.Delete(_root, recursive: true);
}
