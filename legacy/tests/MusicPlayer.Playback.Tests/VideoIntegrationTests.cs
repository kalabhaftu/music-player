using System.Runtime.InteropServices;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Playback.Tests;

public sealed class VideoIntegrationTests
{
    [Fact]
    public async Task Opted_in_video_renders_seeks_selects_subtitles_and_captures_a_frame()
    {
        var fixtures = Environment.GetEnvironmentVariable("MUSICPLAYER_MEDIA_FIXTURES")
            ?? throw new InvalidOperationException("Generate video fixtures before validation.");
        var file = new FileInfo(Path.Combine(fixtures, "fixture.mkv"));
        Assert.True(file.Exists);
        var indexed = TrackReader.Read(file.FullName, Path.Combine(fixtures, "artwork"));
        Assert.Equal("fixture", indexed.Title);
        Assert.True(indexed.Duration > TimeSpan.Zero);
        using var surface = new TestSurface();
        using var playback = new PlaybackService("--aout=dummy", "--vout=wingdi", "--no-video-title-show");
        playback.SetVideoExtensions([".mkv"]);
        playback.VideoSurfaceRequested = _ => surface.Handle;
        var track = new Track(file.FullName, "Video fixture", "", "", "", "", 0, 0, TimeSpan.Zero,
            file.Length, file.LastWriteTimeUtc, DateTime.UtcNow);
        playback.Play(track);
        await Until(() => playback.IsPlaying && playback.Position > 300);
        Assert.True(playback.IsVideoMode);
        await Until(() => playback.GetVideoSubtitleOptions().Any(option => option.Id >= 0));
        var subtitle = playback.GetVideoSubtitleOptions().First(option => option.Id >= 0);
        Assert.True(playback.SetVideoSubtitle(subtitle.Id));
        Assert.Equal(subtitle.Id, playback.VideoSubtitleId);
        Assert.True(playback.SetVideoPlaybackRate(1.25f));
        playback.Seek(3000);
        await Until(() => playback.Position > 3100 && playback.IsPlaying);
        var snapshot = Path.Combine(Path.GetTempPath(), "music-player-frame-" + Guid.NewGuid().ToString("N") + ".png");
        try
        {
            Assert.True(playback.TakeVideoSnapshot(snapshot));
            await Until(() => File.Exists(snapshot) && new FileInfo(snapshot).Length > 1000);
            Assert.Equal(new byte[] { 137, 80, 78, 71 }, File.ReadAllBytes(snapshot)[..4]);
        }
        finally { if (File.Exists(snapshot)) File.Delete(snapshot); }
        Assert.True(playback.SetVideoSubtitle(-1));
        playback.Pause();
        await Until(() => !playback.IsPlaying);
    }

    private static async Task Until(Func<bool> ready)
    {
        var deadline = DateTime.UtcNow.AddSeconds(10);
        while (!ready()) { Assert.True(DateTime.UtcNow < deadline, "Native video did not reach the expected state."); await Task.Delay(25); }
    }

    // LibVLC needs a real HWND with a Windows message pump. The test owns this
    // surface and never starts another Music Player application instance.
    private sealed class TestSurface : IDisposable
    {
        private readonly Thread _thread;
        private readonly TaskCompletionSource<nint> _created = new(TaskCreationOptions.RunContinuationsAsynchronously);
        private uint _threadId;
        public nint Handle { get; }
        public TestSurface()
        {
            _thread = new Thread(() =>
            {
                _threadId = GetCurrentThreadId();
                var window = CreateWindowEx(0, "STATIC", "Music Player isolated video test", 0x10000000 | 0x00CF0000,
                    0, 0, 640, 360, 0, 0, 0, 0);
                if (window == 0) { _created.TrySetException(new InvalidOperationException("Could not create the video test surface.")); return; }
                _created.TrySetResult(window);
                try { while (GetMessage(out var message, 0, 0, 0) > 0) { TranslateMessage(ref message); DispatchMessage(ref message); } }
                finally { DestroyWindow(window); }
            }) { IsBackground = true };
            _thread.Start();
            Handle = _created.Task.WaitAsync(TimeSpan.FromSeconds(10)).GetAwaiter().GetResult();
        }
        public void Dispose() { PostThreadMessage(_threadId, 0x0012, 0, 0); if (!_thread.Join(5000)) throw new TimeoutException("Video surface message pump did not stop."); }
        [StructLayout(LayoutKind.Sequential)] private struct Message { public nint Window; public uint Id; public nuint WParam; public nint LParam; public uint Time; public int X; public int Y; public uint Private; }
        [DllImport("user32.dll", EntryPoint="CreateWindowExW", CharSet=CharSet.Unicode)] private static extern nint CreateWindowEx(uint extendedStyle, string className, string title, uint style, int x, int y, int width, int height, nint parent, nint menu, nint instance, nint parameter);
        [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
        [DllImport("user32.dll")] private static extern int GetMessage(out Message message, nint window, uint min, uint max);
        [DllImport("user32.dll")] private static extern bool TranslateMessage(ref Message message);
        [DllImport("user32.dll")] private static extern nint DispatchMessage(ref Message message);
        [DllImport("user32.dll")] private static extern bool PostThreadMessage(uint threadId, uint message, nuint wParam, nint lParam);
        [DllImport("user32.dll")] private static extern bool DestroyWindow(nint window);
    }
}
