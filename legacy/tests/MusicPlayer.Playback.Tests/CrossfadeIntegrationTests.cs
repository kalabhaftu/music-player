using System.Diagnostics;
using System.Reflection;
using System.Text;
using LibVLCSharp.Shared;
using MusicPlayer.App;
using MusicPlayer.Core;
using Xunit;

namespace MusicPlayer.Playback.Tests;

public sealed class WindowsAudioFactAttribute : FactAttribute
{
    private static readonly Lazy<bool> HasOutput = new(() =>
    {
        LibVLCSharp.Shared.Core.Initialize();
        using var engine = new LibVLC("--aout=dummy", "--no-volume-save");
        return engine.AudioOutputDevices("mmdevice").Any(device => !string.IsNullOrWhiteSpace(device.DeviceIdentifier));
    });

    public WindowsAudioFactAttribute()
    {
        if (Environment.GetEnvironmentVariable("MUSICPLAYER_TEST_WINDOWS_AUDIO") != "1" && !HasOutput.Value)
            Skip = "No Windows audio endpoint was detected; MUSICPLAYER_TEST_WINDOWS_AUDIO=1 requires the hardware checks explicitly.";
    }
}

public sealed class CrossfadeIntegrationTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "music-player-crossfade-" + Guid.NewGuid().ToString("N"));
    public CrossfadeIntegrationTests() => Directory.CreateDirectory(_root);

    [Fact]
    public void Fade_preserves_audio_power_and_has_silent_endpoints()
    {
        Assert.Equal((75, 0), PlaybackService.CrossfadeVolumes(75, 0));
        Assert.Equal((0, 75), PlaybackService.CrossfadeVolumes(75, 1));
        for (var step = 1; step < 100; step++)
        {
            var (outgoing, incoming) = PlaybackService.CrossfadeVolumes(75, step / 100d);
            // Windows VLC volume is cubed into amplitude; power is amplitude squared.
            var power = Math.Pow(outgoing / 75d, 6) + Math.Pow(incoming / 75d, 6);
            Assert.InRange(power, .92, 1.08);
        }
    }

    [WindowsAudioFact]
    public async Task Windows_music_players_have_independent_native_volume()
    {
        LibVLCSharp.Shared.Core.Initialize();
        using var engine = new LibVLC("--aout=directsound", "--no-volume-save");
        using var first = new MediaPlayer(engine);
        using var second = new MediaPlayer(engine);
        var track = Wave("independent");
        using var media = new Media(engine, new Uri(track.Path));
        first.Media = media; second.Media = media;
        first.Volume = 75; second.Volume = 0;
        Assert.True(first.Play()); Assert.True(second.Play());
        await WaitFor(() => first.IsPlaying && second.IsPlaying && second.Time > 0);
        second.Volume = 35;
        await Task.Delay(80);
        Assert.Equal(75, first.Volume);
        first.Volume = 17;
        await Task.Delay(80);
        Assert.Equal(35, second.Volume);
    }

    [WindowsAudioFact]
    public async Task Changing_master_volume_preserves_the_running_fade()
    {
        using var playback = new PlaybackService();
        playback.Play(Wave("outgoing"));
        await WaitFor(() => playback.IsPlaying && playback.Position > 0);
        var fade = playback.CrossfadeToAsync(Wave("incoming"), 1500);
        await WaitFor(() => Field<double>(playback, "_crossfadeProgress") is > .3 and < .7);
        lock (Field<object>(playback, "_gate"))
        {
            playback.Volume = 40;
            var outgoing = Field<MediaPlayer>(playback, "_active").Volume;
            var incoming = Field<MediaPlayer>(playback, "_spare").Volume;
            Assert.InRange(outgoing, 1, 39);
            Assert.InRange(incoming, 1, 39);
            Assert.InRange(Math.Pow(outgoing / 40d, 6) + Math.Pow(incoming / 40d, 6), .85, 1.15);
        }
        await fade.WaitAsync(TimeSpan.FromSeconds(6));
        Assert.Equal("incoming", playback.CurrentTrack?.Title);
        Assert.Equal(40, Field<MediaPlayer>(playback, "_active").Volume);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task Seeking_or_pausing_during_a_fade_keeps_the_outgoing_track(bool pause)
    {
        using var playback = new PlaybackService("--aout=dummy");
        var outgoing = Wave("cancel-outgoing");
        playback.Play(outgoing);
        await WaitFor(() => playback.IsPlaying && playback.Position > 0);
        var completed = 0;
        playback.CrossfadeCompleted += _ => completed++;
        var fade = playback.CrossfadeToAsync(Wave("cancel-incoming"), 2000);
        await WaitFor(() => Field<double>(playback, "_crossfadeProgress") > .1);
        if (pause) playback.Pause(); else playback.Seek(2000);
        await fade.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(outgoing.Path, playback.CurrentTrack?.Path);
        Assert.Equal(0, completed);
        if (pause) Assert.False(playback.IsPlaying);
        else await WaitFor(() => playback.IsPlaying && playback.Position > 2100);
    }

    [WindowsAudioFact]
    public async Task Selecting_a_windows_endpoint_preserves_music_position()
    {
        LibVLCSharp.Shared.Core.Initialize();
        using var engine = new LibVLC("--aout=dummy");
        var endpoint = engine.AudioOutputDevices("mmdevice").First(device => !string.IsNullOrWhiteSpace(device.DeviceIdentifier));
        var mapped = AudioOutputRouting.DirectSoundDeviceId(endpoint.DeviceIdentifier);
        Assert.True(Guid.TryParse(mapped, out _));
        Assert.Contains(engine.AudioOutputDevices("directx"), device => string.Equals(device.DeviceIdentifier, mapped, StringComparison.OrdinalIgnoreCase));
        using var playback = new PlaybackService();
        playback.Play(Wave("routing"));
        await WaitFor(() => playback.IsPlaying && playback.Position > 500);
        var position = playback.Position;
        playback.SelectAudioOutputDevice(endpoint.DeviceIdentifier);
        await WaitFor(() => playback.IsPlaying && playback.Position > position + 100);
        Assert.Equal("routing", playback.CurrentTrack?.Title);
    }

    [Fact]
    public void Winrt_endpoint_wrapper_is_normalized_without_guessing_the_device_guid()
    {
        const string endpoint = "{0.0.0.00000000}.{12345678-1234-1234-1234-123456789abc}";
        Assert.Equal(endpoint, AudioOutputRouting.EndpointId(@"\\?\SWD#MMDEVAPI#" + endpoint + "#{e6327cad-dcec-4949-ae8a-991e976a79d2}"));
        Assert.Equal(endpoint, AudioOutputRouting.EndpointId(endpoint));
    }

    private static T Field<T>(PlaybackService playback, string name)
        => (T)typeof(PlaybackService).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(playback)!;

    private Track Wave(string name)
    {
        var path = Path.Combine(_root, name + ".wav");
        const int rate = 44100, seconds = 8, bytes = rate * seconds * 2;
        using var writer = new BinaryWriter(File.Create(path));
        writer.Write(Encoding.ASCII.GetBytes("RIFF")); writer.Write(36 + bytes);
        writer.Write(Encoding.ASCII.GetBytes("WAVEfmt ")); writer.Write(16);
        writer.Write((short)1); writer.Write((short)1); writer.Write(rate); writer.Write(rate * 2);
        writer.Write((short)2); writer.Write((short)16);
        writer.Write(Encoding.ASCII.GetBytes("data")); writer.Write(bytes); writer.Write(new byte[bytes]);
        return new Track(path, name, "", "", "", "", 0, 0, TimeSpan.FromSeconds(seconds), bytes, DateTime.UtcNow, DateTime.UtcNow);
    }

    private static async Task WaitFor(Func<bool> condition)
    {
        var timer = Stopwatch.StartNew();
        while (!condition())
        {
            // This fixture is eight seconds long and the fade begins in its
            // final two seconds. Leave room for ARM64 decoder startup before
            // expecting the fade progress to advance.
            Assert.True(timer.Elapsed < TimeSpan.FromSeconds(10), "Native crossfade did not reach the expected state.");
            await Task.Delay(20);
        }
    }
    public void Dispose() => Directory.Delete(_root, recursive: true);
}
