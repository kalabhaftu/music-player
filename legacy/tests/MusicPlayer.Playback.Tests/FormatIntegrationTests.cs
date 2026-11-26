using System.Diagnostics;
using System.Reflection;
using System.Collections.Concurrent;
using LibVLCSharp.Shared;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Playback.Tests;

public sealed class FormatIntegrationTests
{
    [Theory]
    [InlineData("mp3")]
    [InlineData("wav")]
    [InlineData("wave")]
    [InlineData("flac")]
    [InlineData("aac")]
    [InlineData("m4a")]
    [InlineData("m4b")]
    [InlineData("ogg")]
    [InlineData("oga")]
    [InlineData("opus")]
    [InlineData("wma")]
    [InlineData("ape")]
    [InlineData("wv")]
    [InlineData("tta")]
    [InlineData("mpc")]
    [InlineData("aiff")]
    [InlineData("aif")]
    [InlineData("dsf")]
    [InlineData("dff")]
    public async Task Representative_codec_decodes_and_seeks(string extension)
    {
        var root = Environment.GetEnvironmentVariable("MUSICPLAYER_MEDIA_FIXTURES")
            ?? throw new InvalidOperationException("Run Prepare-MediaFixtures.ps1 and set MUSICPLAYER_MEDIA_FIXTURES before the full format suite.");
        var file = new FileInfo(Path.Combine(root, "fixture." + extension));
        Assert.True(file.Exists, "Genuine format fixture is missing.");
        Assert.True(LibraryScanner.IsSupportedAudioFile(file.FullName));
        var track = new Track(file.FullName, extension, "", "", "", "", 0, 0,
            TimeSpan.Zero, file.Length, file.LastWriteTimeUtc, DateTime.UtcNow);
        using var playback = new PlaybackService("--aout=dummy", "--no-video-title-show", "--verbose=2");
        var native = (MediaPlayer)typeof(PlaybackService).GetField("_active", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(playback)!;
        var engine = (LibVLC)typeof(PlaybackService).GetField("_libVlc", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(playback)!;
        var logs = new ConcurrentQueue<string>();
        engine.Log += (sender, args) => { logs.Enqueue(args.FormattedLog); if (logs.Count > 80) logs.TryDequeue(out _); };
        var stage = "start";
        var failed = false;
        playback.PlaybackFailed += _ => failed = true;
        playback.Play(track);
        string State() => $"stage={stage}, playing={playback.IsPlaying}, position={playback.Position}, duration={playback.Duration}, ended={playback.HasEnded}, nativeState={native.State}, canPause={native.CanPause}, seekable={native.IsSeekable}\n" + string.Join("\n", logs);
        await WaitFor(() => playback.IsPlaying && playback.Position > 300, () => failed, State);
        Assert.True(playback.Duration > 1000, "The decoder did not report a media duration.");
        stage = "seek";
        playback.Seek(3000);
        await WaitFor(() => playback.Position > 3100 && playback.IsPlaying, () => failed, State);
        stage = "pause";
        playback.Pause();
        await WaitFor(() => !playback.IsPlaying && native.State == VLCState.Paused, () => failed, State);
        var pausedAt = playback.Position;
        await Task.Delay(200);
        Assert.False(playback.IsPlaying);
        Assert.InRange(playback.Position, pausedAt - 100, pausedAt + 100);
        stage = "resume";
        playback.PlayLoaded();
        await WaitFor(() => playback.IsPlaying, () => failed, State);
        if (extension == "dff")
        {
            Assert.Equal(TimeSpan.FromSeconds(12), TrackReader.Read(file.FullName, Path.Combine(root, "artwork")).Duration);
            stage = "end and restart";
            playback.Seek(playback.Duration - 500);
            await WaitFor(() => playback.HasEnded, () => failed, State);
            playback.Seek(3000);
            await WaitFor(() => playback.IsPlaying && playback.Position > 3100, () => failed, State);
            playback.Dispose();
            using var released = File.Open(file.FullName, FileMode.Open, FileAccess.Read, FileShare.None);
            Assert.Equal(file.Length, released.Length);
        }
    }

    private static async Task WaitFor(Func<bool> ready, Func<bool> failed, Func<string> state)
    {
        var timer = Stopwatch.StartNew();
        while (!ready())
        {
            Assert.False(failed(), "The shipped LibVLC build rejected this format.");
            Assert.True(timer.Elapsed < TimeSpan.FromSeconds(12), "Native decoder state did not become ready: " + state());
            await Task.Delay(25);
        }
    }
}
